use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::Path;
use std::sync::{Arc, LazyLock, OnceLock};

use anyhow::{Result, bail};
use ruff_cache::{CacheKey, CacheKeyHasher};
use ruff_diagnostics::Fix;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_node};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, ModModule, NodeKind, PythonVersion};
use ruff_python_semantic::SemanticModel;
use ruff_python_trivia::CommentRanges;
use ruff_text_size::{Ranged, TextRange};
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;

pub mod builtin;
pub mod config;
pub mod context;
pub mod suppression;

include!(concat!(env!("OUT_DIR"), "/rule_fingerprint.rs"));

pub use context::Context;
pub type RuleOptions = toml::Table;

pub trait NameResolver {
    fn qualified_name(&self, expression: &Expr) -> Option<String>;
    fn binding_range(&self, name: &ast::ExprName) -> Option<TextRange>;

    fn semantic_model(&self) -> Option<&SemanticModel<'_>> {
        None
    }
}

impl NameResolver for SemanticModel<'_> {
    fn qualified_name(&self, expression: &Expr) -> Option<String> {
        let mut head = expression;
        while let Expr::Attribute(attribute) = head {
            head = &attribute.value;
        }
        if let Expr::Name(name) = head {
            self.resolve_name(name)?;
        }
        let qualified = self.resolve_qualified_name(expression)?;
        let segments = qualified.segments();
        if segments.first().is_some_and(|head| head.is_empty()) {
            Some(
                std::iter::once("builtins")
                    .chain(segments.iter().skip(1).copied())
                    .collect::<Vec<_>>()
                    .join("."),
            )
        } else {
            Some(segments.join("."))
        }
    }

    fn binding_range(&self, name: &ast::ExprName) -> Option<TextRange> {
        self.resolve_name(name).map(|id| self.binding(id).range())
    }

    fn semantic_model(&self) -> Option<&SemanticModel<'_>> {
        Some(self)
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub rule: &'static str,
    pub range: TextRange,
    pub message: String,
    pub fix: Option<Fix>,
    pub fix_title: Option<String>,
}

impl Finding {
    pub fn new(rule: &'static str, range: TextRange, message: impl Into<String>) -> Self {
        Self {
            rule,
            range,
            message: message.into(),
            fix: None,
            fix_title: None,
        }
    }

    pub fn with_fix(mut self, fix: Fix, title: impl Into<String>) -> Self {
        self.fix = Some(fix);
        self.fix_title = Some(title.into());
        self
    }
}

