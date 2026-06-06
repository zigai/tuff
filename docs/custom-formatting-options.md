# Custom Formatting Options

Tuff follows Ruff's formatter output unless a Tuff option is enabled. Keep standard formatter settings such as line length, quote style, indentation, and file selection in Ruff config (`[tool.ruff]` and `[tool.ruff.format]`). Put only Tuff-specific style choices under `[tool.tuff.format]`.

## Install Tuff

From the repository root:

```sh
cargo install --path crates/ruff --bin tuff --locked --force
```

Cargo installs the binary as `~/.cargo/bin/tuff`. Make sure `~/.cargo/bin` is on `PATH`.

For a faster development build, use:

```sh
cargo install --path crates/ruff --bin tuff --debug --force
```

## Annotation Alignment

Enable annotation alignment under `[tool.tuff.format.alignment]`.

```toml
[tool.tuff.format.alignment]
class-fields = "enabled"
class-field-scope = "all"
function-params = "enabled"
function-param-scope = "all"
align-defaults = true
assignments = "enabled"
assignment-scope = "class"
dict-values = "enabled"
dict-alignment = "value"
call-keyword-args = "enabled"
import-aliases = "enabled"
collection-rows = "enabled"
repeated-call-args = "enabled"
with-items = "enabled"
trailing-comments = "enabled"
min-group-size = 2
break-on-blank-line = true
break-on-leading-comment = true
break-on-trailing-comment = false
```

`class-fields`, `function-params`, `assignments`, `dict-values`, `call-keyword-args`, `import-aliases`, `collection-rows`, `repeated-call-args`, `with-items`, and `trailing-comments` accept these mode values:

| Value              | Behavior                                                      |
| ------------------ | ------------------------------------------------------------- |
| `"disabled"`       | Do not add alignment padding.                                 |
| `"enabled"`        | Add alignment padding for the configured scope.               |
| `"preserve-input"` | Accepted for future use. Currently behaves like `"disabled"`. |

### Class Fields

`class-fields = "enabled"` aligns annotations for neighboring annotated class attributes.

```python
class Example:
    id: int
    name: str
    enabled: bool = True
```

becomes:

```python
class Example:
    id:      int
    name:    str
    enabled: bool = True
```

Only simple single-line annotated assignments participate in a group. Complex or multiline annotations are left on Ruff's normal formatting path.

By default, tuff also aligns defaults in the same class-field group.

```python
@dataclass
class Example:
    title: str = "example"
    metadata: dict[str, Any] = field(default_factory=dict)
    enabled: bool = False
```

becomes:

```python
@dataclass
class Example:
    title:    str            = "example"
    metadata: dict[str, Any] = field(default_factory=dict)
    enabled:  bool           = False
```

Use `class-field-scope = "dataclasses-and-pydantic"` to limit class field alignment to dataclasses and Pydantic model classes. Use `class-field-scope = "schemas"` to include `TypedDict` classes too. Tuff recognizes `@dataclass`, `@dataclasses.dataclass`, `BaseModel`, `BaseSettings`, and `TypedDict`.

### Function Parameters

`function-params = "enabled"` aligns annotations inside each parameter group.

```python
def create_item(
    id: int,
    name: str,
    enabled: bool = True,
):
    ...
```

becomes:

```python
def create_item(
    id:      int,
    name:    str,
    enabled: bool = True,
):
    ...
```

Parameter separators split alignment groups. Positional-only parameters, keyword-only parameters, `*args`, and `**kwargs` are handled as separate groups instead of being aligned across the entire signature.

By default, tuff also aligns defaults inside an aligned parameter group.

```python
class Example:
    def __init__(
        self,
        *,
        active: bool = True,
        retries: int = 3,
        label: str | None = None,
        timeout_seconds: float | None = 30.0,
        refresh_interval_seconds: float | None = 5.0,
    ) -> None:
        ...
```

becomes:

```python
class Example:
    def __init__(
        self,
        *,
        active:                   bool         = True,
        retries:                  int          = 3,
        label:                    str | None   = None,
        timeout_seconds:          float | None = 30.0,
        refresh_interval_seconds: float | None = 5.0,
    ) -> None:
        ...
```

Set `align-defaults = false` to keep default values on Ruff's normal spacing path while still aligning annotations. This applies to class fields and function parameters.

