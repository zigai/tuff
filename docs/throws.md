# Exception flow

`tuff throws` finds the exceptions each function can raise, follows them through calls, and reports where they escape. Calls and exception classes are resolved with ty.

## Inspect

```sh
tuff throws inspect app/store.py::load
```

```text
app/store.py::load (app/store.py:4:5) raises 2 exceptions

  Conflict (app.errors.Conflict)
      load  app/store.py:13:9  raises `Conflict`

  NotFound (app.errors.NotFound)
      load  app/store.py:11:9  raises `NotFound`
```

The target is one of:

- `path/to/file.py::Class.method`: one function.
- `path/to/file.py`: every function in the file.
- `package.module.Class.method`: one function by qualified name.

Each exception is listed with the chain of calls it travels through. A note marks results that may be incomplete because of unresolved calls or the depth limit.

| Option                    | Description                                                                   |
| ------------------------- | ----------------------------------------------------------------------------- |
| `--track <EXCEPTION>`     | Show only exceptions deriving from this class. Repeatable. Replaces `track`. |
| `--depth <N>`             | Maximum number of calls an exception may propagate through.                  |
| `--format text\|json`     | Output format. Default: `text`.                                               |

## Check

```sh
tuff throws check
tuff throws check app/api --track AppError --boundary "app.api.*"
tuff throws check --diff
```

```text
app/api.py:7:16: THROWS001 [unhandled-boundary-exception] `Conflict` escapes boundary `get_item` unhandled
      get_item  app/api.py:7:16    calls `load()`
    → load      app/store.py:13:9  raises `Conflict`
```

Paths limit which files are reported on; the whole project is still analyzed. The command exits with status 1 when it reports a problem.

| Option                 | Description                                                                         |
| ---------------------- | ----------------------------------------------------------------------------------- |
| `[PATHS]...`           | Files or directories to report on. Default: the whole project.                      |
| `--track <EXCEPTION>`  | Exception base class to track. Repeatable. Replaces `track`.                        |
| `--boundary <GLOB>`    | File or symbol glob of boundary functions. Repeatable. Added to `boundaries`.      |
| `--diff`               | Report only on files changed relative to `HEAD`, including untracked files.         |
| `--format text\|json`  | Output format. Default: `text`.                                                     |

## Diagnostics

| Code        | Name                           | Reported when                                                           |
| ----------- | ------------------------------ | ----------------------------------------------------------------------- |
| `THROWS001` | `unhandled-boundary-exception` | A tracked exception escapes a boundary function.                        |
| `THROWS002` | `redundant-except`             | An `except` clause handles a project exception the `try` never raises.  |
| `THROWS003` | `undeclared-exception`         | A function with a contract raises a tracked exception it does not declare. |
| `THROWS004` | `override-widens-throws`       | An override can raise a tracked exception the overridden method's contract does not declare. |

Without `track`, `check` tracks the exception classes defined in the project.

## Configuration

Settings live in `[tool.tuff.throws]` in `pyproject.toml`, or `[throws]` in `tuff.toml` or `.tuff.toml`. Tuff uses the nearest configuration file at or above the current directory, and its directory is the project root.

```toml
[tool.tuff.throws]
track = ["AppError"]
boundaries = ["app.api.*", "app/cli/**"]
sources = ["raise", "decorator", "docstring"]
max-depth = 10
include = ["app/**"]
exclude = ["app/migrations/**"]
transparent-decorators = ["retry"]
suppressing-decorators = ["handle_errors"]
```

| Key                      | Default                                 | Description                                                                                       |
| ------------------------ | --------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `track`                  | `[]`                                    | Exception base classes to report on. A name matches its qualified name or any dotted suffix of it. |
| `boundaries`             | `[]`                                    | Functions that must not let tracked exceptions escape.                                            |
| `sources`                | `["raise", "decorator", "docstring"]`   | Where exceptions come from: `raise` statements, `@throws` decorators, and docstrings.            |
| `max-depth`              | unlimited                               | Maximum number of calls an exception may propagate through.                                       |
| `include`                | `[]`                                    | File globs to analyze. Empty means every Python file in the project.                              |
| `exclude`                | `[]`                                    | File globs to skip.                                                                               |
| `transparent-decorators` | `[]`                                    | Decorators that keep the wrapped function's exceptions.                                           |
| `suppressing-decorators` | `[]`                                    | Decorators that handle every exception the wrapped function raises.                               |

A `boundaries` pattern that contains `/` or ends in `.py` matches project-relative file paths. Any other pattern matches qualified names such as `app.api.handlers.create_order`. Exceptions do not propagate from a boundary into its callers.

Decorators match by full name or by last dotted segment. Common decorators such as `functools.wraps`, `property`, `classmethod`, `staticmethod`, `contextmanager`, and `lru_cache` are transparent already. Calls to a function with any other decorator are marked incomplete.

## Contracts

A function's contract is the set of exceptions it declares. It can come from a `Raises` docstring section in Google, NumPy, or Sphinx style:

```python
def load(key: str) -> str:
    """Load a value.

    Raises:
        NotFound: If the key is missing.
    """
```

or from a decorator named `throws`:

```python
@throws(NotFound, "if the key is missing")
@throws((NotFound, "missing"), (Conflict, "duplicate"))
def load(key: str) -> str: ...
```

Tuff does not provide the decorator; define one in your project. Declared exceptions also count as raised by the function, so callers see them. A function's contract is the union of both sources, limited to the configured `sources`.

## Suppression

```python
return load(key)  # throws: ignore[THROWS001]
raise Conflict(key)  # tuff: ignore[undeclared-exception, Conflict]
```

A suppression lists diagnostic codes, diagnostic names, or exception names. `# throws: ignore` without a list suppresses every diagnostic on that line. Place it on the line where the exception escapes, on the `def` line, or for `THROWS002` on the `except` clause. It may follow another comment, as in `# noqa: E501 # throws: ignore[KeyError]`.

## Editors

When a workspace configures `[tool.tuff.throws]` or `[throws]`, `tuff server` reports `THROWS001`–`THROWS004` diagnostics with call chains in related information using open unsaved buffers. Hovering over a function definition or call site shows declared and inferred exceptions. Workspaces without a throws section incur no exception-flow analysis overhead.
