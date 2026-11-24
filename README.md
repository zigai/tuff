# Tuff

[![tests](https://img.shields.io/github/actions/workflow/status/zigai/tuff/tuff.yml?branch=main&label=tests&labelColor=30363d)](https://github.com/zigai/tuff/actions/workflows/tuff.yml)
[![release](https://img.shields.io/github/v/release/zigai/tuff?label=release&color=blue&logo=github&labelColor=30363d)](https://github.com/zigai/tuff/releases)
[![Rust](https://img.shields.io/badge/Rust-1.98+-blue?logo=rust&logoColor=white&labelColor=30363d)](Cargo.toml)
[![license](https://img.shields.io/github/license/zigai/tuff?label=license&labelColor=30363d)](LICENSE)

Tuff extends [Ruff](https://github.com/astral-sh/ruff) with configurable Python formatting, extra Rust and Python lint rules, and exception-flow analysis.

- **Custom formatting:** Align neighboring code blocks and control collection or one-line suite layout.
- **Native lint rules:** Enable 59 opt-in Rust rules covering import boundaries, style, and spacing.
- **Python rules:** Run repository-local Python lint rules with Ruff name resolution and cached workers.
- **Exception flow:** Trace which exceptions a function can raise and catch escapes at boundaries.
- **Language server:** Surface Tuff diagnostics, fixes, and hover details directly in editors.

## Installation

```sh
cargo install --git https://github.com/zigai/tuff.git --locked --bin tuff ruff
```

Requires Rust 1.98 or newer.

## Quick start

Run `tuff` on an existing Ruff project for identical defaults:

```sh
tuff format .
tuff check .
```

Opt in to Tuff features under `[tool.tuff]` in `pyproject.toml`:

```toml
[tool.tuff.format.alignment]
class-fields = "enabled"

[tool.tuff.lint]
enable = ["require-subprocess-run-timeout", "respect-module-all"]
```

Inspect which exceptions a function can raise and where each one comes from:

```sh
tuff throws inspect app/store.py::load
```

## Commands

```text
$ tuff --help
Tuff: an extremely fast Python linter and code formatter.

Usage: tuff [OPTIONS] <COMMAND>

Commands:
  check    Run Tuff on the given files or directories
  rule     Explain a rule (or all rules)
  config   List or describe the available configuration options
  linter   List all supported upstream linters
  clean    Clear any caches in the current directory and any subdirectories
  format   Run the Tuff formatter on the given files or directories
  server   Run the language server
  analyze  Run analysis over Python source code
  throws   Analyze which exceptions functions can raise
  version  Display Tuff's version
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Log levels:
  -v, --verbose
          Enable verbose logging

  -q, --quiet
          Print diagnostics, but nothing else

  -s, --silent
          Disable all logging (but still exit with status code "1" upon
          detecting diagnostics)

Global options:
      --config <CONFIG_OPTION>
          Either a path to a TOML configuration file (`pyproject.toml` or
          `ruff.toml`), or a TOML `<KEY> = <VALUE>` pair (such as you might find
          in a `ruff.toml` configuration file) overriding a specific
          configuration option (e.g., `--config "line-length = 100"` or
          `--config "format.quote-style = 'single'"`). Overrides of individual
          settings using this option always take precedence over all
          configuration files, including configuration files that were also
          specified using `--config`

      --isolated
          Ignore all configuration files

      --color <WHEN>
          Control when colored output is used

          Possible values:
          - auto:   Use color when writing to a terminal, unless overridden by
            the environment
          - always: Always display colors
          - never:  Never display colors

For help with a specific command, see: `tuff help <command>`.
```

## Documentation

- [Custom formatting options](docs/custom-formatting-options.md)
- [Native lint rules](docs/native-linting.md)
- [Python rules](docs/python-rules.md)
- [Exception flow](docs/throws.md)
- [Architecture](docs/architecture.md)
- [Upstream Ruff README](README.ruff.md)

## License

[MIT](LICENSE)
