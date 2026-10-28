# Formatter patches

Tuff keeps custom policy under `crates/tuff_python_formatter/src/custom`. Copied formatter rules call these helpers:

- `expression/expr_list.rs`, `expr_dict.rs`, `expr_set.rs`, and `expr_tuple.rs`: collection layout, filled rows, trailing-comma handling, and nested-row formatting through `custom::collections` and `custom::hooks`.
- `other/arguments.rs` and `other/parameters.rs`: optional argument and parameter packing through `custom::packing`.
- `statement/stmt_import_from.rs`: optional import-alias packing through `custom::packing`.
- `statement/clause.rs` and `statement/suite.rs`: optional one-line definition suites through `custom::hooks`, measured from rendered output, including normalized single-line docstrings.
- `statement/stmt_delete.rs`: compact multi-target deletion uses the same tuple layout before and after parentheses are introduced.
- `context.rs`: a borrowed formatting context for measuring one-line candidates without copying source text.

All paths above are relative to `crates/tuff_python_formatter/src`. These patches do not change the upstream formatter reference in `crates/ruff_python_formatter`.

## Native lint and spacing integration

- `tuff_lint` and `tuff_lint_macros` own Rust rules, shared spacing decisions, semantic access, named ignores, and generated registration metadata.
- `ruff_linter` registers the Tuff namespace, runs native checks after deferred semantic analysis, applies replacement metadata after per-file ignores, and integrates statement and comment suppression with standard `noqa`. Shared gap ownership replaces competing spacing reports independently of fix availability. Its rule bitset includes the additional native rules.
- `ruff_workspace` resolves Tuff files and pyproject tables, merges inherited native selections in layer order, and shares formatter option conversion with native layout measurement. Native disables do not carry past an explicit child re-enable.
- Tuff formatter `custom/blank_lines.rs` measures source-mapped rendered structure and density, including expanded inline suites and semicolon pairs. Clause headers record both boundary markers. Suite, clause, match-case, and comment hooks apply original-anchor decisions through IR. Module, AST, database, and range entrypoints use that plan.
- CLI formatting consumes resolved custom settings, including explicit configuration and isolation, without separate filesystem discovery.
