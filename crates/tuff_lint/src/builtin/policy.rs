use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Result, bail};
use indexmap::IndexMap;
use regex::Regex;
use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, NodeKind};
use ruff_source_file::{LineRanges, UniversalNewlines};
use ruff_text_size::{Ranged, TextRange, TextSize};
use serde::Deserialize;

use crate::{
    Context, Finding, LintRule, RuleDefinition, RuleOptions, parse_options, reject_options,
};

mod bindings;
mod filters;

use filters::{
    Advice, ContextFilter, PathScope, Shorthand, dedup_by, dotted_pattern, identifier_pattern,
    optional, relative_path,
};

pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! {
        name: "forbidden-call", code: "TUF001", fixable: false,
        explanation: "Ban configured callable symbols, including aliases.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: forbidden_call,
    }
    .replacing(&["TID251"]),
    crate::native_rule! {
        name: "forbidden-import", code: "TUF002", fixable: false,
        explanation: "Ban configured import boundaries.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: forbidden_import,
    }
    .replacing(&["TID251"]),
    crate::native_rule! {
        name: "forbidden-name", code: "TUF003", fixable: false,
        explanation: "Ban identifier names by kind and shell pattern.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: forbidden_name,
    },
    crate::native_rule! {
        name: "import-contracts", code: "TUF004", fixable: false,
        explanation: "Enforce direct-import forbidden, layers, and independence contracts.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: import_contracts,
    },
    crate::native_rule! {
        name: "line-count-limit", code: "TUF005", fixable: false,
        explanation: "Limit file, function, and method lines with path overrides.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: line_count_limit,
    },
    crate::native_rule! {
        name: "no-relative-imports", code: "TUF006", fixable: false,
        explanation: "Require absolute imports.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: no_relative_imports,
    }
    .replacing(&["TID252"]),
    crate::native_rule! {
        name: "no-underscore-import-aliases", code: "TUF007", fixable: false,
        explanation: "Disallow underscore-prefixed import aliases.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: no_underscore_import_aliases,
    },
    crate::native_rule! {
        name: "no-unsafe-tempfile-factories", code: "TUF008", fixable: false,
        explanation: "Prefer managed tempfile context managers.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: tempfile,
    },
    crate::native_rule! {
        name: "forbidden-attribute", code: "TUF029", fixable: false,
        explanation: "Ban configured attribute accesses and method calls.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: forbidden_attribute,
    },
    crate::native_rule! {
        name: "forbidden-raise", code: "TUF030", fixable: false,
        explanation: "Ban raising configured exception types.",
        node_kinds: &[NodeKind::ModModule], checks_comments: false,
        create: forbidden_raise,
    },
];

fn no_relative_imports(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(options)?;
    Ok(Box::new(Policy::Relative))
}
fn no_underscore_import_aliases(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(options)?;
    Ok(Box::new(Policy::Underscore))
}

#[derive(Debug)]
enum Policy {
    Calls(Vec<CallEntry>),
    Imports(Vec<ImportEntry>),
    Names(Vec<NameEntry>),
    Attributes(Vec<AttributeEntry>),
    Raises(Vec<RaiseEntry>),
    Contracts(ContractOptions),
    Lines(LineOptions),
    Relative,
    Underscore,
    Tempfile(TempOptions),
}

#[derive(Debug)]
struct CallEntry {
    symbol: String,
    advice: Advice,
    scope: PathScope,
    filter: ContextFilter,
    require_keywords: Vec<String>,
    forbid_keywords: Vec<String>,
}

#[derive(Debug)]
struct ImportEntry {
    module: String,
    names: Vec<Regex>,
    advice: Advice,
    scope: PathScope,
}

#[derive(Debug)]
struct NameEntry {
    kind: String,
    matcher: Regex,
    advice: Advice,
    scope: PathScope,
}

#[derive(Debug)]
struct AttributeEntry {
    attribute: Option<Regex>,
    receiver: Option<Regex>,
    qualified_prefix: Option<String>,
    calls_only: bool,
    ignore_writes: bool,
    advice: Advice,
    scope: PathScope,
}