Use `function-param-scope` to choose where parameter alignment applies:

| Value          | Effect                                                              |
| -------------- | ------------------------------------------------------------------- |
| `"all"`        | Align parameters in functions and methods.                          |
| `"functions"`  | Align module-level and nested functions, but not methods.           |
| `"methods"`    | Align methods, but not module-level or nested functions.            |
| `"class-init"` | Align only `__init__` methods defined directly inside class bodies. |

### Assignments

`assignments = "enabled"` aligns the `=` for neighboring simple assignments.

```python
class Status(StrEnum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED_WITH_WARNINGS = "completed_with_warnings"
```

with:

```toml
[tool.tuff.format.alignment]
assignments = "enabled"
assignment-scope = "class"
```

becomes:

```python
class Status(StrEnum):
    PENDING                 = "pending"
    IN_PROGRESS             = "in_progress"
    COMPLETED_WITH_WARNINGS = "completed_with_warnings"
```

Only simple, single-target assignments participate. The target must be a plain name, and the `=` must be on the target line. Values may format across multiple lines.

Tuff skips tuple unpacking, attribute and subscript assignments, chained assignments, comments attached to the target, and leading comments attached to the value. Use `break-on-trailing-comment` if inline comments should split assignment groups.

Use `assignment-scope` to choose where assignment alignment applies:

| Value                | Effect                                                                                                                                            |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `"all"`              | Align simple assignments in module, class, and function bodies.                                                                                   |
| `"module"`           | Align only module-level assignments.                                                                                                              |
| `"class"`            | Align assignments in class bodies, including enum classes.                                                                                        |
| `"module-and-class"` | Align module-level and class-body assignments.                                                                                                    |
| `"enum-class"`       | Align only assignments inside `Enum`, `IntEnum`, `StrEnum`, `Flag`, and `IntFlag` classes, including mixin forms such as `class Code(str, Enum)`. |

### Grouping Rules

By default, blank lines and leading comments start a new alignment group.

```python
class Example:
    id: int
    name: str

    # state
    enabled: bool
    updated_at: datetime | None
```

becomes:

```python
class Example:
    id:   int
    name: str

    # state
    enabled:    bool
    updated_at: datetime | None
```

The grouping controls are:

| Option                      | Default | Effect                                                                                                                                  |
| --------------------------- | ------: | --------------------------------------------------------------------------------------------------------------------------------------- |
| `class-field-scope`         | `"all"` | Choose whether class field alignment applies to all classes or only dataclasses and Pydantic models.                                    |
| `function-param-scope`      | `"all"` | Choose whether parameter alignment applies to all functions, functions only, methods only, or class `__init__` methods only.            |
| `assignment-scope`          | `"all"` | Choose whether assignment alignment applies everywhere, module-level code, class bodies, module and class bodies, or enum classes only. |
| `align-defaults`            |  `true` | Align `=` for default values in aligned class-field and parameter groups.                                                               |
| `min-group-size`            |     `2` | Minimum number of adjacent items needed before tuff adds alignment padding.                                                             |
| `break-on-blank-line`       |  `true` | Start a new group after a blank line.                                                                                                   |
| `break-on-leading-comment`  |  `true` | Start a new group before an own-line leading comment.                                                                                   |
| `break-on-trailing-comment` | `false` | Start a new group after an item with a trailing comment.                                                                                |

Tuff does not align across `# fmt: off` regions.

### Dict Values

`dict-values = "enabled"` aligns values in neighboring multiline dictionary entries. `dict-alignment = "value"` enables the same value alignment through the newer dictionary alignment option.

```python
options = {
    "path": path,
    "config_path": config_path,
    "enabled": False,
}
```

becomes:

```python
options = {
    "path":        path,
    "config_path": config_path,
    "enabled":     False,
}
```

Only simple single-line values participate. If a dict group contains a complex value that Ruff may expand, tuff leaves that group on Ruff's normal spacing path to keep repeated formatting stable.

Use `dict-alignment = "colon"` to align the colon itself instead of padding after the colon.

```python
options = {
    "path": path,
    "config_path": config_path,
    "enabled": False,
}
```

becomes:

```python
options = {
    "path"       : path,
    "config_path": config_path,
    "enabled"    : False,
}
```

