"""Rule worker that Tuff drives over length-prefixed JSON frames on stdin/stdout."""

from __future__ import annotations

import ast
import importlib.util
import json
import os
import struct
import sys
import traceback
from pathlib import Path
from typing import Any, BinaryIO

from tuff_rules import RuleContext, TuffRule

_HEADER = struct.Struct(">I")
_TUPLE_FIELDS = ("include", "exclude", "source_patterns", "node_kinds", "watch_files")


class PluginError(Exception):
    pass


def read_frame(stream: BinaryIO) -> Any | None:
    header = stream.read(_HEADER.size)
    if not header:
        return None
    if len(header) != _HEADER.size:
        raise EOFError("truncated frame header")
    (length,) = _HEADER.unpack(header)
    payload = stream.read(length)
    if len(payload) != length:
        raise EOFError("truncated frame payload")
    return json.loads(payload.decode("utf-8"))


def write_frame(stream: BinaryIO, message: Any) -> None:
    payload = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode(
        "utf-8"
    )
    stream.write(_HEADER.pack(len(payload)))
    stream.write(payload)
    stream.flush()


class Worker:
    def __init__(self) -> None:
        self.root = Path()
        self.rules: dict[str, TuffRule] | None = None

    def handle(self, request: dict[str, Any]) -> Any:
        command = request.get("command")
        if command == "manifest":
            return self.manifest(
                request["root"], request["plugin_dirs"], request["files"]
            )
        if command == "check_batch":
            return [self.check_file(file) for file in request["files"]]
        raise PluginError(f"unknown worker command {command!r}")

    def manifest(
        self, root: str, plugin_dirs: list[str], files: list[str]
    ) -> dict[str, Any]:
        if self.rules is not None:
            raise PluginError("rules are already loaded")
        self.root = Path(root)
        for directory in reversed(plugin_dirs):
            if directory not in sys.path:
                sys.path.insert(0, directory)
        rules: dict[str, TuffRule] = {}
        origins: dict[str, str] = {}
        entries = []
        for index, file in enumerate(files):
            for rule in _load_module(index, file):
                entry = _describe(rule, file)
                if rule.name in origins:
                    raise PluginError(
                        f"rule `{rule.name}` in {file} is already defined in "
                        f"{origins[rule.name]}"
                    )
                rules[rule.name] = rule
                origins[rule.name] = file
                entries.append(entry)
        self.rules = rules
        return {
            "python_version": ".".join(str(part) for part in sys.version_info[:3]),
            "rules": entries,
        }

    def check_file(self, file: dict[str, Any]) -> dict[str, Any]:
        if self.rules is None:
            raise PluginError("rules are not loaded")
        findings: list[dict[str, Any]] = []
        errors: list[dict[str, Any]] = []
        source = file["source"]
        try:
            tree = ast.parse(source, filename=file["path"])
        except SyntaxError as error:
            errors.append(
                {"rule": None, "message": f"Python could not parse the file: {error}"}
            )
            return {"findings": findings, "errors": errors}
        base = RuleContext(
            source,
            file_path=file["path"],
            relative_path=file["rel_path"],
            root=self.root,
            comments=file["comments"],
            qualified_names=file["qualified_names"],
        )
        options_by_rule = file["options_by_rule"]
        for name in file["active_rules"]:
            rule = self.rules.get(name)
            if rule is None:
                errors.append(
                    {"rule": name, "message": "rule is not loaded by the worker"}
                )
                continue
            ctx = base._derive(name, options_by_rule.get(name))
            try:
                rule.check(tree, ctx)
            except Exception as error:  # ruff: ignore[blind-except]
                errors.append(
                    {
                        "rule": name,
                        "message": "".join(
                            traceback.format_exception(
                                type(error), error, _caller_frames(error)
                            )
                        ).rstrip(),
                    }
                )
                continue
            for finding in ctx.findings:
                findings.append(
                    {
                        "rule": name,
                        "start": finding.start,
                        "end": finding.end,
                        "message": finding.message,
                        "edits": [
                            {
                                "start": int(edit.start),
                                "end": int(edit.end),
                                "replacement": str(edit.replacement),
                            }
                            for edit in finding.edits
                        ],
                    }
                )
        return {"findings": findings, "errors": errors}


def _caller_frames(error: BaseException) -> Any:
    frames = error.__traceback__
    return frames.tb_next if frames is not None else None


def _load_module(index: int, file: str) -> list[TuffRule]:
    module_name = f"_tuff_plugin_{index}_{Path(file).stem}"
    spec = importlib.util.spec_from_file_location(module_name, file)
    if spec is None or spec.loader is None:
        raise PluginError(f"cannot import {file}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    try:
        spec.loader.exec_module(module)
    except Exception:  # ruff: ignore[blind-except]
        raise PluginError(
            f"failed to import {file}:\n{traceback.format_exc().rstrip()}"
        ) from None
    declared = getattr(module, "RULES", None)
    if declared is None:
        candidates = [
            value
            for value in vars(module).values()
            if isinstance(value, type)
            and issubclass(value, TuffRule)
            and value.__module__ == module_name
            and value.name
        ]
    else:
        candidates = list(declared)
    rules = []
    for candidate in candidates:
        try:
            rule = candidate() if isinstance(candidate, type) else candidate
        except Exception:  # ruff: ignore[blind-except]
            raise PluginError(
                f"failed to instantiate {candidate!r} in {file}:\n"
                f"{traceback.format_exc().rstrip()}"
            ) from None
        if not isinstance(rule, TuffRule):
            raise PluginError(f"{candidate!r} in {file} is not a TuffRule")
        rules.append(rule)
    return rules


def _describe(rule: TuffRule, file: str) -> dict[str, Any]:
    label = f"rule {type(rule).__name__} in {file}"
    if not isinstance(rule.name, str) or not rule.name:
        raise PluginError(f"{label} needs a non-empty string `name`")
    if not isinstance(rule.description, str):
        raise PluginError(f"{label}: `description` must be a string")
    if not isinstance(rule.cacheable, bool):
        raise PluginError(f"{label}: `cacheable` must be a bool")
    entry: dict[str, Any] = {
        "name": rule.name,
        "description": rule.description,
        "cacheable": rule.cacheable,
        "path": file,
    }
    for field in _TUPLE_FIELDS:
        value = getattr(rule, field)
        if isinstance(value, str) or not all(isinstance(item, str) for item in value):
            raise PluginError(f"{label}: `{field}` must be a sequence of strings")
        entry[field] = list(value)
    return entry


def main() -> None:
    output = os.fdopen(os.dup(1), "wb")
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    stream = sys.stdin.buffer
    worker = Worker()
    while True:
        request = read_frame(stream)
        if request is None:
            return
        try:
            reply = {"result": worker.handle(request)}
        except PluginError as error:
            reply = {"error": str(error)}
        except Exception:  # ruff: ignore[blind-except]
            reply = {"error": traceback.format_exc().rstrip()}
        write_frame(output, reply)