#[derive(Debug)]
struct RaiseEntry {
    exception: Regex,
    advice: Advice,
    scope: PathScope,
    filter: ContextFilter,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CallOptions {
    forbidden_calls: Vec<Shorthand<CallTable>>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ImportOptions {
    forbidden_imports: Vec<Shorthand<ImportTable>>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NameOptions {
    forbidden_names: Vec<Shorthand<NameTable>>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AttributeOptions {
    forbidden_attributes: Vec<Shorthand<AttributeTable>>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RaiseOptions {
    forbidden_raises: Vec<Shorthand<RaiseTable>>,
}
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TempOptions {
    excluded_path_parts: Vec<String>,
}
impl Default for TempOptions {
    fn default() -> Self {
        Self {
            excluded_path_parts: vec!["tests".into(), "benchmarks".into()],
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallTable {
    symbol: String,
    message: Option<String>,
    replacement: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    in_async: Option<bool>,
    #[serde(default)]
    decorators: Vec<String>,
    #[serde(default)]
    require_keywords: Vec<String>,
    #[serde(default)]
    forbid_keywords: Vec<String>,
}
impl CallTable {
    fn same_selector(&self, other: &Self) -> bool {
        self.symbol == other.symbol
            && self.include == other.include
            && self.exclude == other.exclude
            && self.in_async == other.in_async
            && self.decorators == other.decorators
            && self.require_keywords == other.require_keywords
            && self.forbid_keywords == other.forbid_keywords
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportTable {
    module: String,
    #[serde(default)]
    names: Vec<String>,
    message: Option<String>,
    replacement: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}
impl ImportTable {
    fn same_selector(&self, other: &Self) -> bool {
        self.module == other.module
            && self.names == other.names
            && self.include == other.include
            && self.exclude == other.exclude
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct NameTable {
    kind: String,
    pattern: String,
    message: Option<String>,
    replacement: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}
impl NameTable {
    fn same_selector(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.pattern == other.pattern
            && self.include == other.include
            && self.exclude == other.exclude
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributeTable {
    attribute: Option<String>,
    receiver: Option<String>,
    qualified_prefix: Option<String>,
    #[serde(default)]
    calls_only: bool,
    #[serde(default)]
    ignore_writes: bool,
    message: Option<String>,
    replacement: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}
impl AttributeTable {
    fn same_selector(&self, other: &Self) -> bool {
        self.attribute == other.attribute
            && self.receiver == other.receiver
            && self.qualified_prefix == other.qualified_prefix
            && self.calls_only == other.calls_only
            && self.ignore_writes == other.ignore_writes
            && self.include == other.include
            && self.exclude == other.exclude
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RaiseTable {
    exception: String,
    message: Option<String>,
    replacement: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    in_async: Option<bool>,
    #[serde(default)]
    decorators: Vec<String>,
}
impl RaiseTable {
    fn same_selector(&self, other: &Self) -> bool {
        self.exception == other.exception
            && self.include == other.include
            && self.exclude == other.exclude
            && self.in_async == other.in_async
            && self.decorators == other.decorators
    }
}

fn ascii_symbol(value: &str) -> bool {
    static SYMBOL: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    SYMBOL
        .get_or_init(|| Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*\z"))
        .as_ref()
        .is_ok_and(|matcher| matcher.is_match(value))
}

fn replacement(value: Option<&str>, valid: fn(&str) -> bool) -> Result<Option<String>> {
    let replacement = optional(value);
    if let Some(replacement) = &replacement
        && !valid(replacement)
    {
        bail!("invalid replacement: {replacement:?}");
    }
    Ok(replacement)
}

fn identifiers(values: &[String], what: &str) -> Result<()> {
    if let Some(value) = values.iter().find(|value| !identifier(value)) {
        bail!("invalid {what}: {value:?}");
    }
    Ok(())
}

fn forbidden_call(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: CallOptions = parse_options(options)?;
    let tables = options
        .forbidden_calls
        .into_iter()
        .map(|entry| match entry {
            Shorthand::Table(table) => table,
            Shorthand::Text(text) => {
                let mut fields = text.splitn(3, '|');
                CallTable {
                    symbol: fields.next().unwrap_or_default().to_owned(),
                    message: fields.next().map(str::to_owned),
                    replacement: fields.next().map(str::to_owned),
                    ..CallTable::default()
                }
            }
        })
        .collect();
    let entries = dedup_by(tables, CallTable::same_selector)
        .into_iter()
        .map(|table| {
            if !ascii_symbol(&table.symbol) {
                bail!("invalid forbidden call symbol: {:?}", table.symbol);
            }
            identifiers(&table.require_keywords, "required keyword")?;
            identifiers(&table.forbid_keywords, "forbidden keyword")?;
            Ok(CallEntry {
                advice: Advice::new(
                    table.message.as_deref(),
                    replacement(table.replacement.as_deref(), ascii_symbol)?,
                ),
                scope: PathScope::new(&table.include, &table.exclude)?,
                filter: ContextFilter::new(table.in_async, &table.decorators)?,
                symbol: table.symbol,
                require_keywords: table.require_keywords,
                forbid_keywords: table.forbid_keywords,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Box::new(Policy::Calls(entries)))
}
fn forbidden_import(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: ImportOptions = parse_options(options)?;
    let tables = options
        .forbidden_imports
        .into_iter()
        .map(|entry| match entry {
            Shorthand::Table(table) => table,
            Shorthand::Text(text) => {
                let (module, message) = text
                    .split_once('|')
                    .map_or((text.as_str(), None), |(module, message)| {
                        (module, Some(message.to_owned()))
                    });
                ImportTable {
                    module: module.to_owned(),
                    message,
                    ..ImportTable::default()
                }
            }
        })
        .collect();
    let mut entries = dedup_by(tables, ImportTable::same_selector)
        .into_iter()
        .map(|table| {
            if !module_identifier(&table.module) {
                bail!("invalid forbidden import module: {:?}", table.module);
            }
            Ok(ImportEntry {
                names: table
                    .names
                    .iter()
                    .map(|pattern| identifier_pattern(pattern))
                    .collect::<Result<_>>()?,
                advice: Advice::new(
                    table.message.as_deref(),
                    replacement(table.replacement.as_deref(), module_identifier)?,
                ),
                scope: PathScope::new(&table.include, &table.exclude)?,
                module: table.module,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // The most specific boundary decides the message; name filters narrow a module boundary.
    entries.sort_by_key(|entry| {
        std::cmp::Reverse((
            entry.module.matches('.').count(),
            entry.module.chars().count(),
            !entry.names.is_empty(),
        ))
    });
    Ok(Box::new(Policy::Imports(entries)))
}
const NAME_KINDS: &[&str] = &[
    "any",
    "variable",
    "parameter",
    "function",
    "class",
    "attribute",
    "import",
    "alias",
];
fn forbidden_name(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: NameOptions = parse_options(options)?;
    let pattern_syntax = Regex::new(r"\A[A-Za-z_*?\[\]!][A-Za-z0-9_*?\[\]!-]*\z")?;
    let tables = options
        .forbidden_names
        .into_iter()
        .map(|entry| match entry {
            Shorthand::Table(table) => Ok(table),
            Shorthand::Text(text) => {
                let (rule, message) = text
                    .split_once('|')
                    .map_or((text.as_str(), None), |(rule, message)| {
                        (rule, Some(message.to_owned()))
                    });
                let Some((kind, pattern)) = rule.split_once(':') else {
                    bail!("invalid forbidden name entry: {text:?}");
                };
                Ok(NameTable {
                    kind: kind.to_owned(),
                    pattern: pattern.to_owned(),
                    message,
                    ..NameTable::default()
                })
            }
        })
        .collect::<Result<_>>()?;
    let entries = dedup_by(tables, NameTable::same_selector)
        .into_iter()
        .map(|table| {
            if !NAME_KINDS.contains(&table.kind.as_str())
                || !pattern_syntax.is_match(&table.pattern)
            {
                bail!(
                    "invalid forbidden name entry: {}:{}",
                    table.kind,
                    table.pattern
                );
            }
            Ok(NameEntry {
                matcher: shell_pattern(&table.pattern)?,
                advice: Advice::new(
                    table.message.as_deref(),
                    replacement(table.replacement.as_deref(), identifier)?,
                ),
                scope: PathScope::new(&table.include, &table.exclude)?,
                kind: table.kind,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Box::new(Policy::Names(entries)))
}
fn forbidden_attribute(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: AttributeOptions = parse_options(options)?;
    let tables = options
        .forbidden_attributes
        .into_iter()
        .map(|entry| match entry {
            Shorthand::Table(table) => table,
            Shorthand::Text(text) => {
                let (pattern, message) = text
                    .split_once('|')
                    .map_or((text.as_str(), None), |(pattern, message)| {
                        (pattern, Some(message.to_owned()))
                    });
                // `*.name` places no constraint on the receiver.
                let (receiver, attribute) = match pattern.rsplit_once('.') {
                    Some(("*", attribute)) => (None, attribute),
                    Some((receiver, attribute)) => (Some(receiver.to_owned()), attribute),
                    None => (None, pattern),
                };
                AttributeTable {
                    attribute: Some(attribute.to_owned()),
                    receiver,
                    message,
                    ..AttributeTable::default()
                }
            }
        })
        .collect();
    let entries = dedup_by(tables, AttributeTable::same_selector)
        .into_iter()
        .map(|table| {
            if table.attribute.is_none() && table.qualified_prefix.is_none() {
                bail!("forbidden attribute entries need `attribute` or `qualified_prefix`");
            }
            if let Some(prefix) = &table.qualified_prefix
                && !module_identifier(prefix)
            {
                bail!("invalid qualified prefix: {prefix:?}");
            }
            Ok(AttributeEntry {
                attribute: table
                    .attribute
                    .as_deref()
                    .map(identifier_pattern)
                    .transpose()?,
                receiver: table.receiver.as_deref().map(dotted_pattern).transpose()?,
                qualified_prefix: table.qualified_prefix,
                calls_only: table.calls_only,
                ignore_writes: table.ignore_writes,
                advice: Advice::new(
                    table.message.as_deref(),
                    replacement(table.replacement.as_deref(), module_identifier)?,
                ),
                scope: PathScope::new(&table.include, &table.exclude)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Box::new(Policy::Attributes(entries)))
}
fn forbidden_raise(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: RaiseOptions = parse_options(options)?;
    let tables = options
        .forbidden_raises
        .into_iter()
        .map(|entry| match entry {
            Shorthand::Table(table) => table,
            Shorthand::Text(text) => {
                let (exception, message) = text
                    .split_once('|')
                    .map_or((text.as_str(), None), |(exception, message)| {
                        (exception, Some(message.to_owned()))
                    });
                RaiseTable {
                    exception: exception.to_owned(),
                    message,
                    ..RaiseTable::default()
                }
            }
        })
        .collect();
    let entries = dedup_by(tables, RaiseTable::same_selector)
        .into_iter()
        .map(|table| {
            Ok(RaiseEntry {
                exception: dotted_pattern(&table.exception)?,
                advice: Advice::new(
                    table.message.as_deref(),
                    replacement(table.replacement.as_deref(), module_identifier)?,
                ),
                scope: PathScope::new(&table.include, &table.exclude)?,
                filter: ContextFilter::new(table.in_async, &table.decorators)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Box::new(Policy::Raises(entries)))
}
fn tempfile(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    Ok(Box::new(Policy::Tempfile(parse_options(options)?)))
}

// Python fnmatchcase: '*' also matches slashes and leading dots; malformed '[' is literal.
fn shell_pattern(pattern: &str) -> Result<Regex> {
    let chars: Vec<_> = pattern.chars().collect();
    let mut expression = String::from("\\A(?s:");
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' => expression.push_str(".*"),
            '?' => expression.push('.'),
            '[' => {
                let mut end = index + 1;
                if chars.get(end) == Some(&'!') {
                    end += 1;
                }
                if chars.get(end) == Some(&']') {
                    end += 1;
                }
                while end < chars.len() && chars[end] != ']' {
                    end += 1;
                }
                if end == chars.len() {
                    expression.push_str("\\[");
                } else {
                    let mut start = index + 1;
                    let negate = chars.get(start) == Some(&'!');
                    if negate {
                        start += 1;
                    }
                    let mut content = String::new();
                    while start < end {
                        if start + 2 < end && chars[start + 1] == '-' {
                            if chars[start] <= chars[start + 2] {
                                write!(
                                    content,
                                    "\\x{{{:x}}}-\\x{{{:x}}}",
                                    u32::from(chars[start]),
                                    u32::from(chars[start + 2])
                                )?;
                            }
                            start += 3;
                        } else {
                            write!(content, "\\x{{{:x}}}", u32::from(chars[start]))?;
                            start += 1;
                        }
                    }
                    if content.is_empty() {
                        expression.push_str(if negate { "." } else { "[^\\s\\S]" });
                    } else {
                        expression.push('[');
                        if negate {
                            expression.push('^');
                        }
                        expression.push_str(&content);
                        expression.push(']');
                    }
                    index = end;
                }
            }
            character => expression.push_str(&regex::escape(&character.to_string())),
        }
        index += 1;
    }
    expression.push_str(")\\z");
    Ok(Regex::new(&expression)?)
}

fn identifier(value: &str) -> bool {
    static IDENTIFIER: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    IDENTIFIER
        .get_or_init(|| Regex::new(r"\A[_\p{XID_Start}][_\p{XID_Continue}]*\z"))
        .as_ref()
        .is_ok_and(|matcher| matcher.is_match(value))
}
fn module_identifier(value: &str) -> bool {
    value.split('.').all(identifier)
}
fn boundary(target: &str, base: &str) -> bool {
    target == base
        || target
            .strip_prefix(base)
            .is_some_and(|suffix| suffix.starts_with('.'))
}
fn join(module: &str, member: &str) -> String {
    if module.is_empty() {
        member.to_owned()
    } else if member.is_empty() {
        module.to_owned()
    } else {
        format!("{module}.{member}")
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ContractOptions {
    source_roots: Vec<String>,
    contracts: Vec<Contract>,
    ignore_type_checking: bool,
}
impl Default for ContractOptions {
    fn default() -> Self {
        Self {
            source_roots: vec![".".into()],
            contracts: Vec::new(),
            ignore_type_checking: true,
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Contract {
    Forbidden {
        name: String,
        source: Vec<String>,
        forbid: Vec<String>,
        #[serde(default)]
        allow: Vec<String>,
        #[serde(default)]
        message: String,
    },
    Layers {
        name: String,
        layers: Vec<Layer>,
        #[serde(default)]
        root: String,
        #[serde(default)]
        message: String,
    },
    Independence {
        name: String,
        packages: Vec<String>,
        #[serde(default)]
        public: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        #[serde(default)]
        message: String,
    },
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    modules: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}
impl Contract {
    fn name_message(&self) -> (&str, &str) {
        match self {
            Self::Forbidden { name, message, .. }
            | Self::Layers { name, message, .. }
            | Self::Independence { name, message, .. } => (name, message),
        }
    }
    fn breaks(&self, source: &str, target: &str) -> bool {
        match self {
            Self::Forbidden {
                source: patterns,
                forbid,
                allow,
                ..
            } => {
                matches_any(patterns, source)
                    && !matches_any(allow, source)
                    && prefixes(target)
                        .iter()
                        .any(|prefix| matches_any(forbid, prefix))
            }
            Self::Layers { layers, root, .. } => {
                let source_layer = layer_index(layers, root, source);
                let target_layer = prefixes(target)
                    .iter()
                    .rev()
                    .find_map(|prefix| layer_index(layers, root, prefix));
                matches!((source_layer, target_layer), (Some(source), Some(target)) if source > target)
            }
            Self::Independence {
                packages,
                public,
                exclude,
                ..
            } => {
                let source_package = sibling(packages, exclude, source);
                let target_package = sibling(packages, exclude, target);
                match (source_package, target_package) {
                    (Some(source), Some(package)) if source != package => {
                        let public_path = target.get(package.len() + 1..).unwrap_or("");
                        !prefixes(public_path)
                            .iter()
                            .any(|prefix| matches_any(public, prefix))
                    }
                    _ => false,
                }
            }
        }
    }
}
fn validate_patterns(patterns: &[String], required: bool) -> Result<()> {
    if required && patterns.is_empty() {
        bail!("expected at least one module pattern");
    }
    for pattern in patterns {
        if pattern
            .split('.')
            .any(|part| part != "*" && part != "**" && !identifier(part))
        {
            bail!("invalid module pattern: {pattern:?}");
        }
    }
    Ok(())
}
fn import_contracts(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: ContractOptions = parse_options(options)?;
    if options.source_roots.is_empty()
        || options
            .source_roots
            .iter()
            .any(|root| !is_contained_relative_path(root))
    {
        bail!("source_roots must be nonempty paths within the config root");
    }
    let mut names = HashSet::new();
    for contract in &options.contracts {
        let (name, _) = contract.name_message();
        if name.trim().is_empty() || !names.insert(name) {
            bail!("contract names must be nonempty and unique");
        }
        match contract {
            Contract::Forbidden {
                source,
                forbid,
                allow,
                ..
            } => {
                validate_patterns(source, true)?;
                validate_patterns(forbid, true)?;
                validate_patterns(allow, false)?;
            }
            Contract::Layers { root, layers, .. } => {
                if !root.is_empty() && !module_identifier(root) {
                    bail!("layer root must be a literal module name");
                }
                if layers.len() < 2 {
                    bail!("expected at least two layers, high to low");
                }
                for layer in layers {
                    validate_patterns(&layer.modules, true)?;
                    validate_patterns(&layer.exclude, false)?;
                }
            }
            Contract::Independence {
                packages,
                public,
                exclude,
                ..
            } => {
                validate_patterns(packages, true)?;
                validate_patterns(public, false)?;
                validate_patterns(exclude, false)?;
                let parents: HashSet<_> = packages
                    .iter()
                    .map(|pattern| pattern.rsplit_once('.').map_or("", |(parent, _)| parent))
                    .collect();
                if parents.len() != 1 || packages.iter().any(|pattern| pattern.contains("**")) {
                    bail!("independent packages must be siblings without '**'");
                }
            }
        }
    }
    Ok(Box::new(Policy::Contracts(options)))
}
pub(super) fn is_contained_relative_path(path: &str) -> bool {
    // Windows treats `\dir`, `C:dir`, and `C:\dir` as rooted or drive-relative. They escape the
    // config root there, so reject them on every platform to keep configs portable.
    let has_drive = matches!(path.as_bytes(), [drive, b':', ..] if drive.is_ascii_alphabetic());
    !(path.starts_with(['/', '\\'])
        || has_drive
        || path.split(['/', '\\']).any(|part| part == ".."))
}
fn module_match(pattern: &str, module: &str) -> bool {
    let pattern: Vec<_> = pattern.split('.').collect();
    let module: Vec<_> = if module.is_empty() {
        Vec::new()
    } else {
        module.split('.').collect()
    };
    let mut state = vec![false; module.len() + 1];
    state[0] = true;
    for part in pattern {
        if part == "**" {
            for index in 1..state.len() {
                state[index] |= state[index - 1];
            }
        } else {
            for index in (1..state.len()).rev() {
                state[index] = state[index - 1] && (part == "*" || part == module[index - 1]);
            }
            state[0] = false;
        }
    }
    state[module.len()]
}
fn matches_any(patterns: &[String], module: &str) -> bool {
    patterns.iter().any(|pattern| module_match(pattern, module))
}
fn prefixes(module: &str) -> Vec<&str> {
    module
        .match_indices('.')
        .map(|(index, _)| &module[..index])
        .chain(std::iter::once(module))
        .collect()
}
fn layer_index(layers: &[Layer], root: &str, module: &str) -> Option<usize> {
    let module = if root.is_empty() {
        module
    } else {
        module.strip_prefix(root)?.strip_prefix('.')?
    };
    layers.iter().position(|layer| {
        matches_any(&layer.modules, module) && !matches_any(&layer.exclude, module)
    })
}
fn sibling(packages: &[String], exclude: &[String], module: &str) -> Option<String> {
    if matches_any(exclude, module) {
        return None;
    }
    packages.iter().find_map(|pattern| {
        let prefix = module
            .split('.')
            .take(pattern.split('.').count())
            .collect::<Vec<_>>()
            .join(".");
        module_match(pattern, &prefix).then_some(prefix)
    })
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Limits {
    #[serde(rename = "max_file_lines")]
    file: usize,
    #[serde(rename = "max_function_lines")]
    function: usize,
    #[serde(rename = "max_method_lines")]
    method: usize,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitOverride {
    #[serde(rename = "max_file_lines")]
    file: Option<usize>,
    #[serde(rename = "max_function_lines")]
    function: Option<usize>,
    #[serde(rename = "max_method_lines")]
    method: Option<usize>,
}
impl Limits {
    fn apply(&mut self, other: &LimitOverride) {
        if let Some(value) = other.file {
            self.file = value;
        }
        if let Some(value) = other.function {
            self.function = value;
        }
        if let Some(value) = other.method {
            self.method = value;
        }
    }
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawLineOptions {
    max_file_lines: usize,
    max_function_lines: usize,
    max_method_lines: usize,
    glob_limits: IndexMap<String, LimitOverride>,
    per_file_limits: IndexMap<String, LimitOverride>,
}
#[derive(Debug)]
struct LineOptions {
    base: Limits,
    globs: Vec<(String, Regex, LimitOverride)>,
    files: IndexMap<String, LimitOverride>,
}
fn line_count_limit(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let raw: RawLineOptions = parse_options(options)?;
    for (pattern, limits) in raw.glob_limits.iter().chain(&raw.per_file_limits) {
        if pattern.is_empty()
            || (limits.file.is_none() && limits.function.is_none() && limits.method.is_none())
        {
            bail!("expected nonempty path pattern and at least one limit");
        }
    }
    let mut globs = raw
        .glob_limits
        .into_iter()
        .map(|(pattern, limits)| {
            Ok((
                pattern.clone(),
                shell_pattern(&normalize_pattern(&pattern))?,
                limits,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    globs.sort_by_key(|(pattern, _, _)| {
        let wildcards = pattern
            .chars()
            .filter(|ch| matches!(ch, '*' | '?' | '['))
            .count();
        (
            pattern.chars().count() - wildcards,
            std::cmp::Reverse(wildcards),
            pattern.chars().count(),
        )
    });
    Ok(Box::new(Policy::Lines(LineOptions {
        base: Limits {
            file: raw.max_file_lines,
            function: raw.max_function_lines,
            method: raw.max_method_lines,
        },
        globs,
        files: raw.per_file_limits,
    })))
}
fn normalize_pattern(pattern: &str) -> String {
    Path::new(pattern)
        .components()
        .filter(|part| *part != Component::CurDir)
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned()
}
fn absolute(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
impl LineOptions {
    fn limits(&self, context: &Context<'_, '_, '_>) -> Limits {
        let mut limits = self.base;
        let path = absolute(context.path);
        let root = absolute(context.root);
        let relative = path
            .strip_prefix(root)
            .ok()
            .map(|path| path.to_string_lossy().into_owned());
        for (pattern, matcher, values) in &self.globs {
            let candidate = if Path::new(pattern).is_absolute() {
                Some(path.to_string_lossy().into_owned())
            } else {
                relative.clone()
            };
            if candidate.is_some_and(|candidate| matcher.is_match(&candidate)) {
                limits.apply(values);
            }
        }
        for (pattern, values) in &self.files {
            let matches = if Path::new(pattern).is_absolute() {
                path == absolute(Path::new(pattern))
            } else {
                relative
                    .as_ref()
                    .is_some_and(|relative| *relative == normalize_pattern(pattern))
            };
            if matches {
                limits.apply(values);
            }
        }
        limits
    }
}

impl Policy {
    fn scopes(&self) -> Option<Vec<&PathScope>> {
        Some(match self {
            Self::Calls(entries) => entries.iter().map(|entry| &entry.scope).collect(),
            Self::Imports(entries) => entries.iter().map(|entry| &entry.scope).collect(),
            Self::Names(entries) => entries.iter().map(|entry| &entry.scope).collect(),
            Self::Attributes(entries) => entries.iter().map(|entry| &entry.scope).collect(),
            Self::Raises(entries) => entries.iter().map(|entry| &entry.scope).collect(),
            _ => return None,
        })
    }
}

impl LintRule for Policy {
    fn is_enabled(&self) -> bool {
        match self {
            Self::Calls(entries) => !entries.is_empty(),
            Self::Imports(entries) => !entries.is_empty(),
            Self::Names(entries) => !entries.is_empty(),
            Self::Attributes(entries) => !entries.is_empty(),
            Self::Raises(entries) => !entries.is_empty(),
            Self::Contracts(options) => !options.contracts.is_empty(),
            Self::Lines(options) => {
                options.base.file > 0
                    || options.base.function > 0
                    || options.base.method > 0
                    || !options.globs.is_empty()
                    || !options.files.is_empty()
            }
            Self::Relative | Self::Underscore | Self::Tempfile(_) => true,
        }
    }

    fn check(
        &self,
        node: AnyNodeRef<'_>,
        context: &Context<'_, '_, '_>,
        findings: &mut Vec<Finding>,
    ) {
        let AnyNodeRef::ModModule(_) = node else {
            return;
        };
        let module = context.module;
        if let Self::Tempfile(options) = self {
            if context.path.components().any(|part| {
                options
                    .excluded_path_parts
                    .iter()
                    .any(|excluded| part.as_os_str() == excluded.as_str())
            }) {
                return;
            }
        }
        let path = if let Some(scopes) = self.scopes() {
            let path = if scopes.iter().all(|scope| scope.is_unrestricted()) {
                None
            } else {
                relative_path(context)
            };
            if !scopes.iter().any(|scope| scope.allows(path.as_deref())) {
                return;
            }
            path
        } else {
            None
        };
        let identity = if let Self::Contracts(options) = self {
            module_identity(context, &options.source_roots)
        } else {
            None
        };
        let limits = if let Self::Lines(options) = self {
            options.limits(context)
        } else {
            Limits::default()
        };
        if matches!(self, Self::Lines(_)) {
            let count = splitline_count(context.source);
            if limits.file > 0 && count > limits.file {
                findings.push(Finding::new(
                    "line-count-limit",
                    context.source.full_line_range(TextSize::ZERO),
                    format!(
                        "File has {count} lines, exceeding the configured limit of {}.",
                        limits.file
                    ),
                ));
            }
        }
        let mut visitor = PolicyVisitor {
            policy: self,
            context,
            findings,
            ancestors: Vec::new(),
            identity,
            limits,
            type_only: Vec::new(),
            path,
        };
        AnyNodeRef::ModModule(module).visit_source_order(&mut visitor);
    }
}
fn splitline_count(source: &str) -> usize {
    let mut chars = source.chars().peekable();
    let mut lines = 0;
    let mut trailing_content = false;
    while let Some(character) = chars.next() {
        if matches!(
            character,
            '\n' | '\r'
                | '\u{000b}'
                | '\u{000c}'
                | '\u{001c}'
                | '\u{001d}'
                | '\u{001e}'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            if character == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            lines += 1;
            trailing_content = false;
        } else {
            trailing_content = true;
        }
    }
    lines + usize::from(trailing_content)
}
fn module_identity(context: &Context<'_, '_, '_>, roots: &[String]) -> Option<(String, String)> {
    let path = absolute(context.path);
    let mut roots: Vec<_> = roots
        .iter()
        .map(|root| absolute(&context.root.join(root)))
        .collect();
    roots.sort_by_key(|root| std::cmp::Reverse(root.components().count()));
    for root in roots {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.with_extension("");
        let mut parts: Vec<_> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        let package = parts.last().is_some_and(|part| part == "__init__");
        if package {
            parts.pop();
        }
        if parts.is_empty() || parts.iter().any(|part| !identifier(part)) {
            return None;
        }
        let module = parts.join(".");
        if !package {
            parts.pop();
        }
        return Some((module, parts.join(".")));
    }
    None
}

struct PolicyVisitor<'rule, 'ctx, 'ast, 'src> {
    policy: &'rule Policy,
    context: &'rule Context<'ctx, 'ast, 'src>,
    findings: &'rule mut Vec<Finding>,
    ancestors: Vec<AnyNodeRef<'ast>>,
    identity: Option<(String, String)>,
    limits: Limits,
    type_only: Vec<TextRange>,
    path: Option<String>,
}
impl PolicyVisitor<'_, '_, '_, '_> {
    fn report_name(&mut self, name: &str, range: TextRange, kind: &str) {
        let Policy::Names(entries) = self.policy else {
            return;
        };
        if let Some(entry) = entries.iter().find(|entry| {
            (entry.kind == kind || entry.kind == "any")
                && entry.matcher.is_match(name)
                && entry.scope.allows(self.path.as_deref())
        }) {
            self.findings.push(Finding::new(
                "forbidden-name",
                range,
                entry
                    .advice
                    .render(|| format!("Do not use forbidden {kind} name '{name}'.")),
            ));
        }
    }
    fn dotted_import_names(&mut self, name: &ast::Identifier) {
        // Use Python token boundaries, including the full Unicode identifier alphabet.
        // Identifier ranges can contain whitespace and continuations between dotted components.
        let source = self.context.text(name.range());
        let mut lexer =
            ruff_python_parser::lexer::lex(source, ruff_python_parser::Mode::Expression);
        loop {
            let kind = lexer.next_token();
            if kind.is_eof() {
                break;
            }
            if kind == TokenKind::Identifier || kind.is_soft_keyword() {
                let token_range = lexer.current_range();
                let range = TextRange::new(
                    name.start() + token_range.start(),
                    name.start() + token_range.end(),
                );
                self.report_name(&source[token_range], range, "import");
            }
        }
    }
    fn import(&mut self, alias: &ast::Alias, module: &str, name: Option<&str>, level: u32) {
        let target = match name {
            None | Some("*") => module.to_owned(),
            Some(name) => join(module, name),
        };
        if let Policy::Imports(entries) = self.policy {
            self.forbidden_import(entries, alias, module, name, level);
        }
        if let Policy::Contracts(options) = self.policy {
            let Some((source, package)) = &self.identity else {
                return;
            };
            if options.ignore_type_checking
                && self
                    .type_only
                    .iter()
                    .any(|range| range.contains_range(alias.range()))
            {
                return;
            }
            let target = if level == 0 {
                target
            } else {
                let parts: Vec<_> = package.split('.').filter(|part| !part.is_empty()).collect();
                if level as usize > parts.len() {
                    return;
                }
                join(&parts[..=(parts.len() - level as usize)].join("."), &target)
            };
            for contract in &options.contracts {
                if contract.breaks(source, &target) {
                    let (name, extra) = contract.name_message();
                    let mut message = format!(
                        "Import contract '{name}' forbids '{source}' importing '{target}'."
                    );
                    if !extra.is_empty() {
                        message.push(' ');
                        message.push_str(extra);
                    }
                    self.findings
                        .push(Finding::new("import-contracts", alias.range(), message));
                }
            }
        }
    }
    fn forbidden_import(
        &mut self,
        entries: &[ImportEntry],
        alias: &ast::Alias,
        module: &str,
        name: Option<&str>,
        level: u32,
    ) {
        let modules = if level == 0 {
            vec![module.to_owned()]
        } else {
            let mut parts: Vec<_> = self
                .context
                .path
                .parent()
                .into_iter()
                .flat_map(Path::components)
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            parts.truncate(parts.len().saturating_sub(level.saturating_sub(1) as usize));
            let count = parts.len();
            if !module.is_empty() {
                parts.extend(module.split('.').map(str::to_owned));
            }
            (0..count).map(|index| parts[index..].join(".")).collect()
        };
        let Some(entry) = entries.iter().find(|entry| {
            let imported = if entry.names.is_empty() {
                modules.iter().any(|module| {
                    let target = match name {
                        None | Some("*") => module.clone(),
                        Some(name) => join(module, name),
                    };
                    boundary(&target, &entry.module)
                })
            } else {
                // Name filters apply to `from module import name`, including star imports.
                name.is_some_and(|name| {
                    (name == "*" || entry.names.iter().any(|pattern| pattern.is_match(name)))
                        && modules.contains(&entry.module)
                })
            };
            imported && entry.scope.allows(self.path.as_deref())
        }) else {
            return;
        };
        let message = entry.advice.render(|| match name {
            Some(name) if !entry.names.is_empty() => {
                format!(
                    "Do not import forbidden name '{name}' from '{}'.",
                    entry.module
                )
            }
            _ => format!(
                "Do not import across forbidden boundary '{}'.",
                entry.module
            ),
        });
        self.findings
            .push(Finding::new("forbidden-import", alias.range(), message));
    }
    fn runtime_truth(&self, expression: &Expr) -> Option<bool> {
        if self
            .context
            .qualified_name(expression)
            .is_some_and(|symbol| {
                symbol == "typing.TYPE_CHECKING" || symbol == "typing_extensions.TYPE_CHECKING"
            })
        {
            return Some(false);
        }
        match expression {
            Expr::UnaryOp(unary) if unary.op == ast::UnaryOp::Not => {
                self.runtime_truth(&unary.operand).map(|truth| !truth)
            }
            Expr::BoolOp(boolean) => {
                let decisive = boolean.op == ast::BoolOp::Or;
                let values: Vec<_> = boolean
                    .values
                    .iter()
                    .map(|value| self.runtime_truth(value))
                    .collect();
                if values.contains(&Some(decisive)) {
                    Some(decisive)
                } else if values.iter().all(|value| *value == Some(!decisive)) {
                    Some(!decisive)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn mark_type_only(&mut self, statement: &ast::StmtIf) {
        let truth = self.runtime_truth(&statement.test);
        if truth == Some(false) {
            self.type_only
                .extend(statement.body.iter().map(Ranged::range));
        }
        if truth == Some(true) {
            self.type_only
                .extend(statement.elif_else_clauses.iter().map(Ranged::range));
        }
        // Each elif's body has its own test, even though Ruff stores the chain flat.
        for (index, clause) in statement.elif_else_clauses.iter().enumerate() {
            if let Some(test) = &clause.test {
                match self.runtime_truth(test) {
                    Some(false) => self.type_only.extend(clause.body.iter().map(Ranged::range)),
                    Some(true) => self.type_only.extend(
                        statement.elif_else_clauses[index + 1..]
                            .iter()
                            .map(Ranged::range),
                    ),
                    None => {}
                }
            }
        }
    }
    fn call(&mut self, call: &ast::ExprCall) {
        let star_modules = if matches!(self.policy, Policy::Tempfile(_)) {
            &["tempfile"][..]
        } else {
            &[]
        };
        let symbol = super::typing::resolve(self.context, &call.func, star_modules);
        match self.policy {
            Policy::Calls(entries) => {
                let dotted = dotted(&call.func);
                let known_local = bindings::is_local(self.context, &call.func);
                let canonical = |entry: &CallEntry| {
                    symbol
                        .as_ref()
                        .is_some_and(|symbol| symbol == &entry.symbol)
                };
                let plain = |entry: &CallEntry| {
                    (!known_local
                        && dotted.as_ref().is_some_and(|name| {
                            *name == entry.symbol && (symbol.is_none() || !name.contains('.'))
                        }))
                        || (symbol.is_none()
                            && !known_local
                            && matches!(call.func.as_ref(), Expr::Name(_))
                            && bindings::star_matches(
                                self.context,
                                &entry.symbol,
                                dotted.as_deref().unwrap_or(""),
                                &call.func,
                            ))
                };
                // Canonical provenance wins over any earlier plain-name fallback.
                let reported = entries
                    .iter()
                    .filter(|entry| canonical(entry))
                    .chain(entries.iter().filter(|entry| plain(entry)))
                    .find_map(|entry| Some((entry, self.call_violation(entry, call)?)));
                if let Some((entry, violation)) = reported {
                    let message = entry.advice.render(|| match violation {
                        CallViolation::Call => {
                            format!("Do not call forbidden callable '{}'.", entry.symbol)
                        }
                        CallViolation::MissingKeyword(keyword) => format!(
                            "Call to '{}' must pass keyword argument '{keyword}'.",
                            entry.symbol
                        ),
                        CallViolation::ForbiddenKeyword(keyword) => format!(
                            "Call to '{}' must not pass keyword argument '{keyword}'.",
                            entry.symbol
                        ),
                    });
                    self.findings
                        .push(Finding::new("forbidden-call", call.func.range(), message));
                }
            }
            Policy::Tempfile(_) => {
                let star = symbol.is_none()
                    && !bindings::is_local(self.context, &call.func)
                    && dotted(&call.func).is_some_and(|name| {
                        matches!(name.as_str(), "mkstemp" | "mkdtemp")
                            && bindings::star_matches(
                                self.context,
                                &format!("tempfile.{name}"),
                                &name,
                                &call.func,
                            )
                    });
                if symbol
                    .as_deref()
                    .is_some_and(|symbol| matches!(symbol, "tempfile.mkstemp" | "tempfile.mkdtemp"))
                    || star
                {
                    self.findings.push(Finding::new(
                        "no-unsafe-tempfile-factories",
                        call.func.range(),
                        "Use tempfile context managers instead of mkstemp or mkdtemp.",
                    ));
                }
            }
            _ => {}
        }
    }
    fn call_violation<'entry>(
        &self,
        entry: &'entry CallEntry,
        call: &ast::ExprCall,
    ) -> Option<CallViolation<'entry>> {
        if !entry.scope.allows(self.path.as_deref())
            || !entry
                .filter
                .allows(self.context, &self.ancestors, call.range())
        {
            return None;
        }
        if entry.require_keywords.is_empty() && entry.forbid_keywords.is_empty() {
            return Some(CallViolation::Call);
        }
        let keywords = &call.arguments.keywords;
        let passed = |name: &str| {
            keywords
                .iter()
                .any(|keyword| keyword.arg.as_ref().is_some_and(|arg| arg.as_str() == name))
        };
        if let Some(keyword) = entry.forbid_keywords.iter().find(|name| passed(name)) {
            return Some(CallViolation::ForbiddenKeyword(keyword));
        }
        // `**options` may supply any keyword, so a required keyword cannot be proven missing.
        if keywords.iter().any(|keyword| keyword.arg.is_none()) {
            return None;
        }
        entry
            .require_keywords
            .iter()
            .find(|name| !passed(name))
            .map(|keyword| CallViolation::MissingKeyword(keyword))
    }
    fn attribute(&mut self, attribute: &ast::ExprAttribute) {
        let Policy::Attributes(entries) = self.policy else {
            return;
        };
        let called = matches!(self.ancestors.last(), Some(AnyNodeRef::ExprCall(call)) if call.func.range() == attribute.range());
        let qualified = || {
            super::typing::resolve(self.context, &attribute.value, &[])
                .map(|receiver| join(&receiver, attribute.attr.as_str()))
        };
        let receiver = dotted(&attribute.value);
        let Some(entry) = entries.iter().find(|entry| {
            (!entry.calls_only || called)
                && entry
                    .attribute
                    .as_ref()
                    .is_none_or(|pattern| pattern.is_match(attribute.attr.as_str()))
                && entry.receiver.as_ref().is_none_or(|pattern| {
                    receiver
                        .as_ref()
                        .is_some_and(|receiver| pattern.is_match(receiver))
                })
                && entry.qualified_prefix.as_ref().is_none_or(|prefix| {
                    // Report only the innermost access, not every longer chain such as
                    // `os.environ.get` that extends an already reported `os.environ`.
                    qualified().is_some_and(|qualified| boundary(&qualified, prefix))
                        && !matches!(attribute.value.as_ref(), Expr::Attribute(_)
                            if super::typing::resolve(self.context, &attribute.value, &[])
                                .is_some_and(|value| boundary(&value, prefix)))
                })
                && !(entry.ignore_writes && self.is_write_target(attribute))
                && entry.scope.allows(self.path.as_deref())
        }) else {
            return;
        };
        let display = receiver.map_or_else(
            || attribute.attr.to_string(),
            |receiver| join(&receiver, attribute.attr.as_str()),
        );
        let message = entry
            .advice
            .render(|| format!("Do not use forbidden attribute '{display}'."));
        self.findings.push(Finding::new(
            "forbidden-attribute",
            attribute.range(),
            message,
        ));
    }
    fn is_write_target(&self, attribute: &ast::ExprAttribute) -> bool {
        let mut current = attribute.range();
        let mut context = attribute.ctx;
        // `request.state.user = value` writes through `request.state`.
        for parent in self.ancestors.iter().rev() {
            match parent {
                AnyNodeRef::ExprAttribute(parent) if parent.value.range() == current => {
                    current = parent.range();
                    context = parent.ctx;
                }
                AnyNodeRef::ExprSubscript(parent) if parent.value.range() == current => {
                    current = parent.range();
                    context = parent.ctx;
                }
                _ => break,
            }
        }
        matches!(context, ast::ExprContext::Store | ast::ExprContext::Del)
    }
    fn raise(&mut self, statement: &ast::StmtRaise) {
        let Policy::Raises(entries) = self.policy else {
            return;
        };
        let Some(exception) = &statement.exc else {
            return;
        };
        let exception = match exception.as_ref() {
            Expr::Call(call) => call.func.as_ref(),
            exception => exception,
        };
        let qualified = super::typing::resolve(self.context, exception, &[]);
        let plain = dotted(exception).filter(|name| {
            (qualified.is_none() || !name.contains('.'))
                && !bindings::is_local(self.context, exception)
        });
        let Some(entry) = entries.iter().find(|entry| {
            qualified
                .iter()
                .chain(&plain)
                .any(|name| entry.exception.is_match(name))
                && entry.scope.allows(self.path.as_deref())
                && entry
                    .filter
                    .allows(self.context, &self.ancestors, statement.range())
        }) else {
            return;
        };
        let display = qualified
            .as_ref()
            .or(plain.as_ref())
            .map_or("", String::as_str);
        let message = entry
            .advice
            .render(|| format!("Do not raise forbidden exception '{display}'."));
        self.findings
            .push(Finding::new("forbidden-raise", exception.range(), message));
    }
    fn function_lines(&mut self, function: &ast::StmtFunctionDef) {
        if !matches!(self.policy, Policy::Lines(_)) {
            return;
        }
        let method = self
            .ancestors
            .iter()
            .rev()
            .find_map(|node| match node {
                AnyNodeRef::StmtClassDef(_) => Some(true),
                AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::ExprLambda(_) => Some(false),
                _ => None,
            })
            .unwrap_or(false);
        let nested = !method
            && self
                .ancestors
                .iter()
                .any(|node| matches!(node, AnyNodeRef::StmtFunctionDef(_)));
        let limit = if method {
            self.limits.method
        } else {
            self.limits.function
        };
        if nested || limit == 0 {
            return;
        }
        let start = function
            .decorator_list
            .first()
            .map_or(function.start(), Ranged::start);
        let count = self.context.source[TextRange::new(start, function.end())]
            .universal_newlines()
            .count();
        if count > limit {
            let kind = if method { "Method" } else { "Function" };
            self.findings.push(Finding::new(
                "line-count-limit",
                function.name.range(),
                format!(
                    "{kind} '{}' has {count} lines, exceeding the configured limit of {limit}.",
                    function.name
                ),
            ));
        }
    }
}
#[derive(Clone, Copy)]
enum CallViolation<'entry> {
    Call,
    MissingKeyword(&'entry str),
    ForbiddenKeyword(&'entry str),
}

fn dotted(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Name(name) => Some(name.id.to_string()),
        Expr::Attribute(attribute) => {
            Some(join(&dotted(&attribute.value)?, attribute.attr.as_str()))
        }
        _ => None,
    }
}
impl<'ast> SourceOrderVisitor<'ast> for PolicyVisitor<'_, '_, 'ast, '_> {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        match node {
            AnyNodeRef::StmtFunctionDef(function) => {
                self.report_name(function.name.as_str(), function.name.range(), "function");
                self.function_lines(function);
            }
            AnyNodeRef::StmtClassDef(class) => {
                self.report_name(class.name.as_str(), class.name.range(), "class");
            }
            AnyNodeRef::Parameter(parameter) => {
                self.report_name(parameter.name.as_str(), parameter.name.range(), "parameter");
            }
            AnyNodeRef::ExprName(name) if name.ctx == ast::ExprContext::Store => {
                self.report_name(name.id.as_str(), name.range(), "variable");
            }
            AnyNodeRef::ExprAttribute(attribute) => {
                self.report_name(attribute.attr.as_str(), attribute.attr.range(), "attribute");
                self.attribute(attribute);
            }
            AnyNodeRef::StmtRaise(statement) => self.raise(statement),
            AnyNodeRef::PatternMatchAs(pattern) => {
                if let Some(name) = &pattern.name {
                    self.report_name(name.as_str(), name.range(), "variable");
                }
            }
            AnyNodeRef::PatternMatchStar(pattern) => {
                if let Some(name) = &pattern.name {
                    self.report_name(name.as_str(), name.range(), "variable");
                }
            }
            AnyNodeRef::PatternMatchMapping(pattern) => {
                if let Some(name) = &pattern.rest {
                    self.report_name(name.as_str(), name.range(), "variable");
                }
            }
            AnyNodeRef::ExceptHandlerExceptHandler(handler) => {
                if let Some(name) = &handler.name {
                    self.report_name(name.as_str(), name.range(), "variable");
                }
            }
            AnyNodeRef::StmtImport(statement) => {
                for alias in &statement.names {
                    self.import(alias, alias.name.as_str(), None, 0);
                }
            }
            AnyNodeRef::StmtImportFrom(statement) => {
                if statement.level > 0 && matches!(self.policy, Policy::Relative) {
                    self.findings.push(Finding::new(
                        "no-relative-imports",
                        statement.range(),
                        "Use absolute imports instead of relative imports.",
                    ));
                }
                if let Some(module) = &statement.module {
                    self.dotted_import_names(module);
                }
                let module = statement
                    .module
                    .as_ref()
                    .map_or("", ast::Identifier::as_str);
                for alias in &statement.names {
                    self.import(alias, module, Some(alias.name.as_str()), statement.level);
                }
            }
            AnyNodeRef::Alias(alias) => {
                self.dotted_import_names(&alias.name);
                if let Some(name) = &alias.asname {
                    self.report_name(name.as_str(), name.range(), "alias");
                    if name.as_str().starts_with('_') && matches!(self.policy, Policy::Underscore) {
                        self.findings.push(Finding::new(
                            "no-underscore-import-aliases",
                            name.range(),
                            "Import aliases must not start with an underscore.",
                        ));
                    }
                }
            }
            AnyNodeRef::StmtIf(statement) => self.mark_type_only(statement),
            AnyNodeRef::ExprCall(call) => self.call(call),
            _ => {}
        }
        self.ancestors.push(node);
        TraversalSignal::Traverse
    }
    fn leave_node(&mut self, _node: AnyNodeRef<'ast>) {
        self.ancestors.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NameResolver;

    struct NoNames;
    impl NameResolver for NoNames {
        fn qualified_name(&self, _expression: &Expr) -> Option<String> {
            None
        }
        fn binding_range(&self, _expression: &ast::ExprName) -> Option<TextRange> {
            None
        }
    }
    fn with_context(path: &str, root: &str, check: impl FnOnce(&Context<'_, '_, '_>)) {
        let module = ast::ModModule {
            node_index: ast::AtomicNodeIndex::default(),
            range: TextRange::default(),
            body: std::iter::empty().collect(),
        };
        let context = Context {
            module: &module,
            source: "",
            path: Path::new(path),
            root: Path::new(root),
            python_version: ast::PythonVersion::PY313,
            resolver: &NoNames,
            ancestors: &[],
            rendered_metrics: None,
        };
        check(&context);
    }
    fn options(source: &str) -> RuleOptions {
        toml::from_str(source).unwrap()
    }

    #[test]
    fn line_overrides_preserve_source_order_on_ties() {
        let source = "first()\nsecond()\n";
        assert!(
            check_path(
                "line-count-limit",
                source,
                "/repo/ab.py",
                "/repo",
                "glob_limits = {'a?.py' = {max_file_lines = 1}, '?b.py' = {max_file_lines = 2}}"
            )
            .is_empty()
        );
        assert!(check_path("line-count-limit", source, "/repo/ab.py", "/repo", "per_file_limits = {'ab.py' = {max_file_lines = 1}, './ab.py' = {max_file_lines = 2}}").is_empty());
    }
    fn check_path(
        name: &str,
        source: &str,
        path: &str,
        root: &str,
        settings: &str,
    ) -> Vec<Finding> {
        let definition = RULES.iter().find(|rule| rule.name == name).unwrap();
        let rule = (definition.create)(&options(settings)).unwrap();
        let module = ruff_python_parser::parse_module(source)
            .unwrap()
            .into_syntax();
        let context = Context {
            module: &module,
            source,
            path: Path::new(path),
            root: Path::new(root),
            python_version: ast::PythonVersion::PY313,
            resolver: &NoNames,
            ancestors: &[],
            rendered_metrics: None,
        };
        let mut findings = Vec::new();
        rule.check(AnyNodeRef::ModModule(&module), &context, &mut findings);
        findings
    }

    #[test]
    fn relative_import_boundaries_respect_package_paths() {
        for (source, blocked) in [
            ("from .private import X\n", "pkg.private"),
            ("from . import private\n", "pkg.private"),
            ("from . import *\n", "pkg"),
        ] {
            assert_eq!(
                check_path(
                    "forbidden-import",
                    source,
                    "/repo/pkg/module.py",
                    "/repo",
                    &format!("forbidden_imports = ['{blocked}']")
                )
                .len(),
                1
            );
        }
        assert!(
            check_path(
                "forbidden-import",
                "from .private import X\n",
                "/repo/other/module.py",
                "/repo",
                "forbidden_imports = ['pkg.private']"
            )
            .is_empty()
        );
    }

    #[test]
    fn contracts_package_initializers_and_relative_imports() {
        let reports = check_path(
            "import-contracts",
            "from . import *\n",
            "/repo/src/parcel/orders/logic.py",
            "/repo",
            "source_roots = ['src']\ncontracts = [{type = 'forbidden', name = 'leaf', source = ['parcel.orders.**'], forbid = ['parcel.orders']}]",
        );
        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].message,
            "Import contract 'leaf' forbids 'parcel.orders.logic' importing 'parcel.orders'."
        );
        let settings = "source_roots = ['src']\ncontracts = [{type = 'forbidden', name = 'leaf', source = ['parcel.orders.**'], forbid = ['parcel.billing', 'parcel.orders.router']}]";
        for (source, path) in [
            ("from ..billing import storage\n", "logic.py"),
            ("from . import router\n", "__init__.py"),
            ("from .router import *\n", "__init__.py"),
        ] {
            assert_eq!(
                check_path(
                    "import-contracts",
                    source,
                    &format!("/repo/src/parcel/orders/{path}"),
                    "/repo",
                    settings
                )
                .len(),
                1
            );
        }
        assert!(
            check_path(
                "import-contracts",
                "from ...billing import storage\n",
                "/repo/src/parcel/orders/logic.py",
                "/repo",
                settings
            )
            .is_empty()
        );
    }

    #[test]
    fn root_identity_and_relative_resolution_inputs() {
        with_context(
            "/policy-repo/apps/server/src/parcel/orders/logic.py",
            "/policy-repo",
            |context| {
                assert_eq!(
                    module_identity(context, &[".".into(), "apps/server/src".into()]),
                    Some(("parcel.orders.logic".into(), "parcel.orders".into()))
                );
            },
        );
        with_context(
            "/policy-repo/src/parcel/orders/__init__.py",
            "/policy-repo",
            |context| {
                assert_eq!(
                    module_identity(context, &["src".into()]),
                    Some(("parcel.orders".into(), "parcel.orders".into()))
                );
            },
        );
        with_context(
            "/policy-repo/vendor/parcel/orders/logic.py",
            "/policy-repo",
            |context| {
                assert_eq!(module_identity(context, &["src".into()]), None);
            },
        );
        with_context(
            "/policy-repo/src/not-a-module/logic.py",
            "/policy-repo",
            |context| {
                assert_eq!(module_identity(context, &["src".into()]), None);
            },
        );
    }

    #[test]
    fn path_limits_specificity_absolute_paths_and_repo_relative_matching() {
        let configured = options(
            "max_function_lines = 30\nglob_limits = {'src/**/b.py' = {max_function_lines = 10}, 'src/a/b.py' = {max_function_lines = 2}}\nper_file_limits = {'src/a/b.py' = {max_method_lines = 7}}",
        );
        let raw: RawLineOptions = parse_options(&configured).unwrap();
        assert_eq!(raw.max_function_lines, 30);
        let rule = line_count_limit(&configured).unwrap();
        // Inspect behavior through findings instead of reaching into the trait object.
        let source = "def f():\n    first()\n    second()\n";
        let function = ruff_python_parser::parse_module(source)
            .unwrap()
            .into_syntax();
        let context = Context {
            module: &function,
            source,
            path: Path::new("/policy-repo/src/a/b.py"),
            root: Path::new("/policy-repo"),
            python_version: ast::PythonVersion::PY313,
            resolver: &NoNames,
            ancestors: &[],
            rendered_metrics: None,
        };
        let mut findings = Vec::new();
        rule.check(AnyNodeRef::ModModule(&function), &context, &mut findings);
        assert_eq!(findings.len(), 1);
        let outside = Context {
            path: Path::new("/policy-repo/vendor/src/a/b.py"),
            ..context
        };
        findings.clear();
        rule.check(AnyNodeRef::ModModule(&function), &outside, &mut findings);
        assert!(findings.is_empty());
        let absolute_rule = line_count_limit(&options(
            "per_file_limits = {'/policy-repo/src/a/b.py' = {max_function_lines = 1}}",
        ))
        .unwrap();
        let direct = Context {
            path: Path::new("/policy-repo/src/a/b.py"),
            ..outside
        };
        absolute_rule.check(AnyNodeRef::ModModule(&function), &direct, &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn validates_all_policy_settings() {
        for source in [
            "source_roots = []",
            "source_roots = ['/absolute']",
            "source_roots = ['../outside']",
            "contracts = [{type = 'forbidden', name = '', source = ['x'], forbid = ['y']}]",
            "contracts = [{type = 'forbidden', name = 'x', source = [], forbid = ['y']}]",
            "contracts = [{type = 'forbidden', name = 'x', source = ['x*'], forbid = ['y']}]",
            "contracts = [{type = 'layers', name = 'x', layers = [{modules = ['x']}]}]",
            "contracts = [{type = 'independence', name = 'x', packages = ['a.**']}]",
            "contracts = [{type = 'independence', name = 'x', packages = ['a.x', 'b.y']}]",
        ] {
            assert!(import_contracts(&options(source)).is_err(), "{source}");
        }
        assert!(
            forbidden_call(&options("forbidden_calls = ['os.remove||not a callable']")).is_err()
        );
        assert!(forbidden_import(&options("forbidden_imports = ['bad..module']")).is_err());
        assert!(forbidden_name(&options("forbidden_names = ['unknown:thing']")).is_err());
        for source in [
            "max_file_lines = -1",
            "max_method_lines = true",
            "glob_limits = {'' = {max_file_lines = 1}}",
            "per_file_limits = {'file.py' = {}}",
            "glob_limits = {'*' = {unknown = 1}}",
        ] {
            assert!(line_count_limit(&options(source)).is_err(), "{source}");
        }
        assert!(tempfile(&options("unknown = []")).is_err());
        assert!(no_relative_imports(&options("unknown = true")).is_err());
        assert!(no_underscore_import_aliases(&options("unknown = true")).is_err());
    }

    #[test]
    fn source_roots_reject_rooted_and_drive_relative_paths() {
        for root in [
            "/absolute",
            r"\rooted",
            "C:relative",
            r"C:\absolute",
            "c:/absolute",
            "src/../../outside",
            r"src\..\..\outside",
        ] {
            let source = format!("source_roots = [{root:?}]");
            assert!(import_contracts(&options(&source)).is_err(), "{root}");
        }
        for root in [
            ".",
            "src",
            "apps/server/src",
            r"apps\server\src",
            "lib:extra",
        ] {
            let source = format!("source_roots = [{root:?}]");
            assert!(import_contracts(&options(&source)).is_ok(), "{root}");
        }
    }

    #[test]
    fn python_shell_patterns_and_splitlines() {
        for (pattern, name, expected) in [
            ("src/*.py", "src/a/b.py", true),
            ("[!a-z]*", "Blocked", true),
            ("[z-a]", "z", false),
            ("[[]", "[", true),
            ("[]]", "]", true),
            ("*Manager", "UserManager", true),
            ("thing[", "thing[", true),
        ] {
            assert_eq!(shell_pattern(pattern).unwrap().is_match(name), expected);
        }
        assert_eq!(splitline_count(""), 0);
        assert_eq!(splitline_count("a\r\nb\rc\n"), 3);
        assert_eq!(splitline_count("\n\n"), 2);
    }

    #[test]
    fn forbidden_call_recommendations_never_supply_fixes() {
        let definition = RULES
            .iter()
            .find(|rule| rule.name == "forbidden-call")
            .unwrap();
        assert!(!definition.fixable);
        let findings = check_path(
            "forbidden-call",
            "import legacy\nimport modern\nlegacy.run('x')\n",
            "/repo/module.py",
            "/repo",
            "forbidden_calls = ['legacy.run||modern.run']",
        );
        assert_eq!(findings.len(), 1);
        assert!(findings[0].fix.is_none());
        assert!(findings[0].fix_title.is_none());
        assert!(findings[0].message.ends_with(" Use instead: modern.run."));
    }

    #[test]
    fn unicode_import_ranges_use_complete_python_tokens() {
        for name in ["a\u{203f}b", "a\u{1dc0}b"] {
            let source = format!("import {name}\n");
            let findings = check_path(
                "forbidden-name",
                &source,
                "/repo/module.py",
                "/repo",
                "forbidden_names = ['import:*']",
            );
            assert_eq!(findings.len(), 1);
            assert_eq!(&source[findings[0].range], name);
        }
    }

    #[test]
    fn line_limit_ranges_and_counts_handle_all_python_newlines() {
        for newline in ["\n", "\r\n", "\r"] {
            let source = format!("def f():{newline}    first(){newline}    second(){newline}");
            let findings = check_path(
                "line-count-limit",
                &source,
                "/repo/module.py",
                "/repo",
                "max_function_lines = 2\nmax_file_lines = 2",
            );
            assert_eq!(findings.len(), 2);
            assert_eq!(&source[findings[0].range], format!("def f():{newline}"));
            assert_eq!(
                findings[1].message,
                "Function 'f' has 3 lines, exceeding the configured limit of 2."
            );
        }
        let source = "first(); second()";
        // Also check the range primitive used when the first line has no terminator.
        assert_eq!(&source[source.full_line_range(TextSize::ZERO)], source);
    }

    fn messages(name: &str, source: &str, path: &str, settings: &str) -> Vec<String> {
        check_path(name, source, path, "/repo", settings)
            .into_iter()
            .map(|finding| finding.message)
            .collect()
    }
    fn create(name: &str, settings: &str) -> Result<Box<dyn LintRule>> {
        let definition = RULES.iter().find(|rule| rule.name == name).unwrap();
        (definition.create)(&options(settings))
    }
    fn error(name: &str, settings: &str) -> String {
        format!("{:#}", create(name, settings).unwrap_err())
    }

    #[test]
    fn structured_entries_match_their_shorthand() {
        let source = "import legacy\nlegacy.run('x')\n";
        let expected = vec!["Avoid it. Use instead: modern.run.".to_owned()];
        for settings in [
            "forbidden_calls = ['legacy.run|Avoid it.|modern.run']",
            "forbidden_calls = [{symbol = 'legacy.run', message = 'Avoid it.', replacement = 'modern.run'}]",
            "[[forbidden_calls]]\nsymbol = 'legacy.run'\nmessage = 'Avoid it.'\nreplacement = 'modern.run'",
        ] {
            assert_eq!(
                messages("forbidden-call", source, "/repo/a.py", settings),
                expected,
                "{settings}"
            );
        }
        assert_eq!(
            messages(
                "forbidden-import",
                "import legacy.core\n",
                "/repo/a.py",
                "forbidden_imports = ['other', {module = 'legacy', replacement = 'modern'}]"
            ),
            ["Do not import across forbidden boundary 'legacy'. Use instead: modern."]
        );
        assert_eq!(
            messages(
                "forbidden-name",
                "class UserManager: ...\n",
                "/repo/a.py",
                "forbidden_names = [{kind = 'class', pattern = '*Manager', message = 'Name the role.'}]"
            ),
            ["Name the role."]
        );
    }

    #[test]
    fn later_entries_replace_earlier_entries_with_the_same_selector() {
        let source = "import legacy\nlegacy.run('x')\n";
        assert_eq!(
            messages(
                "forbidden-call",
                source,
                "/repo/a.py",
                "forbidden_calls = ['legacy.run|first', {symbol = 'legacy.run', message = 'second'}]"
            ),
            ["second"]
        );
        // A scoped entry selects different files, so it does not replace the global entry.
        assert_eq!(
            messages(
                "forbidden-call",
                source,
                "/repo/a.py",
                "forbidden_calls = ['legacy.run|global', {symbol = 'legacy.run', message = 'scoped', include = ['src']}]"
            ),
            ["global"]
        );
    }

    #[test]
    fn include_and_exclude_globs_are_relative_to_the_config_root() {
        let source = "import legacy\nlegacy.run('x')\n";
        let scoped = |scope: &str, path: &str| {
            !check_path(
                "forbidden-call",
                source,
                path,
                "/repo",
                &format!("forbidden_calls = [{{symbol = 'legacy.run', {scope}}}]"),
            )
            .is_empty()
        };
        for (scope, path, expected) in [
            ("include = ['src/**']", "/repo/src/pkg/deep/a.py", true),
            ("include = ['src/**']", "/repo/tests/a.py", false),
            ("include = ['src/**']", "/repo/vendor/src/a.py", false),
            ("include = ['src/*.py']", "/repo/src/a.py", true),
            ("include = ['src/*.py']", "/repo/src/pkg/a.py", false),
            ("include = ['src']", "/repo/src/pkg/a.py", true),
            ("include = ['./src/']", "/repo/src/a.py", true),
            (
                "include = ['**/routers/**']",
                "/repo/app/routers/items.py",
                true,
            ),
            (
                "include = ['**/*_router.py']",
                "/repo/app/items_router.py",
                true,
            ),
            ("include = ['**/*_router.py']", "/repo/app/items.py", false),
            ("include = ['src/**']", "/repo/./src/../src/a.py", true),
            ("exclude = ['tests']", "/repo/tests/unit/a.py", false),
            ("exclude = ['tests']", "/repo/src/tests_helper.py", true),
            (
                "include = ['src'], exclude = ['src/generated']",
                "/repo/src/generated/a.py",
                false,
            ),
            (
                "include = ['src'], exclude = ['src/generated']",
                "/repo/src/a.py",
                true,
            ),
            ("include = ['src']", "/elsewhere/src/a.py", false),
            ("exclude = ['src']", "/elsewhere/src/a.py", true),
        ] {
            assert_eq!(scoped(scope, path), expected, "{scope} {path}");
        }
    }

    #[test]
    fn every_policy_entry_honors_path_scopes() {
        for (rule, source, settings) in [
            (
                "forbidden-import",
                "import legacy\n",
                "forbidden_imports = [{module = 'legacy', exclude = ['tests']}]",
            ),
            (
                "forbidden-name",
                "temp = 1\n",
                "forbidden_names = [{kind = 'variable', pattern = 'temp', exclude = ['tests']}]",
            ),
            (
                "forbidden-attribute",
                "model.dict()\n",
                "forbidden_attributes = [{attribute = 'dict', exclude = ['tests']}]",
            ),
            (
                "forbidden-raise",
                "raise Blocked()\n",
                "forbidden_raises = [{exception = 'Blocked', exclude = ['tests']}]",
            ),
        ] {
            assert_eq!(
                check_path(rule, source, "/repo/src/a.py", "/repo", settings).len(),
                1,
                "{rule}"
            );
            assert!(
                check_path(rule, source, "/repo/tests/a.py", "/repo", settings).is_empty(),
                "{rule}"
            );
        }
    }

    #[test]
    fn async_filters_use_the_innermost_executing_scope() {
        let source = "\
import legacy
legacy.run()
async def handler(value=legacy.run()):
    legacy.run()
    callback = lambda: legacy.run()
    class Nested:
        legacy.run()
def sync():
    legacy.run()
";
        let lines = |in_async: bool| -> Vec<usize> {
            check_path(
                "forbidden-call",
                source,
                "/repo/a.py",
                "/repo",
                &format!("forbidden_calls = [{{symbol = 'legacy.run', in_async = {in_async}}}]"),
            )
            .iter()
            .map(|finding| {
                source[..finding.range.start().to_usize()]
                    .matches('\n')
                    .count()
                    + 1
            })
            .collect()
        };
        assert_eq!(lines(true), [4]);
        assert_eq!(lines(false), [2, 3, 5, 7, 9]);
    }

    #[test]
    fn decorator_filters_match_enclosing_definitions() {
        let source = "\
import legacy
@router.get('/items', response=legacy.run())
def read():
    legacy.run()
    def inner():
        legacy.run()
@items_router.post('/items')
def write():
    legacy.run()
@service
class Handler:
    def method(self):
        legacy.run()
def plain():
    legacy.run()
";
        let lines = |patterns: &str| -> Vec<usize> {
            check_path(
                "forbidden-call",
                source,
                "/repo/a.py",
                "/repo",
                &format!("forbidden_calls = [{{symbol = 'legacy.run', decorators = {patterns}}}]"),
            )
            .iter()
            .map(|finding| {
                source[..finding.range.start().to_usize()]
                    .matches('\n')
                    .count()
                    + 1
            })
            .collect()
        };
        assert_eq!(lines("['router.*']"), [4, 6]);
        assert_eq!(lines("['router.get', '*_router.*']"), [4, 6, 9]);
        assert_eq!(lines("['service']"), [13]);
        assert_eq!(lines("['missing']"), Vec::<usize>::new());
    }

    #[test]
    fn keyword_requirements_and_bans() {
        let settings = "forbidden_calls = [{symbol = 'subprocess.run', require_keywords = ['timeout', 'check'], forbid_keywords = ['shell']}]";
        let check = |call: &str| {
            messages(
                "forbidden-call",
                &format!("import subprocess\n{call}\n"),
                "/repo/a.py",
                settings,
            )
        };
        assert_eq!(
            check("subprocess.run(cmd, check=True)"),
            ["Call to 'subprocess.run' must pass keyword argument 'timeout'."]
        );
        assert_eq!(
            check("subprocess.run(cmd, timeout=1, check=True, shell=True)"),
            ["Call to 'subprocess.run' must not pass keyword argument 'shell'."]
        );
        assert_eq!(
            check("subprocess.run(cmd, shell=True, **options)"),
            ["Call to 'subprocess.run' must not pass keyword argument 'shell'."]
        );
        assert!(check("subprocess.run(cmd, timeout=1, check=True)").is_empty());
        assert!(check("subprocess.run(cmd, **options)").is_empty());
        let custom = messages(
            "forbidden-call",
            "import subprocess\nsubprocess.run(cmd)\n",
            "/repo/a.py",
            "forbidden_calls = [{symbol = 'subprocess.run', require_keywords = ['timeout'], message = 'Bound the call.'}]",
        );
        assert_eq!(custom, ["Bound the call."]);
    }

    #[test]
    fn import_name_filters_ban_selected_names() {
        let settings = "forbidden_imports = [{module = 'app.service', names = ['service', '*_client'], message = 'Inject it.'}]";
        for source in [
            "from app.service import service\n",
            "from app.service import service as shared\n",
            "from app.service import http_client\n",
            "from app.service import *\n",
        ] {
            assert_eq!(
                messages("forbidden-import", source, "/repo/a.py", settings),
                ["Inject it."],
                "{source}"
            );
        }
        for source in [
            "from app.service import ServiceDep\n",
            "import app.service\n",
            "from app.service.sub import service\n",
            "from app import service\n",
        ] {
            assert!(
                messages("forbidden-import", source, "/repo/a.py", settings).is_empty(),
                "{source}"
            );
        }
        assert_eq!(
            messages(
                "forbidden-import",
                "from .service import service\n",
                "/repo/app/router.py",
                "forbidden_imports = [{module = 'app.service', names = ['service']}]"
            ),
            ["Do not import forbidden name 'service' from 'app.service'."]
        );
    }

    #[test]
    fn forbidden_attributes_match_names_receivers_and_calls() {
        let source = "\
payload = model.dict()
fields = model.dict
request.state.user
request.state.user = 1
del request.state
other.state.user
self.request.state
";
        let lines = |settings: &str| -> Vec<usize> {
            check_path(
                "forbidden-attribute",
                source,
                "/repo/a.py",
                "/repo",
                settings,
            )
            .iter()
            .map(|finding| {
                source[..finding.range.start().to_usize()]
                    .matches('\n')
                    .count()
                    + 1
            })
            .collect()
        };
        assert_eq!(lines("forbidden_attributes = ['*.dict']"), [1, 2]);
        assert_eq!(
            lines("forbidden_attributes = [{attribute = 'dict', calls_only = true}]"),
            [1]
        );
        assert_eq!(lines("forbidden_attributes = ['request.state']"), [3, 4, 5]);
        assert_eq!(
            lines(
                "forbidden_attributes = [{receiver = 'request', attribute = 'state', ignore_writes = true}]"
            ),
            [3]
        );
        assert_eq!(
            lines("forbidden_attributes = ['*request.state']"),
            [3, 4, 5, 7]
        );
        assert_eq!(lines("forbidden_attributes = ['state']"), [3, 4, 5, 6, 7]);
        let reports = check_path(
            "forbidden-attribute",
            "model.dict()\n",
            "/repo/a.py",
            "/repo",
            "forbidden_attributes = [{attribute = 'dict', replacement = 'model_dump'}]",
        );
        assert_eq!(
            reports[0].message,
            "Do not use forbidden attribute 'model.dict'. Use instead: model_dump."
        );
        assert_eq!(&"model.dict()\n"[reports[0].range], "model.dict");
    }

    #[test]
    fn forbidden_raises_match_direct_and_instantiated_exceptions() {
        let source = "\
raise Blocked
raise Blocked('x') from error
raise errors.Blocked(1)
raise Allowed()
raise
";
        let lines = |settings: &str| -> Vec<usize> {
            check_path("forbidden-raise", source, "/repo/a.py", "/repo", settings)
                .iter()
                .map(|finding| {
                    source[..finding.range.start().to_usize()]
                        .matches('\n')
                        .count()
                        + 1
                })
                .collect()
        };
        assert_eq!(lines("forbidden_raises = ['Blocked']"), [1, 2]);
        assert_eq!(lines("forbidden_raises = ['*Blocked']"), [1, 2, 3]);
        assert_eq!(lines("forbidden_raises = ['errors.Blocked']"), [3]);
        let reports = check_path(
            "forbidden-raise",
            "raise Blocked('x')\n",
            "/repo/a.py",
            "/repo",
            "forbidden_raises = ['Blocked']",
        );
        assert_eq!(
            reports[0].message,
            "Do not raise forbidden exception 'Blocked'."
        );
        assert_eq!(&"raise Blocked('x')\n"[reports[0].range], "Blocked");
        let filtered = "\
async def handler():
    raise Blocked()
@router.get('/')
def read():
    raise Blocked()
";
        for (settings, line) in [
            ("in_async = true", 2),
            ("in_async = false", 5),
            ("decorators = ['router.get']", 5),
        ] {
            let reports = check_path(
                "forbidden-raise",
                filtered,
                "/repo/a.py",
                "/repo",
                &format!("forbidden_raises = [{{exception = 'Blocked', {settings}}}]"),
            );
            assert_eq!(reports.len(), 1, "{settings}");
            assert_eq!(
                filtered[..reports[0].range.start().to_usize()]
                    .matches('\n')
                    .count()
                    + 1,
                line,
                "{settings}"
            );
        }
    }

    #[test]
    fn empty_policy_settings_are_inactive() {
        for name in [
            "forbidden-call",
            "forbidden-import",
            "forbidden-name",
            "forbidden-attribute",
            "forbidden-raise",
        ] {
            assert!(!create(name, "").unwrap().is_enabled(), "{name}");
        }
        assert!(
            create("forbidden-attribute", "forbidden_attributes = ['*.dict']")
                .unwrap()
                .is_enabled()
        );
        assert!(
            create("forbidden-raise", "forbidden_raises = ['Blocked']")
                .unwrap()
                .is_enabled()
        );
    }

    #[test]
    fn invalid_structured_entries_are_rejected() {
        for (name, settings, expected) in [
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a.b', unknown = 1}]",
                "unknown field `unknown`",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{message = 'x'}]",
                "missing field `symbol`",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a.b', in_async = 'yes'}]",
                "invalid type",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [1]",
                "a shorthand string or a table",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a b'}]",
                "invalid forbidden call symbol",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', require_keywords = ['not valid']}]",
                "invalid required keyword",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', forbid_keywords = ['']}]",
                "invalid forbidden keyword",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', decorators = ['a b']}]",
                "invalid name pattern",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', include = ['/abs/**']}]",
                "relative to the configuration root",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', exclude = ['../x']}]",
                "within the configuration root",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', include = ['./']}]",
                "must name a path",
            ),
            (
                "forbidden-call",
                "forbidden_calls = [{symbol = 'a', include = ['src/[']}]",
                "error",
            ),
            (
                "forbidden-import",
                "forbidden_imports = [{module = 'a', names = ['a.b']}]",
                "invalid identifier pattern",
            ),
            (
                "forbidden-import",
                "forbidden_imports = [{module = 'a', symbol = 'b'}]",
                "unknown field `symbol`",
            ),
            (
                "forbidden-name",
                "forbidden_names = [{kind = 'module', pattern = 'x'}]",
                "invalid forbidden name entry",
            ),
            (
                "forbidden-name",
                "forbidden_names = ['variable']",
                "invalid forbidden name entry",
            ),
            (
                "forbidden-attribute",
                "forbidden_attributes = [{receiver = 'request'}]",
                "need `attribute` or `qualified_prefix`",
            ),
            (
                "forbidden-attribute",
                "forbidden_attributes = [{qualified_prefix = 'os.*'}]",
                "invalid qualified prefix",
            ),
            (
                "forbidden-attribute",
                "forbidden_attributes = [{attribute = 'x', calls = true}]",
                "unknown field `calls`",
            ),
            (
                "forbidden-attribute",
                "unknown = []",
                "unknown field `unknown`",
            ),
            (
                "forbidden-raise",
                "forbidden_raises = [{exception = 'A', in_async = true, extra = 1}]",
                "unknown field `extra`",
            ),
            (
                "forbidden-raise",
                "forbidden_raises = ['']",
                "invalid name pattern",
            ),
            (
                "forbidden-raise",
                "forbidden_raises = [{exception = 'A', replacement = 'not valid'}]",
                "invalid replacement",
            ),
        ] {
            let message = error(name, settings);
            assert!(message.contains(expected), "{settings}: {message}");
        }
    }
}
