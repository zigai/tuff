# Native lint rules

Tuff provides 59 opt-in Rust rules. Enable canonical names under `[tool.tuff.lint]`, or select their `TUF` codes with the standard lint selectors. Broad Ruff selectors such as `ALL` and categories do not enable native rules. Defaults remain Ruff-compatible.

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

For repository-local rules written in Python, see [Python rules](python-rules.md). For exception-flow checks, see [Exception flow](throws.md).

## Policy rules

Five rules ban configured symbols. Each takes a list of entries:

| Rule                             | Option                 | Shorthand                      |
| -------------------------------- | ---------------------- | ------------------------------ |
| `forbidden-call` (`TUF001`)      | `forbidden_calls`      | `symbol\|message\|replacement` |
| `forbidden-import` (`TUF002`)    | `forbidden_imports`    | `module\|message`              |
| `forbidden-name` (`TUF003`)      | `forbidden_names`      | `kind:pattern\|message`        |
| `forbidden-attribute` (`TUF029`) | `forbidden_attributes` | `receiver.attribute\|message`  |
| `forbidden-raise` (`TUF030`)     | `forbidden_raises`     | `exception\|message`           |

An entry is either a shorthand string or a table. Tables accept these keys in addition to the selector keys below:

- `message`: replaces the default diagnostic message.
- `replacement`: a dotted name added to the message as advice: `Use instead: <replacement>.`
- `include`, `exclude`: path globs relative to the configuration root. `*` stays within one path segment, and a directory pattern such as `tests` covers every file below it.

A later entry with the same selector replaces an earlier one.

```toml
[tool.tuff.lint]
enable = ["forbidden-call", "forbidden-import", "forbidden-attribute", "forbidden-raise"]

[tool.tuff.lint.rules.forbidden-call]
forbidden_calls = [
    "builtins.eval|Parse the input instead.",
    { symbol = "time.sleep", in_async = true, replacement = "asyncio.sleep" },
    { symbol = "subprocess.run", require_keywords = ["timeout"], forbid_keywords = ["shell"] },
]

[tool.tuff.lint.rules.forbidden-import]
forbidden_imports = [
    { module = "app.db", exclude = ["app/repositories"], message = "Use a repository." },
    { module = "app.services", names = ["*_client"], message = "Inject the client." },
]

[tool.tuff.lint.rules.forbidden-attribute]
forbidden_attributes = [
    { attribute = "dict", calls_only = true, replacement = "model_dump" },
    { qualified_prefix = "os.environ", exclude = ["app/settings.py"] },
    { receiver = "request", attribute = "state", ignore_writes = true },
]

[tool.tuff.lint.rules.forbidden-raise]
forbidden_raises = [
    { exception = "HTTPException", exclude = ["app/api"], message = "Raise a domain error." },
]
```

Selector keys per rule:

- `forbidden_calls`: `symbol` is the qualified callable, matched through import aliases.
    - `require_keywords` reports calls that omit a listed keyword argument. Calls that pass `**kwargs` satisfy it.
    - `forbid_keywords` reports calls that pass a listed keyword argument.
    - With either list, the entry reports only these keyword violations.
- `forbidden_imports`: `module` bans the module and everything below it. `names` narrows the entry to `from module import name` of matching names, including star imports, from that exact module.
- `forbidden_names`: `kind` is `any`, `variable`, `parameter`, `function`, `class`, `attribute`, `import`, or `alias`. `pattern` is a shell-style pattern.
- `forbidden_attributes`: an entry needs `attribute` or `qualified_prefix`.
    - `attribute` and `receiver` are shell-style patterns for the attribute name and the dotted receiver as written.
    - `qualified_prefix` matches accesses whose resolved qualified name is at or below the prefix. Only the innermost access is reported.
    - `calls_only` reports only method calls.
    - `ignore_writes` skips assignments and `del` through the attribute.
- `forbidden_raises`: `exception` is a shell-style pattern matched against the resolved qualified name and the name as written.

