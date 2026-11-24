"""Implementation of the `tuff_rules` API.

All offsets are UTF-8 byte offsets into the checked source, matching the
`lineno`/`col_offset` positions that `ast` reports.
"""

from __future__ import annotations

import ast
import builtins
import io
import tokenize
from pathlib import Path
from typing import (
    Any,
    Callable,
    Iterable,
    Iterator,
    NamedTuple,
    Tuple,
    Union,
)

Span = Tuple[int, int]
NodeOrSpan = Union[ast.AST, Span]


class Edit(NamedTuple):
    """Replace the bytes in `start..end` with `replacement`."""

    start: int
    end: int
    replacement: str


class Finding(NamedTuple):
    rule: str
    start: int
    end: int
    message: str
    edits: tuple[Edit, ...] = ()


class TuffRule:
    """Base class for a custom rule.

    Subclasses defined in a plugin module are discovered automatically. A module can
    instead list its rules in a module-level `RULES` sequence.
    """

    name: str = ""
    description: str = ""
    include: tuple[str, ...] = ()
    exclude: tuple[str, ...] = ()
    source_patterns: tuple[str, ...] = ()
    node_kinds: tuple[str, ...] = ()
    watch_files: tuple[str, ...] = ()
    cacheable: bool = True

    def check(self, tree: ast.Module, ctx: RuleContext) -> None:
        raise NotImplementedError


def tuff_rule(
    name: str,
    *,
    description: str = "",
    include: Iterable[str] = (),
    exclude: Iterable[str] = (),
    source_patterns: Iterable[str] = (),
    node_kinds: Iterable[str] = (),
    watch_files: Iterable[str] = (),
    cacheable: bool = True,
) -> Callable[[Callable[[ast.Module, RuleContext], None]], type]:
    """Turn a `check(tree, ctx)` function into a `TuffRule` subclass."""

    def decorate(function: Callable[[ast.Module, RuleContext], None]) -> type:
        def check(self: TuffRule, tree: ast.Module, ctx: RuleContext) -> None:
            function(tree, ctx)

        return type(
            function.__name__,
            (TuffRule,),
            {
                "__module__": function.__module__,
                "__doc__": function.__doc__,
                "name": name,
                "description": description or (function.__doc__ or "").strip(),
                "include": tuple(include),
                "exclude": tuple(exclude),
                "source_patterns": tuple(source_patterns),
                "node_kinds": tuple(node_kinds),
                "watch_files": tuple(watch_files),
                "cacheable": cacheable,
                "check": check,
            },
        )

    return decorate


class RuleContext:
    """Per-file facts and the reporting interface passed to `TuffRule.check`.

    Tuff supplies `comments` and `qualified_names` from its own analysis. When they are
    omitted, as in unit tests, they are derived from `source` with the standard library.
    """

    def __init__(
        self,
        source: str,
        *,
        file_path: str | Path = "example.py",
        relative_path: str | None = None,
        root: str | Path | None = None,
        options: dict[str, Any] | None = None,
        comments: Iterable[Span] | None = None,
        qualified_names: Iterable[tuple[int, int, str]] | None = None,
        rule: str = "",
    ) -> None:
        self.source = source
        self.file_path = Path(file_path)
        self.root = Path(root) if root is not None else self.file_path.parent
        self.relative_path = (
            relative_path if relative_path is not None else self.file_path.name
        )
        self.options: dict[str, Any] = dict(options or {})
        self.rule = rule
        self.findings: list[Finding] = []
        self._bytes = source.encode("utf-8")
        self._line_starts = _line_starts(self._bytes)
        self._comments = (
            None
            if comments is None
            else [(int(start), int(end)) for start, end in comments]
        )
        self._names = (
            None
            if qualified_names is None
            else {(start, end): name for start, end, name in qualified_names}
        )

    def _derive(self, rule: str, options: dict[str, Any] | None) -> RuleContext:
        derived = object.__new__(RuleContext)
        derived.__dict__.update(self.__dict__)
        derived.rule = rule
        derived.options = dict(options or {})
        derived.findings = []
        return derived

    @property
    def comments(self) -> list[Span]:
        if self._comments is None:
            self._comments = _scan_comments(self.source, self._line_starts)
        return self._comments

    def node_span(self, node: ast.AST) -> Span:
        lineno = getattr(node, "lineno", None)
        end_lineno = getattr(node, "end_lineno", None)
        if lineno is None or end_lineno is None:
            raise ValueError(f"{type(node).__name__} node has no source location")
        start = self._line_starts[lineno - 1] + node.col_offset
        end = self._line_starts[end_lineno - 1] + node.end_col_offset
        return start, end

    def span(self, node_or_span: NodeOrSpan) -> Span:
        if isinstance(node_or_span, ast.AST):
            return self.node_span(node_or_span)
        start, end = node_or_span
        return int(start), int(end)

    def text(self, node_or_span: NodeOrSpan) -> str:
        start, end = self.span(node_or_span)
        return self._bytes[start:end].decode("utf-8")

    def qualified_name(self, node: ast.AST) -> str | None:
        """Return the fully qualified name an `ast.Name` or `ast.Attribute` refers to.

        Imports are followed through aliases, so after `import foo as bar`,
        `bar.baz` resolves to `foo.baz`. Builtins resolve to `builtins.<name>`.
        """
        if not isinstance(node, (ast.Name, ast.Attribute)):
            return None
        if self._names is None:
            self._names = _resolve_names(self.source, self)
        return self._names.get(self.node_span(node))

    def edit(self, node_or_span: NodeOrSpan, replacement: str) -> Edit:
        start, end = self.span(node_or_span)
        return Edit(start, end, replacement)

    def report(
        self,
        node_or_span: NodeOrSpan,
        message: str,
        edits: Iterable[Edit] | None = None,
    ) -> None:
        start, end = self.span(node_or_span)
        self.findings.append(
            Finding(
                self.rule,
                start,
                end,
                str(message),
                tuple(Edit(*edit) for edit in edits or ()),
            )
        )


