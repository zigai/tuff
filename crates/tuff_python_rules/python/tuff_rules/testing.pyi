# ruff: file-ignore[docstring-in-stub]
from pathlib import Path
from typing import Any

from tuff_rules import Finding, TuffRule

def check_source(
    rule: TuffRule | type[TuffRule],
    source: str,
    *,
    file_path: str | Path = "example.py",
    options: dict[str, Any] | None = None,
) -> list[Finding]:
    """Run one rule over `source` and return its findings in report order."""

def apply_edits(source: str, findings: list[Finding]) -> str:
    """Apply every finding's edits, which must not overlap, and return the new source."""