Accepted values for `dict-alignment` are `"none"`, `"value"`, and `"colon"`. The legacy `dict-values = "enabled"` setting is still accepted and is equivalent to `dict-alignment = "value"` unless `dict-alignment` is set explicitly.

### Call Keyword Arguments

`call-keyword-args = "enabled"` aligns keyword arguments in multiline calls.

```python
runner.configure(
    mode="fast",
    retries=3,
    log_output=True,
)
```

becomes:

```python
runner.configure(
    mode       = "fast",
    retries    = 3,
    log_output = True,
)
```

Single-line calls are left alone. Multiline calls with complex keyword values are also left alone, because Ruff may need to reshape those values independently.

### Import Aliases

`import-aliases = "enabled"` aligns `as` aliases in neighboring import statements.

```python
import package.alpha as alpha
import package.beta as beta
import package.submodule.gamma as gamma
```

becomes:

```python
import package.alpha           as alpha
import package.beta            as beta
import package.submodule.gamma as gamma
```

Only single-alias import statements participate. Parenthesized `from ... import (...)` blocks keep Ruff's normal formatting.

### Collection Rows

`collection-rows = "enabled"` aligns columns inside table-like nested list and tuple rows.

```python
rows = [
    ("id", "name", "active"),
    (1, "Ana", True),
    (20, "Benedict", False),
]
```

becomes:

```python
rows = [
    ("id", "name",     "active"),
    (1,    "Ana",      True),
    (20,   "Benedict", False),
]
```

Only rectangular list and tuple rows with simple single-line elements participate.

### Repeated Call Arguments

`repeated-call-args = "enabled"` aligns positional arguments across neighboring calls to the same callee.

```python
router.add_route("GET", "/users", list_users)
router.add_route("POST", "/users", create_user)
router.add_route("DELETE", "/users/{id}", delete_user)
```

becomes:

```python
router.add_route("GET",    "/users",      list_users)
router.add_route("POST",   "/users",      create_user)
router.add_route("DELETE", "/users/{id}", delete_user)
```

Calls must have the same callee, the same positional arity, no keyword arguments, and simple single-line argument values.

### With Items

`with-items = "enabled"` aligns the `as` target in multi-item `with` statements.

```python
with (
    open(input_path) as input_file,
    open(output_path) as output_file,
    lock as acquired_lock,
):
    process()
```

becomes:

```python
with (
    open(input_path)  as input_file,
    open(output_path) as output_file,
    lock              as acquired_lock,
):
    process()
```

Only items with simple single-line context expressions and `as` targets participate.

### Trailing Comments

`trailing-comments = "enabled"` aligns neighboring end-of-line comments.

```python
HOST = "localhost"  # main API host
PORT = 443  # TLS
TIMEOUT_SECONDS = 30  # request timeout
```

becomes:

```python
HOST = "localhost"    # main API host
PORT = 443            # TLS
TIMEOUT_SECONDS = 30  # request timeout
```

Statements with multiple trailing comments are left on Ruff's normal spacing path.

## Collection Layout

Collection layout options live under `[tool.tuff.format.collections]`.

```toml
[tool.tuff.format.collections]
lists = { layout = "ruff-default" }
dicts = { layout = "ruff-default" }
tuples = { layout = "ruff-default" }
sets = { layout = "ruff-default" }
```

The same layout modes are available for `lists`, `dicts`, `tuples`, and `sets`.

### Ruff Default

`layout = "ruff-default"` keeps Ruff's normal collection formatting.

```toml
[tool.tuff.format.collections]
lists = { layout = "ruff-default" }
```

### Force Expanded

`layout = "force-expanded"` prints non-empty collections across multiple lines.

```python
numbers = [1, 2, 3]
settings = {"debug": True, "retries": 3}
point = (1, 2, 3)
flags = {"read", "write"}
```

with:

```toml
[tool.tuff.format.collections]
lists = { layout = "force-expanded" }
dicts = { layout = "force-expanded" }
tuples = { layout = "force-expanded" }
sets = { layout = "force-expanded" }
```

becomes:

```python
numbers = [
    1,
    2,
    3,
]
settings = {
    "debug": True,
    "retries": 3,
}
point = (
    1,
    2,
    3,
)
flags = {
    "read",
    "write",
}
```

### Expand By Size

`layout = "expand-if-more-than"` expands only when the collection has more items than `threshold`.

