# Native lint rules

Tuff provides 52 opt-in Rust rules. Enable canonical names under `[tool.tuff.lint]`, or select their `TUF` codes with the standard lint selectors. Broad Ruff selectors such as `ALL` and categories do not enable native rules. Defaults remain Ruff-compatible.

```toml
[tool.tuff.lint]
enable = ["forbidden-call", "no-bare-object-annotations"]
disable = []

[tool.tuff.lint.rules.forbidden-call]
forbidden_calls = ["builtins.eval|Use a parser for the expected input format."]
```

Selectors accept individual names, `all`, `builtin` (the non-legacy rules), and `legacy`. `all` and `builtin` include both header-spacing policies; explicitly disable either `blank-line-before-unrelated-block` or `block-header-cuddle-strict`. Enabling both is an error. Unknown names and rule options are errors. Equal-specificity line-limit overrides apply in source order; exact per-file limits apply after globs.

An active native rule replaces the overlapping Ruff checks declared in its metadata. Disabled, version-inactive, per-file-ignored, and empty-option inactive native rules leave those checks available. Replacement does not imply identical coverage. Ordinary Ruff selectors and per-file ignores still apply.

Spacing policies own competing Ruff blank-line reports at the same gap, even when fixes are disabled. Unrelated blank-line reports remain available.

## Blank lines

Selecting a spacing rule for linting also enables its formatting policy. Formatter-only selection does not enable lint diagnostics:

```toml
[tool.tuff.format.blank-lines]
rules = ["blank-line-before-branch", "no-suite-leading-trailing-blank-lines"]

[tool.tuff.lint.rules.blank-line-before-branch]
max_suite_non_empty_lines = 3
allow_related_return_tails = false
```

Both consumers read the same per-rule options and effective selection. A child configuration can re-enable a parent's native disable. The eight spacing rules are:

- `blank-line-after-control-block`
- `blank-line-after-terminal-control-block`
- `blank-line-before-assignment`
- `blank-line-before-branch`
- `blank-line-before-unrelated-block`
- `block-header-cuddle-strict`
- `match-case-separation`
- `no-suite-leading-trailing-blank-lines`

Structure and line thresholds follow rendered layout, including expanded semicolons, compact suites, width, and alignment. The formatter applies shared decisions through its normal IR hooks. `fmt: off` and `fmt: skip` retain their normal formatting scope. Reordering attributes, methods, and `__all__` stays in lint rules.

## Ignore comments

Canonical names work for native and upstream checks:

```python
import package  # tuff: ignore[unused-import]
eval(text)  # tuff: ignore[forbidden-call]
```

A leading ignore belongs to the next statement or immediately following comment. An ignore also covers its own comment, including at EOF. A compound header ignore covers that header; an inline suite shares its physical line. It does not suppress a later multiline body. Unknown rule names produce a diagnostic.

Standard `noqa` still works. Opt-in `use-tuff-ignore-comment` diagnoses standard suppressions for all checks. A `noqa` cannot suppress that migration rule itself; `tuff: ignore[use-tuff-ignore-comment]` can.

## Adding a Rust rule

Implement `LintRule` and register metadata in a builtin module:

```rust
use anyhow::Result;
use ruff_python_ast::{AnyNodeRef, NodeKind};
use ruff_text_size::Ranged;
use crate::{Context, Finding, LintRule, RuleDefinition, RuleOptions, reject_options};

#[derive(Debug)]
struct TemporaryName;

impl LintRule for TemporaryName {
    fn check(&self, node: AnyNodeRef<'_>, _: &Context<'_, '_, '_>, findings: &mut Vec<Finding>) {
        if let AnyNodeRef::StmtFunctionDef(function) = node
            && function.name.starts_with("temporary_")
        {
            findings.push(Finding::new("no-temporary-function-name", function.name.range(), "Choose a descriptive function name."));
        }
    }
}

fn create(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(options)?;
    Ok(Box::new(TemporaryName))
}

pub(super) static RULES: &[RuleDefinition] = &[crate::native_rule! {
    name: "no-temporary-function-name", code: "TUF301",
    explanation: "Use descriptive function names.",
    node_kinds: &[NodeKind::StmtFunctionDef], checks_comments: false,
    fixable: false, create: create,
}];
```

Include the module's definitions in `builtin::definitions`. Registration, codes, violation adapters, dispatch, and replacement tables are generated from the metadata. `.replacing(...)`, `.since(...)`, and `.before(...)` declare overlap and version activity. `is_enabled()` lets an empty configuration remain inactive.

`Context` supplies the parsed module, source, path, Python version, ancestors, and completed Ruff semantic facts. Rules return `Finding` values with optional Ruff fixes. Registry construction is lazy, and cache keys include rule source contents, options, and project root. No Python runtime or matching-language interpreter is involved.

## Reference differences

The port follows Python semantics where the LibCST implementation differs: completed references respect assignment evaluation order, deferred bodies, comprehension locals, and type parameters. Quoted compound annotations use those lexical scopes. Relative imports retain their package meaning. Literal identity does not assume allocation or interning. Fixes that can invoke arbitrary equality, reorder assertion evaluation, or remove evaluated annotations require `--unsafe-fixes`; unavailable typing names are withheld. Callable recommendations remain advisory.

Diagnostics use Ruff AST spans and source order. Decorated definitions and generator conditions can therefore have different spans, and trailing blank-line cleanup groups a contiguous footer into one report. Unknown external ignore names are rejected. Rendered structure and safe-fix eligibility can also differ from the reference.
