# Python rules

`tuff check` can run lint rules written in Python and kept in your repository. Rules run in separate CPython worker processes.

## Configuration

```toml
[tool.tuff.lint]
plugin-dirs = ["lint_rules"]
python = ".venv/bin/python"
plugin-workers = 4
disable = ["no-bare-todo"]

[tool.tuff.lint.rules.no-print-call]
replacement = "logger.info"

[tool.ruff.lint.per-file-ignores]
"scripts/*" = ["no-print-call"]
```

- `plugin-dirs`: directories, relative to the configuration file, searched recursively for rule modules and added to `sys.path`. Tuff skips `tests`, `__pycache__`, hidden directories, and `_*.py` helper files.
- `python`: the interpreter. A bare name is looked up on `PATH`; other paths are relative to the configuration file. Default: `$VIRTUAL_ENV`, the project's `.venv`, then `python3` or `python`.
- `plugin-workers`: the maximum number of worker processes. Defaults to the number of available CPUs. Workers start only when a file passes a rule's filters.

Discovered rules are enabled by default. Use `disable` with a rule name or `all`, and `enable` to turn rules back on. Unknown names are errors, and a Python rule name must not match a native Tuff or Ruff rule name.

`[tool.tuff.lint.rules.<name>]` is passed to the rule as `ctx.options`. Ruff's `per-file-ignores` accepts Python rule names. Python rules skip notebooks.

## Writing a rule

Subclass `TuffRule` and set `name` to a lower kebab-case name:

```python
import ast

from tuff_rules import RuleContext, TuffRule


class NoPrintCall(TuffRule):
    name = "no-print-call"
    description = "Use the project logger instead of print()."
    exclude = ("scripts/**",)
    source_patterns = ("print",)
    node_kinds = ("Call",)

    def check(self, tree: ast.Module, ctx: RuleContext) -> None:
        replacement = ctx.options.get("replacement", "logger.info")
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and ctx.qualified_name(node.func) == "builtins.print":
                ctx.report(
                    node.func,
                    f"Use `{replacement}` instead of `print`.",
                    [ctx.edit(node.func, replacement)],
                )
```

`@tuff_rule` turns a `check(tree, ctx)` function into a rule. Its keyword arguments match the class attributes, and the docstring becomes the description:

```python
import ast

from tuff_rules import RuleContext, tuff_rule


@tuff_rule("no-bare-todo", source_patterns=("TODO",))
def no_bare_todo(tree: ast.Module, ctx: RuleContext) -> None:
    """Require an owner on TODO comments."""
    for span in ctx.comments:
        text = ctx.text(span)
        if "TODO" in text and "TODO(" not in text:
            ctx.report(span, "Add an owner: `TODO(name)`.")
```

Every `TuffRule` subclass defined in a module is loaded. To choose the rules explicitly, define a module-level `RULES` sequence of rule classes or instances.

### Rule attributes

| Attribute         | Default | Meaning                                                               |
| ----------------- | ------- | --------------------------------------------------------------------- |
| `name`            | none    | Rule name used in configuration and ignore comments.                  |
| `description`     | `""`    | Short description.                                                    |
| `include`         | `()`    | Path globs relative to the project root. Empty means every file.      |
| `exclude`         | `()`    | Path globs to skip.                                                   |
| `source_patterns` | `()`    | Substrings; the file must contain at least one.                       |
| `node_kinds`      | `()`    | `ast` class names such as `Call`; the file must contain at least one. |
| `watch_files`     | `()`    | Files, relative to the project root, whose contents affect findings.  |
| `cacheable`       | `True`  | Set to `False` when findings depend on state Tuff cannot observe.     |

Tuff applies `include`, `exclude`, `source_patterns`, and `node_kinds` before sending a file to Python. In globs, `*` does not cross `/`.

### `RuleContext`

| Member                                         | Description                                                                                                                           |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `source`, `file_path`, `relative_path`, `root` | The checked source and its paths.                                                                                                     |
| `options`                                      | The rule's configured options as a `dict`.                                                                                            |
| `comments`                                     | Byte spans of every comment, including `#`.                                                                                           |
| `qualified_name(node)`                         | Qualified name of an `ast.Name` or `ast.Attribute`, following imports; builtins resolve to `builtins.<name>`. `None` when unresolved. |
| `node_span(node)`                              | Byte span of a node.                                                                                                                  |
| `text(node_or_span)`                           | Source text of a node or span.                                                                                                        |
| `edit(node_or_span, replacement)`              | Build an `Edit`.                                                                                                                      |
| `report(node_or_span, message, edits=None)`    | Report a finding. Edits become a fix applied by `tuff check --fix`.                                                                   |

Offsets are UTF-8 byte offsets into the source. `Edit(start, end, replacement)` replaces `start..end`. Do not mutate `tree`.

An exception raised by a rule is reported as an error for that file.

## Testing

`tuff_rules.testing` runs a rule without Tuff:

```python
from no_print import NoPrintCall
from tuff_rules.testing import apply_edits, check_source


def test_reports_print() -> None:
    findings = check_source(NoPrintCall, "print(1)\n")
    assert [finding.message for finding in findings] == ["Use `logger.info` instead of `print`."]


def test_fix_uses_option() -> None:
    source = "print(1)\n"
    findings = check_source(NoPrintCall, source, options={"replacement": "log"})
    assert apply_edits(source, findings) == "log(1)\n"
```

- `check_source(rule, source, *, file_path="example.py", options=None)` returns the findings in report order.
- `apply_edits(source, findings)` applies all non-overlapping edits.

Outside Tuff, `comments` and `qualified_name` are derived with the standard library. Name resolution follows imports and builtins but not other bindings.

The `tuff_rules` package is in the Tuff source tree at `crates/tuff_python_rules/python`. Add that directory and your plugin directory to `PYTHONPATH` to run the tests:

```sh
PYTHONPATH=path/to/tuff/crates/tuff_python_rules/python:lint_rules pytest
```

## Suppression

```python
print(value)  # tuff: ignore[no-print-call]
```

Ignore comments follow the rules in [Native lint rules](native-linting.md#ignore-comments).

## Caching

Lint results are cached. The cache is invalidated when any file in `plugin-dirs` changes, including `_*.py` helpers, when the interpreter or a rule's options change, or when a file in `watch_files` changes. While an enabled rule sets `cacheable = False`, results are not reused between runs.

## Editors

`tuff server` uses the same configuration. It reports Python rule diagnostics with their fixes and `# tuff: ignore[<rule>]` suppression actions, and shows rule details on hover over `# tuff: ignore[...]` comments. Workers stay warm across edits and reload when files in `plugin-dirs` or `watch_files` change.