pub trait LintRule:
    Debug + Send + Sync + std::panic::UnwindSafe + std::panic::RefUnwindSafe
{
    fn is_enabled(&self) -> bool {
        true
    }

    fn check(
        &self,
        node: AnyNodeRef<'_>,
        context: &Context<'_, '_, '_>,
        findings: &mut Vec<Finding>,
    );

    fn check_comment(
        &self,
        _range: TextRange,
        _context: &Context<'_, '_, '_>,
        _findings: &mut Vec<Finding>,
    ) {
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionRange {
    pub minimum: Option<PythonVersion>,
    pub before: Option<PythonVersion>,
}

impl VersionRange {
    pub fn contains(self, version: PythonVersion) -> bool {
        self.minimum.is_none_or(|minimum| version >= minimum)
            && self.before.is_none_or(|before| version < before)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RuleDefinition {
    pub name: &'static str,
    pub code: &'static str,
    pub explanation: &'static str,
    pub node_kinds: &'static [NodeKind],
    pub checks_comments: bool,
    pub fixable: bool,
    pub create: fn(&RuleOptions) -> Result<Box<dyn LintRule>>,
    pub source_file: &'static str,
    pub source_line: u32,
    pub replaces: &'static [&'static str],
    pub versions: VersionRange,
}

impl RuleDefinition {
    pub const fn replacing(mut self, codes: &'static [&'static str]) -> Self {
        self.replaces = codes;
        self
    }

    pub const fn since(mut self, version: PythonVersion) -> Self {
        self.versions.minimum = Some(version);
        self
    }

    pub const fn before(mut self, version: PythonVersion) -> Self {
        self.versions.before = Some(version);
        self
    }
}

/// Registers native Rust metadata and records its source location.
#[macro_export]
macro_rules! native_rule {
    ($($fields:tt)*) => {
        $crate::RuleDefinition {
            source_file: file!(), source_line: line!(), replaces: &[],
            versions: $crate::VersionRange { minimum: None, before: None }, $($fields)*
        }
    };
}

pub fn parse_options<T: DeserializeOwned>(options: &RuleOptions) -> Result<T> {
    Ok(toml::Value::Table(options.clone()).try_into()?)
}

pub fn reject_options(options: &RuleOptions) -> Result<()> {
    if let Some(key) = options.keys().next() {
        bail!("unknown rule option `{key}`");
    }
    Ok(())
}

#[derive(Debug)]
struct RegisteredRule {
    definition: &'static RuleDefinition,
    rule: OnceLock<Result<Box<dyn LintRule>, String>>,
}

impl RegisteredRule {
    fn instance(&self) -> std::result::Result<&dyn LintRule, &str> {
        self.rule
            .get_or_init(|| {
                (self.definition.create)(&RuleOptions::new()).map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .map(std::convert::AsRef::as_ref)
            .map_err(String::as_str)
    }
}

#[derive(Debug, Default)]
pub struct Registry {
    rules: Vec<RegisteredRule>,
    by_name: FxHashMap<&'static str, usize>,
    by_kind: FxHashMap<NodeKind, Vec<usize>>,
    comment_rules: Vec<usize>,
}

impl Registry {
    pub fn new(
        definitions: impl IntoIterator<Item = &'static RuleDefinition>,
        options: &BTreeMap<String, RuleOptions>,
    ) -> Result<Self> {
        let mut registry = Self::default();
        for definition in definitions {
            let rule = options
                .get(definition.name)
                .map(definition.create)
                .transpose()?;
            registry.register(definition, rule);
        }
        Ok(registry)
    }

    fn register(&mut self, definition: &'static RuleDefinition, rule: Option<Box<dyn LintRule>>) {
        let index = self.rules.len();
        self.by_name.insert(definition.name, index);
        for kind in definition.node_kinds {
            self.by_kind.entry(*kind).or_default().push(index);
        }
        if definition.checks_comments {
            self.comment_rules.push(index);
        }
        let rule = rule.map_or_else(OnceLock::new, |rule| OnceLock::from(Ok(rule)));
        self.rules.push(RegisteredRule { definition, rule });
    }

    pub fn active(&self, name: &str, version: PythonVersion) -> Result<bool> {
        let Some(index) = self.by_name.get(name) else {
            bail!("unregistered native rule `{name}`");
        };
        let rule = &self.rules[*index];
        if !rule.definition.versions.contains(version) {
            return Ok(false);
        }
        rule.instance()
            .map(LintRule::is_enabled)
            .map_err(|message| anyhow::anyhow!("{message}"))
    }

    pub fn check(
        &self,
        context: &Context<'_, '_, '_>,
        comments: &CommentRanges,
        enabled: &dyn Fn(&RuleDefinition) -> bool,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        let active: Vec<Option<&dyn LintRule>> = self
            .rules
            .iter()
            .map(|registered| {
                if !enabled(registered.definition)
                    || !registered
                        .definition
                        .versions
                        .contains(context.python_version)
                {
                    return None;
                }
                match registered.instance() {
                    Ok(rule) => rule.is_enabled().then_some(rule),
                    Err(error) => {
                        findings.push(Finding::new(
                            registered.definition.name,
                            context.module.range(),
                            format!("Cannot initialize native rule: {error}"),
                        ));
                        None
                    }
                }
            })
            .collect();
        if !active.iter().any(Option::is_some) {
            return findings;
        }
        let Context {
            module,
            source,
            path,
            root,
            python_version,
            resolver,
            rendered_metrics,
            ..
        } = *context;
        let mut visitor = Visitor {
            registry: self,
            module,
            source,
            path,
            root,
            python_version,
            resolver,
            active: &active,
            ancestors: Vec::new(),
            findings,
            rendered_metrics,
        };
        walk_node(&mut visitor, AnyNodeRef::ModModule(module));
        for range in comments.iter() {
            let context = Context {
                module,
                source,
                path,
                root,
                python_version,
                resolver,
                ancestors: &[],
                rendered_metrics,
            };
            for index in &self.comment_rules {
                if let Some(rule) = active[*index] {
                    rule.check_comment(*range, &context, &mut visitor.findings);
                }
            }
        }
        visitor.findings
    }
}

struct Visitor<'reg, 'ast, 'src> {
    registry: &'reg Registry,
    module: &'ast ModModule,
    source: &'src str,
    path: &'reg Path,
    root: &'reg Path,
    python_version: PythonVersion,
    resolver: &'reg dyn NameResolver,
    active: &'reg [Option<&'reg dyn LintRule>],
    ancestors: Vec<AnyNodeRef<'ast>>,
    findings: Vec<Finding>,
    rendered_metrics: Option<&'reg dyn builtin::spacing::RenderedMetrics>,
}

impl<'ast> SourceOrderVisitor<'ast> for Visitor<'_, 'ast, '_> {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        if let Some(indices) = self.registry.by_kind.get(&node.kind()) {
            let context = Context {
                module: self.module,
                source: self.source,
                path: self.path,
                root: self.root,
                python_version: self.python_version,
                resolver: self.resolver,
                ancestors: &self.ancestors,
                rendered_metrics: self.rendered_metrics,
            };
            for index in indices {
                if let Some(rule) = self.active[*index] {
                    rule.check(node, &context, &mut self.findings);
                }
            }
        }
        self.ancestors.push(node);
        TraversalSignal::Traverse
    }

    fn leave_node(&mut self, _node: AnyNodeRef<'ast>) {
        self.ancestors.pop();
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub registry: Arc<Registry>,
    pub root: std::path::PathBuf,
    fingerprint: String,
}

impl Settings {
    pub fn defaults(root: std::path::PathBuf) -> Self {
        static REGISTRY: LazyLock<Arc<Registry>> = LazyLock::new(|| {
            let mut registry = Registry::default();
            for definition in builtin::definitions() {
                registry.register(definition, None);
            }
            Arc::new(registry)
        });
        Self {
            registry: Arc::clone(&REGISTRY),
            root,
            fingerprint: String::new(),
        }
    }

    pub fn new(root: std::path::PathBuf, options: BTreeMap<String, RuleOptions>) -> Result<Self> {
        let definitions = builtin::definitions();
        for name in options.keys() {
            if !definitions.iter().any(|rule| rule.name == name) {
                bail!("unknown Tuff rule `{name}`");
            }
        }
        let fingerprint = toml::to_string(&options)?;
        let registry = Arc::new(Registry::new(definitions, &options)?);
        Ok(Self {
            registry,
            root,
            fingerprint,
        })
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::defaults(std::path::PathBuf::new())
    }
}

impl CacheKey for Settings {
    fn cache_key(&self, state: &mut CacheKeyHasher) {
        self.root.cache_key(state);
        self.fingerprint.cache_key(state);
        RULE_FINGERPRINT.cache_key(state);
        env!("CARGO_PKG_VERSION").cache_key(state);
    }
}
