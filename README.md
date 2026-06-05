# Tuff

Tuff is a fork of [Ruff](https://github.com/astral-sh/ruff) that provides extra Python formatting options.

Currently added formatting options:

| Option                     | Description                                        |
| -------------------------- | -------------------------------------------------- |
| `class-fields`             | Align class attribute annotations and defaults.    |
| `function-params`          | Align function parameter annotations and defaults. |
| `assignments`              | Align simple assignment operators.                 |
| `dict-values`              | Align values in multiline dictionaries.            |
| `call-keyword-args`        | Align keyword arguments in multiline calls.        |
| `import-aliases`           | Align `as` aliases in imports.                     |
| `collections`              | Control list, dict, tuple, and set expansion.      |
| `docstrings.args-sections` | Align Google-style argument descriptions.          |
| `one-line-suites`          | Collapse simple suites onto one line.              |

See [Custom formatting options](docs/custom-formatting-options.md) for examples.
