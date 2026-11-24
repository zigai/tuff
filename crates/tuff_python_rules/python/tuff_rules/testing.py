"""Helpers for unit-testing custom rules without running Tuff."""

from __future__ import annotations

import ast
from pathlib import Path
from typing import Any

from tuff_rules import Finding, RuleContext, TuffRule

__all__ = ["apply_edits", "check_source"]


def check_source(
    rule: TuffRule | type[TuffRule],
    source: str,
    *,
    file_path: str | Path = "example.py",
    options: dict[str, Any] | None = None,
) -> list[Finding]:
    """Run one rule over `source` and return its findings in report order."""
    instance = rule() if isinstance(rule, type) else rule
    ctx = RuleContext(source, file_path=file_path, options=options, rule=instance.name)
    instance.check(ast.parse(source, filename=str(file_path)), ctx)
    return ctx.findings


def apply_edits(source: str, findings: list[Finding]) -> str:
    """Apply every finding's edits, which must not overlap, and return the new source."""
    edits = sorted(
        (edit for finding in findings for edit in finding.edits),
        key=lambda edit: (edit.start, edit.end),
    )
    data = source.encode("utf-8")
    output = bytearray()
    position = 0
    for edit in edits:
        if edit.start < position:
            raise ValueError(f"overlapping edit at byte {edit.start}")
        output += data[position : edit.start]
        output += edit.replacement.encode("utf-8")
        position = edit.end
    output += data[position:]
    return output.decode("utf-8")
