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
    Calls(Vec<Entry>),
    Imports(Vec<Entry>),
    Names(Vec<NameEntry>),
    Contracts(ContractOptions),
    Lines(LineOptions),
    Relative,
    Underscore,
    Tempfile(TempOptions),
}

#[derive(Debug)]
struct Entry {
    symbol: String,
    message: Option<String>,
    replacement: Option<String>,
}

#[derive(Debug)]
struct NameEntry {
    kind: String,
    pattern: String,
    matcher: Regex,
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CallOptions {
    forbidden_calls: Vec<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ImportOptions {
    forbidden_imports: Vec<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NameOptions {
    forbidden_names: Vec<String>,
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

fn optional(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn entries(values: Vec<String>, count: usize, ascii: bool) -> Result<Vec<Entry>> {
    let ascii_symbol = Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*\z")?;
    let mut entries: Vec<Entry> = Vec::new();
    for value in values {
        let fields: Vec<_> = value.splitn(count, '|').collect();
        let symbol = fields[0];
        let replacement = fields.get(2).and_then(|text| optional(text));
        if !(if ascii {
            ascii_symbol.is_match(symbol)
        } else {
            module_identifier(symbol)
        }) || replacement
            .as_ref()
            .is_some_and(|value| !ascii_symbol.is_match(value))
        {
            bail!("invalid policy entry: {value:?}");
        }
        let entry = Entry {
            symbol: symbol.to_owned(),
            message: fields.get(1).and_then(|text| optional(text)),
            replacement,
        };
        if let Some(previous) = entries.iter_mut().find(|entry| entry.symbol == symbol) {
            *previous = entry;
        } else {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn forbidden_call(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: CallOptions = parse_options(options)?;
    Ok(Box::new(Policy::Calls(entries(
        options.forbidden_calls,
        3,
        true,
    )?)))
}
fn forbidden_import(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: ImportOptions = parse_options(options)?;
    let mut entries = entries(options.forbidden_imports, 2, false)?;
    entries.sort_by_key(|entry| {
        std::cmp::Reverse((
            entry.symbol.matches('.').count(),
            entry.symbol.chars().count(),
        ))
    });
    Ok(Box::new(Policy::Imports(entries)))
}
fn forbidden_name(options: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: NameOptions = parse_options(options)?;
    let syntax = Regex::new(
        r"\A(any|variable|parameter|function|class|attribute|import|alias):([A-Za-z_*?\[\]!][A-Za-z0-9_*?\[\]!-]*)\z",
    )?;
    let mut entries: Vec<NameEntry> = Vec::new();
    for value in options.forbidden_names {
        let (rule, message) = value
            .split_once('|')
            .map_or((value.as_str(), None), |(rule, message)| {
                (rule, optional(message))
            });
        let Some(captures) = syntax.captures(rule) else {
            bail!("invalid forbidden name entry: {value:?}");
        };
        let kind = captures[1].to_owned();
        let pattern = captures[2].to_owned();
        if let Some(previous) = entries
            .iter_mut()
            .find(|entry| entry.kind == kind && entry.pattern == pattern)
        {
            previous.message = message;
        } else {
            entries.push(NameEntry {
                kind,
                matcher: shell_pattern(&pattern)?,
                pattern,
                message,
            });
        }
    }
    Ok(Box::new(Policy::Names(entries)))
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
        || options.source_roots.iter().any(|root| {
            Path::new(root).is_absolute()
                || Path::new(root)
                    .components()
                    .any(|component| component == Component::ParentDir)
        })
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
    max_file_lines: usize,
    max_function_lines: usize,
    max_method_lines: usize,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitOverride {
    max_file_lines: Option<usize>,
    max_function_lines: Option<usize>,
    max_method_lines: Option<usize>,
}
impl Limits {
    fn apply(&mut self, other: &LimitOverride) {
        if let Some(value) = other.max_file_lines {
            self.max_file_lines = value;
        }
        if let Some(value) = other.max_function_lines {
            self.max_function_lines = value;
        }
        if let Some(value) = other.max_method_lines {
            self.max_method_lines = value;
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
            || (limits.max_file_lines.is_none()
                && limits.max_function_lines.is_none()
                && limits.max_method_lines.is_none())
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
            max_file_lines: raw.max_file_lines,
            max_function_lines: raw.max_function_lines,
            max_method_lines: raw.max_method_lines,
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

impl LintRule for Policy {
    fn is_enabled(&self) -> bool {
        match self {
            Self::Calls(entries) | Self::Imports(entries) => !entries.is_empty(),
            Self::Names(entries) => !entries.is_empty(),
            Self::Contracts(options) => !options.contracts.is_empty(),
            Self::Lines(options) => {
                options.base.max_file_lines > 0
                    || options.base.max_function_lines > 0
                    || options.base.max_method_lines > 0
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
            if limits.max_file_lines > 0 && count > limits.max_file_lines {
                findings.push(Finding::new(
                    "line-count-limit",
                    context.source.full_line_range(TextSize::ZERO),
                    format!(
                        "File has {count} lines, exceeding the configured limit of {}.",
                        limits.max_file_lines
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
}
impl PolicyVisitor<'_, '_, '_, '_> {
    fn report_name(&mut self, name: &str, range: TextRange, kind: &str) {
        let Policy::Names(entries) = self.policy else {
            return;
        };
        if let Some(entry) = entries.iter().find(|entry| {
            (entry.kind == kind || entry.kind == "any") && entry.matcher.is_match(name)
        }) {
            self.findings.push(Finding::new(
                "forbidden-name",
                range,
                entry
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("Do not use forbidden {kind} name '{name}'.")),
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
    fn import(&mut self, alias: &ast::Alias, target: &str, level: u32) {
        if let Policy::Imports(entries) = self.policy {
            let candidates = if level == 0 {
                vec![target.to_owned()]
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
                if !target.is_empty() {
                    parts.extend(target.split('.').map(str::to_owned));
                }
                (0..count).map(|index| parts[index..].join(".")).collect()
            };
            if let Some(entry) = entries.iter().find(|entry| {
                candidates
                    .iter()
                    .any(|candidate| boundary(candidate, &entry.symbol))
            }) {
                self.findings.push(Finding::new(
                    "forbidden-import",
                    alias.range(),
                    entry.message.clone().unwrap_or_else(|| {
                        format!(
                            "Do not import across forbidden boundary '{}'.",
                            entry.symbol
                        )
                    }),
                ));
            }
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
                target.to_owned()
            } else {
                let parts: Vec<_> = package.split('.').filter(|part| !part.is_empty()).collect();
                if level as usize > parts.len() {
                    return;
                }
                join(&parts[..=(parts.len() - level as usize)].join("."), target)
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
                // Canonical provenance wins over any earlier plain-name fallback.
                let entry = entries
                    .iter()
                    .find(|entry| {
                        symbol
                            .as_ref()
                            .is_some_and(|symbol| symbol == &entry.symbol)
                    })
                    .or_else(|| {
                        entries.iter().find(|entry| {
                            (!known_local
                                && dotted.as_ref().is_some_and(|name| {
                                    *name == entry.symbol
                                        && (symbol.is_none() || !name.contains('.'))
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
                        })
                    });
                if let Some(entry) = entry {
                    let mut message = entry.message.clone().unwrap_or_else(|| {
                        format!("Do not call forbidden callable '{}'.", entry.symbol)
                    });
                    if let Some(replacement) = &entry.replacement {
                        message.push_str(" Use instead: ");
                        message.push_str(replacement);
                        message.push('.');
                    }
                    // A recommendation need not accept the same arguments or preserve semantics.
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
            self.limits.max_method_lines
        } else {
            self.limits.max_function_lines
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
                self.report_name(class.name.as_str(), class.name.range(), "class")
            }
            AnyNodeRef::Parameter(parameter) => {
                self.report_name(parameter.name.as_str(), parameter.name.range(), "parameter")
            }
            AnyNodeRef::ExprName(name) if name.ctx == ast::ExprContext::Store => {
                self.report_name(name.id.as_str(), name.range(), "variable")
            }
            AnyNodeRef::ExprAttribute(attribute) => {
                self.report_name(attribute.attr.as_str(), attribute.attr.range(), "attribute")
            }
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
                    self.import(alias, alias.name.as_str(), 0);
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
                    let target = if alias.name.as_str() == "*" {
                        module.to_owned()
                    } else {
                        join(module, alias.name.as_str())
                    };
                    self.import(alias, &target, statement.level);
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
            node_index: Default::default(),
            range: TextRange::default(),
            body: Default::default(),
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
}