`forbidden_calls` and `forbidden_raises` entries also accept two context filters:

- `in_async`: `true` matches only inside `async def` bodies; `false` matches only outside them.
- `decorators`: shell-style patterns. The entry matches only inside a function or class with a matching decorator, by written or resolved name.

### Architecture and size rules

| Rule                                      | Description                                                | Options                                                                                      |
| ----------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `import-contracts` (`TUF004`)             | Enforce direct-import forbidden, layer, and package rules. | `source_roots`, `ignore_type_checking`, `contracts` (`forbidden`, `layers`, `independence`)  |
| `line-count-limit` (`TUF005`)             | Limit file, function, and method line counts.              | `max_file_lines`, `max_function_lines`, `max_method_lines`, `glob_limits`, `per_file_limits` |
| `no-relative-imports` (`TUF006`)          | Require absolute imports. Replaces `TID252`.               | none                                                                                         |
| `no-underscore-import-aliases` (`TUF007`) | Disallow underscore-prefixed import aliases.               | none                                                                                         |
| `no-unsafe-tempfile-factories` (`TUF008`) | Prefer managed `tempfile` context managers.                | `excluded_path_parts`                                                                        |

## Style, export, typing, and modernization rules

| Rule                                                  | Description                                                                 | Options                                               |
| ----------------------------------------------------- | --------------------------------------------------------------------------- | ----------------------------------------------------- |
| `no-annotated-self` (`TUF009`)                        | Omit redundant instance `self` annotations. Fixable.                        | none                                                  |
| `no-exception-message-variables` (`TUF010`)           | Inline throwaway exception messages. Fixable. Replaces `EM101`–`EM103`.     | none                                                  |
| `no-str-exception-translation` (`TUF011`)             | Use fixed messages when translating caught exceptions.                      | none                                                  |
| `no-underscore-class` (`TUF012`)                      | Disallow class names starting with an underscore.                           | none                                                  |
| `public-method-order` (`TUF013`)                      | Place public methods before private helpers.                                | `class_name_patterns`, `excluded_class_name_patterns` |
| `sorted-attributes` (`TUF014`)                        | Sort opted-in class assignment groups. Fixable.                             | none                                                  |
| `module-all-at-bottom` (`TUF015`)                     | Place module `__all__` after runtime definitions. Fixable.                  | none                                                  |
| `no-underscore-all-exports` (`TUF016`)                | Disallow underscore-prefixed names in `__all__`.                            | `allowed_exports`, `allow_dunder_exports`             |
| `no-bare-object-annotations` (`TUF017`)               | Prefer narrower types over bare `object` annotations.                       | `excluded_path_parts`                                 |
| `use-callable-ellipsis` (`TUF018`)                    | Use `Callable[..., T]` for arbitrary parameters. Fixable.                   | none                                                  |
| `use-types-from-typing` (`TUF019`)                    | Use `typing` aliases before Python 3.9. Fixable. Replaces `UP006`, `UP035`. | none                                                  |
| `explicit-frozen-dataclass` (`TUF020`)                | Specify `frozen=True` or `frozen=False` on dataclasses.                     | none                                                  |
| `no-named-tuple` (`TUF021`)                           | Prefer dataclasses over named tuples. Replaces `UP014`, `PYI024`.           | none                                                  |
| `no-static-if-condition` (`TUF022`)                   | Remove constant `if` conditions and debug clauses.                          | none                                                  |
| `use-tuff-ignore-comment` (`TUF023`)                  | Prefer `# tuff: ignore[rule-name]` over `# noqa`.                           | none                                                  |
| `no-del-unused-argument` (`TUF024`)                   | Prefix unused arguments with `_` instead of `del`. Fixable.                 | `excluded_path_parts`                                 |
| `require-subprocess-run-timeout` (`TUF025`)           | Require a non-`None` `timeout` on `subprocess.run`.                         | none                                                  |
| `require-typed-empty-container-assignment` (`TUF026`) | Annotate names assigned an empty list, dict, or set.                        | none                                                  |
| `respect-module-all` (`TUF027`)                       | Import only names a module exports through `__all__`.                       | `source_roots`, `module_exports`                      |
| `no-boilerplate-docstrings` (`TUF028`)                | Remove docstrings and argument entries that restate names. Fixable.         | none                                                  |

