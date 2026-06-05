# Initial Inspection

Date: 2026-05-29

## Repository

- Root: `/home/zigai/Projects/tuff`
- Upstream commit: `47d742f520dc6352209710dcad9b40d55c0f3b44`
- Remote: `upstream https://github.com/astral-sh/ruff.git`
- Cargo metadata: generated at `/tmp/tuff-cargo-metadata.json`
- Workspace shape: `Cargo.toml` uses `members = ["crates/*"]`, so a new crate under `crates/` is included automatically.

## Formatter Entry Points

- Full-source formatting: `crates/ruff_python_formatter/src/lib.rs`
    - `format_module_source(source: &str, options: PyFormatOptions) -> Result<Printed, FormatModuleError>`
    - `format_module_ast(parsed: &Parsed<Mod>, comment_ranges: &CommentRanges, source: &str, options: PyFormatOptions) -> FormatResult<Formatted<PyFormatContext>>`
    - private shared helper: `format_node`
- Database-backed formatting: `crates/ruff_python_formatter/src/lib.rs`
    - `formatted_file(db: &dyn Db, file: File) -> Result<Option<String>, FormatModuleError>`
- Range formatting: `crates/ruff_python_formatter/src/range.rs`
    - `format_range(source: &str, range: TextRange, options: PyFormatOptions) -> Result<PrintedRange, FormatModuleError>`
    - Range formatting constructs its own `PyFormatContext` after parsing, except for full-file ranges, which delegate to `format_module_source`.

## Context Type

- Type: `PyFormatContext<'a>` in `crates/ruff_python_formatter/src/context.rs`
- Current fields:
    - `options: PyFormatOptions`
    - `contents: &'a str`
    - `comments: Comments<'a>`
    - `tokens: &'a Tokens`
    - `node_level: NodeLevel`
    - `indent_level: IndentLevel`
    - `docstring: Option<Quote>`
    - `interpolated_string_state: InterpolatedStringState`
- Constructor:
    - `PyFormatContext::new(options, contents, comments, tokens)`

## Options Type

- Type: `PyFormatOptions` in `crates/ruff_python_formatter/src/options.rs`
- Current fields:
    - `source_type`
    - `target_version`
    - `indent_style`
    - `line_width`
    - `indent_width`
    - `line_ending`
    - `quote_style`
    - `magic_trailing_comma`
    - `source_map_generation`
    - `docstring_code`
    - `docstring_code_line_width`
    - `preview`
    - `nested_string_quote_style`
- Builder methods exist for target version, indentation, quote style, magic trailing comma, preview, line ending, line width, docstring code, nested string quote style, and source map generation.

## Fixture System

- Formatter fixture harness: `crates/ruff_python_formatter/tests/fixtures.rs`
- Fixtures:
    - `crates/ruff_python_formatter/resources/test/fixtures/black`
    - `crates/ruff_python_formatter/resources/test/fixtures/ruff`
- Snapshot output: `crates/ruff_python_formatter/tests/snapshots`
- Test command for the upstream formatter crate: `cargo test -p ruff_python_formatter`

## CLI Config Path

- Ruff formatter config struct: `FormatOptions` in `crates/ruff_workspace/src/options.rs`
- Resolved formatter settings: `FormatterSettings` in `crates/ruff_workspace/src/settings.rs`
- Conversion to formatter options:
    - `FormatterSettings::to_format_options(source_type, source, path) -> PyFormatOptions`
- Ruff CLI formatting uses `crates/ruff/src/commands/format.rs` and calls `format_module_source` / `format_range`.

## Initial Decision

Create `crates/tuff_python_formatter` as a copy of `crates/ruff_python_formatter`, keep the upstream formatter crate untouched, and use the copied crate as the divergence point for tuff-specific options, layout planning, and hooks.
