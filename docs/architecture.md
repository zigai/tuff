# Tuff Architecture

Tuff is a Ruff fork with additional, opt-in lint and formatter behavior.

- `crates/ruff` builds the `tuff` binary.
- `crates/tuff_python_formatter` contains Tuff-specific formatter behavior.
- `crates/tuff_lint` owns native Rust rules, shared facts, named suppression, and spacing decisions. `crates/tuff_lint_macros` generates Ruff registrations from rule metadata.
- `crates/ruff_python_formatter` remains the upstream-compatible formatter reference.
- Ruff parser, AST, trivia, source, diagnostics, cache storage, and generic formatter crates stay aligned with upstream. Workspace resolution also loads Tuff configuration.
- Tuff defaults stay Ruff-compatible.
- Custom behavior is enabled through `TuffCustomOptions` or `[tool.tuff]`.

## Fork Boundary

Tuff-owned code lives in:

- `crates/tuff_python_formatter`
- `crates/tuff_lint` and `crates/tuff_lint_macros`
- `crates/ruff_linter/src/tuff.rs` and `rules/tuff.rs`
- `crates/ruff_workspace/src/tuff.rs`
- `crates/ruff/src/tuff`
- `crates/ruff/tests/tuff_cli.rs`
- Tuff docs and config references

`crates/ruff/src/commands` contains CLI orchestration. Workspace settings resolve custom options alongside standard options. Formatter entrypoints live under `crates/ruff/src/tuff`.

The cache layer is product-neutral. Tuff formatting options participate through generic format cache keys.

## Formatter Flow

1. Resolve `PyFormatOptions`, including grouped `TuffCustomOptions`.
2. Parse source with Ruff's Python parser.
3. Build Ruff's comment model.
4. Build `TuffLayoutPlan` from AST, tokens, comments, source, and custom options.
5. Store the layout plan in `PyFormatContext`.
6. Format nodes through copied Ruff formatter rules.
7. Let copied node files call `custom::hooks` at narrow insertion points.
8. Keep policy and derived decisions under `crates/tuff_python_formatter/src/custom`.

Enabled blank-line policies measure the rendered layout before the shared spacing planner runs. Formatter hooks apply its constraints through formatting IR. Optional lint diagnostics use the same planner and rendered metrics. Attribute, method, and export reordering remains lint-only. See [native linting](native-linting.md) for configuration and rule authoring.

## Upstream Vendoring

- Formatter options describe user intent.
- AST-dependent decisions live in `TuffLayoutPlan`.
- Tuff policy lives under `crates/tuff_python_formatter/src/custom`.
- Copied upstream formatter files may call `custom::hooks`.
- Copied upstream formatter files should not contain Tuff policy.
- Hook changes in copied formatter files are recorded in `UPSTREAM_PATCHES.md`.
- CLI tests cover config discovery, errors, cache behavior, and user-facing integration.
