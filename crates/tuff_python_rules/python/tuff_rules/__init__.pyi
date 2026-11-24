# ruff: file-ignore[docstring-in-stub]
"""Python API for repository-local Tuff lint rules.

Offsets are UTF-8 byte offsets into the checked source.
"""

import ast
from pathlib import Path
from typing import Any, Callable, Iterable, NamedTuple

from typing_extensions import TypeAlias

Span: TypeAlias = tuple[int, int]
NodeOrSpan: TypeAlias = ast.AST | Span

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

    `name` is the lower kebab-case rule name used in configuration and in
    `# tuff: ignore[...]` comments.

    Tuff runs `check` only for files whose path relative to the project root
    matches an `include` glob (or any file when `include` is empty), matches no
    `exclude` glob, contains at least one of `source_patterns` (when given), and
    contains at least one of `node_kinds` (names of `ast` node classes, when given).

    `watch_files` lists files, relative to the project root, whose contents
    affect findings; editing them invalidates Tuff's lint cache. Set `cacheable`
    to `False` when findings depend on state that Tuff cannot observe.
    """

    name: str
    description: str
    include: tuple[str, ...]
    exclude: tuple[str, ...]
    source_patterns: tuple[str, ...]
    node_kinds: tuple[str, ...]
    watch_files: tuple[str, ...]
    cacheable: bool
    def check(self, tree: ast.Module, ctx: RuleContext) -> None:
        """Inspect `tree` and report findings through `ctx`. Do not mutate `tree`."""

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
) -> Callable[[Callable[[ast.Module, RuleContext], None]], type[TuffRule]]:
    """Turn a `check(tree, ctx)` function into a `TuffRule` subclass."""

class RuleContext:
    """Per-file facts and the reporting interface passed to `TuffRule.check`.

    Outside Tuff, `comments` and `qualified_names` default to values derived
    from `source` with the standard library, which follow imports and builtins
    but not other bindings.
    """

    source: str
    file_path: Path
    relative_path: str
    root: Path
    options: dict[str, Any]
    rule: str
    findings: list[Finding]
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
    ) -> None: ...
    @property
    def comments(self) -> list[Span]:
        """Byte spans of every comment, including the leading `#`."""

    def node_span(self, node: ast.AST) -> Span:
        """Byte span of a node that has a source location."""

    def span(self, node_or_span: NodeOrSpan) -> Span: ...
    def text(self, node_or_span: NodeOrSpan) -> str:
        """Source text of a node or byte span."""

    def qualified_name(self, node: ast.AST) -> str | None:
        """Fully qualified name of an `ast.Name` or `ast.Attribute`, following import aliases.

        After `import foo as bar`, `bar.baz` resolves to `foo.baz`; builtins resolve
        to `builtins.<name>`. Returns `None` for unresolved names and other nodes.
        """

    def edit(self, node_or_span: NodeOrSpan, replacement: str) -> Edit: ...
    def report(
        self,
        node_or_span: NodeOrSpan,
        message: str,
        edits: Iterable[Edit] | None = None,
    ) -> None:
        """Report a finding. Edits become a fix that `tuff check --fix` applies."""