`no-del-unused-argument` skips test files and paths containing a part in `excluded_path_parts` (default `["tests", "benchmarks"]`).

`respect-module-all` reads `__all__` from project modules found under `source_roots` (default `["."]`, relative to the configuration root). `module_exports` declares exports for other modules as `module=name1,name2`; these entries take precedence over project files. Relative imports, star imports, `conftest.py`, and test files are skipped.

```toml
[tool.tuff.lint.rules.respect-module-all]
source_roots = ["src"]
module_exports = ["vendor.client=Client,connect"]
```

## Legacy rules

Select all 21 compatibility rules with `enable = ["legacy"]`. Each replaces overlapping Ruff checks when active:

| Rule                                         | Fixable | Replaces Ruff codes           |
| -------------------------------------------- | ------- | ----------------------------- |
| `collapse-isinstance-checks` (`TUF201`)      | No      | `SIM101`                      |
| `deprecated-unittest-asserts` (`TUF202`)     | Yes     | `UP005`                       |
| `no-inherit-from-object` (`TUF203`)          | Yes     | `UP004`                       |
| `no-or-in-except` (`TUF204`)                 | No      | `PLW0711`                     |
| `no-redundant-arguments-super` (`TUF205`)    | No      | `UP008`                       |
| `no-redundant-f-string` (`TUF206`)           | Yes     | `F541`                        |
| `no-redundant-lambda` (`TUF207`)             | No      | `PLW0108`                     |
| `no-redundant-list-comprehension` (`TUF208`) | No      | `C419`                        |
| `no-string-type-annotation` (`TUF209`)       | Yes     | `UP037`, `PYI020`             |
| `replace-union-with-optional` (`TUF210`)     | No      | `UP007`, `UP045`              |
| `use-assert-equal` (`TUF211`)                | Yes     | `PT009`                       |
| `use-assert-in` (`TUF212`)                   | Yes     | `PT009`                       |
| `use-assert-is-not-none` (`TUF213`)          | Yes     | `PT009`                       |
| `use-async-sleep-in-async-def` (`TUF214`)    | No      | `ASYNC251`                    |
| `use-cls-in-classmethod` (`TUF215`)          | Yes     | `N804`                        |
| `use-collections-abc` (`TUF216`)             | Yes     | `UP035`                       |
| `use-comprehension` (`TUF217`)               | No      | `C400`–`C404`, `C411`         |
| `use-eq-for-primitives` (`TUF218`)           | Yes     | `F632`                        |
| `use-f-string` (`TUF219`)                    | Yes     | `UP031`, `UP032`              |
| `use-is-for-singletons` (`TUF220`)           | Yes     | `E711`, `E712`                |
| `use-literal` (`TUF221`)                     | Yes     | `C405`, `C406`, `C408`–`C410` |

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

The port follows Python scoping and evaluation rules where LibCST differs: completed references respect assignment evaluation order, deferred bodies, comprehension locals, and type parameters. Quoted compound annotations use those lexical scopes. Relative imports retain their package meaning.

Literal identity does not assume allocation or interning. Fixes that can invoke arbitrary equality, reorder assertion evaluation, or remove evaluated annotations require `--unsafe-fixes`; unavailable typing names are withheld. Callable recommendations remain advisory.

Diagnostics use Ruff AST spans and source order. Decorated definitions and generator conditions can therefore have different spans, and trailing blank-line cleanup groups a contiguous footer into one report. Unknown external ignore names are rejected. Rendered structure and safe-fix eligibility can also differ from the reference.