```toml
[tool.tuff.format.collections]
lists = { layout = "expand-if-more-than", threshold = 3 }
```

With this setting, lists with one, two, or three items use Ruff's default formatting. Lists with four or more items expand.

### Fill

`layout = "fill"` keeps an already multiline collection expanded but packs as many items as fit on each line.

```python
values = [
    "read",
    "write",
    "delete",
    "admin",
    "billing",
    "support",
]
```

with:

```toml
[tool.tuff.format.collections]
lists = { layout = "fill" }
```

becomes:

```python
values = [
    "read", "write", "delete", "admin", "billing", "support",
]
```

Single-line collections keep Ruff's normal formatting.

### Accepted Values

| Value                   | Behavior                                                         |
| ----------------------- | ---------------------------------------------------------------- |
| `"ruff-default"`        | Use Ruff's normal collection formatting.                         |
| `"force-expanded"`      | Expand non-empty collections onto multiple lines.                |
| `"expand-if-more-than"` | Expand collections whose item count is greater than `threshold`. |
| `"fill"`                | Pack already multiline collection items onto filled rows.        |
| `"prefer-compact"`      | Accepted, currently equivalent to `"ruff-default"`.              |
| `"preserve-input"`      | Accepted, currently equivalent to `"ruff-default"`.              |

Collections with comments or magic trailing commas keep Ruff's normal behavior.

## Docstring Sections

Docstring section options live under `[tool.tuff.format.docstrings]`.

```toml
[tool.tuff.format.docstrings]
args-sections = "enabled"
```

`args-sections = "enabled"` aligns descriptions in Google-style argument sections.

```python
def process(name, config_path):
    """Process an item.

    Args:
        name: Item name.
        config_path: Configuration file.
    """
```

becomes:

```python
def process(name, config_path):
    """Process an item.

    Args:
        name:        Item name.
        config_path: Configuration file.
    """
```

Tuff recognizes `Args:`, `Arguments:`, `Parameters:`, `Keyword Args:`, `Keyword Arguments:`, `Other Args:`, and `Other Arguments:`. It only aligns `name: description` entries with a plain description after the colon. Continuation lines stay untouched, and padding is based on display width so non-ASCII names line up correctly. Blank lines and non-entry paragraphs end the group. Markdown fences and reStructuredText literal blocks are skipped, so examples are not rewritten as argument sections.

## One-Line Suites

One-line suites move a simple block body onto the same physical line as its clause header when the whole line fits.

```toml
[tool.tuff.format.one-line-suites]
mode = "enabled"
clauses = ["if"]
statements = ["return", "call"]
```

With that config:

```python
def process(callback):
    if callback is None:
        return
    if callback is not None:
        callback()
```

becomes:

```python
def process(callback):
    if callback is None: return
    if callback is not None: callback()
```

The colon stays in the output because Python requires it. Tuff only collapses a suite when the body has exactly one allowed statement, has no comments attached to the body statement, has no trailing comment after the clause colon, and fits within `line-length`.

Accepted `clauses` values:

| Value          | Clause matched       |
| -------------- | -------------------- |
| `"if"`         | `if condition:`      |
| `"elif"`       | `elif condition:`    |
| `"else"`       | `else:`              |
| `"for"`        | `for item in items:` |
| `"while"`      | `while condition:`   |
| `"with"`       | `with manager:`      |
| `"try"`        | `try:`               |
| `"except"`     | `except ...:`        |
| `"finally"`    | `finally:`           |
| `"match-case"` | `case pattern:`      |

Accepted `statements` values:

| Value                    | Body matched                        |
| ------------------------ | ----------------------------------- |
| `"return"`               | `return` and `return value`         |
| `"call"`                 | A call expression, such as `done()` |
| `"raise"`                | `raise` statements                  |
| `"assert"`               | `assert` statements                 |
| `"pass"`                 | `pass`                              |
| `"break"`                | `break`                             |
| `"continue"`             | `continue`                          |
| `"ellipsis"`             | `...`                               |
| `"expression"`           | Any expression statement            |
| `"assignment"`           | Plain assignment statements         |
| `"annotated-assignment"` | Annotated assignment statements     |

## Docstring Code Blocks

Tuff-specific alignment, collection layout, and one-line suite options do not apply inside Python snippets formatted from docstrings. Docstring code blocks use the same behavior as Ruff.