def _line_starts(data: bytes) -> list[int]:
    starts = [0]
    index = 0
    length = len(data)
    while index < length:
        byte = data[index]
        if byte == 0x0A:
            starts.append(index + 1)
        elif byte == 0x0D:
            if index + 1 < length and data[index + 1] == 0x0A:
                index += 1
            starts.append(index + 1)
        index += 1
    return starts


def _scan_comments(source: str, line_starts: list[int]) -> list[Span]:
    lines = source.splitlines(keepends=True)
    comments = []
    try:
        tokens = list(tokenize.generate_tokens(io.StringIO(source).readline))
    except (tokenize.TokenError, SyntaxError):
        return comments
    for token in tokens:
        if token.type != tokenize.COMMENT:
            continue
        row, column = token.start
        prefix = lines[row - 1][:column].encode("utf-8")
        start = line_starts[row - 1] + len(prefix)
        comments.append((start, start + len(token.string.encode("utf-8"))))
    return comments


_SCOPES = (
    ast.FunctionDef,
    ast.AsyncFunctionDef,
    ast.Lambda,
    ast.ClassDef,
    ast.ListComp,
    ast.SetComp,
    ast.DictComp,
    ast.GeneratorExp,
)

# Local, non-import bindings shadow outer names but have no qualified name.
_LOCAL = ""


def _same_scope(node: ast.AST) -> Iterator[ast.AST]:
    for child in ast.iter_child_nodes(node):
        yield child
        if not isinstance(child, _SCOPES):
            yield from _same_scope(child)


def _scope_bindings(scope: ast.AST) -> dict[str, str]:
    bindings: dict[str, str] = {}
    for node in _same_scope(scope):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.asname is not None:
                    bindings.setdefault(alias.asname, alias.name)
                else:
                    head = alias.name.split(".")[0]
                    bindings.setdefault(head, head)
        elif isinstance(node, ast.ImportFrom):
            for alias in node.names:
                if alias.name == "*":
                    continue
                qualified = (
                    f"{node.module}.{alias.name}"
                    if node.level == 0 and node.module
                    else _LOCAL
                )
                bindings.setdefault(alias.asname or alias.name, qualified)
        elif isinstance(node, ast.Name) and not isinstance(node.ctx, ast.Load):
            bindings.setdefault(node.id, _LOCAL)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            bindings.setdefault(node.name, _LOCAL)
        elif isinstance(node, ast.arg):
            bindings.setdefault(node.arg, _LOCAL)
    return bindings


def _resolve_names(source: str, ctx: RuleContext) -> dict[Span, str]:
    """Approximate Tuff's name resolution for code checked outside Tuff.

    Bindings are flow-insensitive: the first binding of a name in a scope applies to
    the whole scope.
    """
    names: dict[Span, str] = {}

    def lookup(name: str, chain: list[dict[str, str]]) -> str | None:
        for bindings in reversed(chain):
            if name in bindings:
                return bindings[name] or None
        return f"builtins.{name}" if hasattr(builtins, name) else None

    def resolve(node: ast.AST, chain: list[dict[str, str]]) -> str | None:
        if isinstance(node, ast.Name):
            return lookup(node.id, chain)
        if isinstance(node, ast.Attribute):
            head = resolve(node.value, chain)
            return None if head is None else f"{head}.{node.attr}"
        return None

    def visit(scope: ast.AST, chain: list[dict[str, str]]) -> None:
        chain = [*chain, _scope_bindings(scope)]
        # Class bodies are not visible from the functions nested in them.
        inner = chain[:-1] if isinstance(scope, ast.ClassDef) else chain
        for node in _same_scope(scope):
            if isinstance(node, _SCOPES):
                visit(node, inner)
            elif isinstance(node, (ast.Name, ast.Attribute)):
                name = resolve(node, chain)
                if name is not None:
                    names[ctx.node_span(node)] = name

    visit(ast.parse(source), [])
    return names
