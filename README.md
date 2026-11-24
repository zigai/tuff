# Tuff

Tuff is a fork of [Ruff](https://github.com/astral-sh/ruff) that provides extra Python formatting options.

## Install

With Rust and Cargo installed:

```sh
cargo install --git https://github.com/zigai/tuff.git --locked --bin tuff ruff
```

## Use

```sh
tuff format .
tuff check .
```

## Formatting options

Currently added formatting options:

| Option                     | Description                                        |
| -------------------------- | -------------------------------------------------- |
| `class-fields`             | Align class attribute annotations and defaults.    |
| `function-params`          | Align function parameter annotations and defaults. |
| `assignments`              | Align simple assignment operators.                 |
| `dict-values`              | Align values in multiline dictionaries.            |
| `dict-alignment`           | Align dictionary values or colons.                 |
| `call-keyword-args`        | Align keyword arguments in multiline calls.        |
| `import-aliases`           | Align `as` aliases in imports.                     |
| `collection-rows`          | Align table-like nested list and tuple rows.       |
| `repeated-call-args`       | Align positional args across repeated calls.       |
| `with-items`               | Align `as` targets in multi-item `with` blocks.    |
| `trailing-comments`        | Align neighboring end-of-line comments.            |
| `collections`              | Control list, dict, tuple, and set expansion.      |
| `docstrings.args-sections` | Align Google-style argument descriptions.          |
| `one-line-suites`          | Collapse simple suites onto one line.              |

See [Custom formatting options](docs/custom-formatting-options.md) for examples.

## Native lint rules

`tuff check` runs Ruff's lint rules plus opt-in native `TUF` rules. Enable them by name:

```toml
[tool.tuff.lint]
enable = ["require-subprocess-run-timeout", "respect-module-all"]
```

See [Native lint rules](docs/native-linting.md) for selectors, rule options, policy rules, and `tuff: ignore` comments.

## Python rules

`tuff check` also runs lint rules written in Python from directories listed in `plugin-dirs`. See [Python rules](docs/python-rules.md).

## Exception flow

`tuff throws inspect` shows the exceptions a function can raise and where they come from. `tuff throws check` reports exceptions that escape boundaries or contracts. See [Exception flow](docs/throws.md).

## Editors

`tuff server` runs the language server with the same configuration as `tuff check`, including Python rules.
