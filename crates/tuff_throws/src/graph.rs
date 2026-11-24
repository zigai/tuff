//! Project-wide call graph and exception propagation.
//!
//! Each function's escaping exceptions are computed from its own `raise` statements, its
//! declared contracts, and the escaping exceptions of the functions it calls, filtered through
//! the `try`/`except` handlers and `contextlib.suppress` blocks active at each site. Strongly
//! connected components of the call graph are solved callees-first, iterating each component to
//! a fixpoint so mutual recursion converges.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use ruff_source_file::LineIndex;
use ruff_text_size::{TextRange, TextSize};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::config::{
    FileFilter, FunctionMatcher, LoadedConfig, SourceKind, ThrowsConfig, is_python_path,
};
use crate::extract::{
    CallSite, ExceptionRef, FunctionFacts, Guard, HandlerTypes, ModuleFacts, RaiseKind, Suppression,
};
use crate::hierarchy::{ExceptionHierarchy, ExceptionId, is_builtin_exception, name_matches};
use crate::ty_adapter::{CallTarget, FileId, TyProject};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionId(u32);

impl FunctionId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug)]
pub struct AnalyzedFile {
    pub path: PathBuf,
    /// Path relative to the configuration root, with `/` separators.
    pub relative_path: String,
    pub module: Option<String>,
    pub facts: ModuleFacts,
    source: String,
    line_index: LineIndex,
}

impl AnalyzedFile {
    pub fn location(&self, offset: TextSize) -> Location {
        let line_column = self.line_index.line_column(offset, &self.source);
        Location {
            path: self.relative_path.clone(),
            line: line_column.line.get(),
            column: line_column.column.get(),
        }
    }

    fn line(&self, offset: TextSize) -> usize {
        self.line_index.line_index(offset).get()
    }

    /// Returns the suppression comments on the first or last line of `range`.
    pub fn suppressions(&self, range: TextRange) -> impl Iterator<Item = &Suppression> {
        let start = self.line(range.start());
        let end = self.line(range.end());
        self.facts
            .suppressions
            .iter()
            .filter(move |suppression| suppression.line == start || suppression.line == end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Location {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.path, self.line, self.column)
    }
}

#[derive(Debug)]
pub struct FunctionNode {
    pub file: FileId,
    /// Index into the file's [`ModuleFacts::functions`].
    pub index: usize,
    /// `module.qualname`, or `path::qualname` for files outside any module.
    pub symbol: String,
    pub is_boundary: bool,
    /// Decorated with a decorator that handles every exception.
    pub is_suppressing: bool,
    /// Decorated with a decorator that may change which exceptions escape.
    has_opaque_decorator: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractSource {
    Decorator,
    Docstring,
}

/// How an exception reaches a function's callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Raise { site: usize },
    Call { site: usize, callee: FunctionId },
    Declared(ContractSource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trace {
    /// Number of calls between this function and the originating `raise`.
    pub depth: usize,
    pub via: Via,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TryOutcome {
    /// Exceptions that reach the try statement's handlers.
    pub reaching: BTreeSet<ExceptionId>,
    /// Whether the body contains calls or raises whose exceptions are not fully known.
    pub incomplete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunctionResult {
    /// Exceptions that escape the function.
    pub exceptions: BTreeMap<ExceptionId, Trace>,
    /// Whether the escaping set may be missing exceptions.
    pub incomplete: bool,
    pub tries: Vec<TryOutcome>,
}

#[derive(Debug, Clone)]
enum Catch {
    All,
    Types(Vec<(ExceptionId, TextRange)>),
}

impl Catch {
    fn catches(&self, hierarchy: &ExceptionHierarchy, exception: ExceptionId) -> bool {
        match self {
            Catch::All => true,
            Catch::Types(types) => types
                .iter()
                .any(|(base, _)| hierarchy.is_subclass(exception, *base)),
        }
    }
}

#[derive(Debug, Clone)]
enum ResolvedGuard {
    Try(usize),
    Catch(Catch),
}

#[derive(Debug, Clone, Copy)]
enum RaisedException {
    Class(ExceptionId),
    Unknown,
    Reraise {
        try_index: usize,
        handler_index: usize,
    },
}

#[derive(Debug)]
struct ResolvedRaise {
    exception: RaisedException,
    guards: Vec<ResolvedGuard>,
    suppression: SiteSuppression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallResolution {
    Project(Vec<FunctionId>),
    /// Resolved outside the project. `passes_callbacks` records whether a project function is
    /// passed as an argument, which the external callee may invoke.
    External {
        passes_callbacks: bool,
    },
    Unresolved,
}

#[derive(Debug)]
struct ResolvedCall {
    resolution: CallResolution,
    guards: Vec<ResolvedGuard>,
    suppression: SiteSuppression,
}

/// Exceptions dropped at a site by an inline suppression comment.
#[derive(Debug, Clone, Default)]
enum SiteSuppression {
    #[default]
    None,
    All,
    Names(Vec<String>),
}

impl SiteSuppression {
    fn drops(&self, hierarchy: &ExceptionHierarchy, exception: ExceptionId) -> bool {
        match self {
            SiteSuppression::None => false,
            SiteSuppression::All => true,
            SiteSuppression::Names(names) => names
                .iter()
                .any(|name| hierarchy.derives_from_name(exception, name)),
        }
    }
}

#[derive(Debug, Default)]
struct ResolvedFunction {
    raises: Vec<ResolvedRaise>,
    calls: Vec<ResolvedCall>,
    handlers: Vec<Vec<Catch>>,
    declared: Option<Vec<ExceptionId>>,
    documented: Option<Vec<ExceptionId>>,
}

#[derive(Debug, Default)]
struct ClassEntry {
    ancestors: Vec<String>,
    methods: FxHashMap<String, FunctionId>,
}

#[derive(Debug, Clone, Copy)]
enum Definition {
    Function(FunctionId),
    Class(usize),
}

/// One hop in an exception's provenance chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub function: FunctionId,
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    Raise {
        range: TextRange,
    },
    Call {
        range: TextRange,
        callee: FunctionId,
    },
    Declared(ContractSource),
}

/// The analyzed project: extracted facts, the resolved call graph, and propagation results.
#[derive(Debug)]
pub struct Analysis {
    pub config: ThrowsConfig,
    pub root: PathBuf,
    pub files: Vec<AnalyzedFile>,
    pub functions: Vec<FunctionNode>,
    pub hierarchy: ExceptionHierarchy,
    pub results: Vec<FunctionResult>,
    resolved: Vec<ResolvedFunction>,
    classes: FxHashMap<String, ClassEntry>,
    calls: Vec<Vec<CallResolution>>,
}

impl Analysis {
    pub fn build(project: &TyProject, loaded: &LoadedConfig) -> Result<Self> {
        Ok(Builder::new(project, loaded)?.build())
    }

    pub fn file(&self, function: FunctionId) -> &AnalyzedFile {
        &self.files[self.functions[function.index()].file.index()]
    }

    pub fn node(&self, function: FunctionId) -> &FunctionNode {
        &self.functions[function.index()]
    }

    pub fn facts(&self, function: FunctionId) -> &FunctionFacts {
        let node = self.node(function);
        &self.files[node.file.index()].facts.functions[node.index]
    }

    pub fn result(&self, function: FunctionId) -> &FunctionResult {
        &self.results[function.index()]
    }

    pub fn call_resolution(&self, function: FunctionId, site: usize) -> &CallResolution {
        &self.calls[function.index()][site]
    }

    pub fn function_ids(&self) -> impl Iterator<Item = FunctionId> + use<> {
        (0..self.functions.len()).map(|index| FunctionId(u32::try_from(index).unwrap_or(u32::MAX)))
    }

    /// Exceptions declared by the function's contract, if it has one.
    ///
    /// The contract is the union of `@throws` and the docstring's raises section, limited to
    /// the configured sources.
    pub fn contract(&self, function: FunctionId) -> Option<Vec<ExceptionId>> {
        let resolved = &self.resolved[function.index()];
        let declared = resolved
            .declared
            .as_ref()
            .filter(|_| self.config.uses(SourceKind::Decorator));
        let documented = resolved
            .documented
            .as_ref()
            .filter(|_| self.config.uses(SourceKind::Docstring));
        if declared.is_none() && documented.is_none() {
            return None;
        }
        Some(
            declared
                .into_iter()
                .chain(documented)
                .flatten()
                .copied()
                .collect(),
        )
    }

    /// The handler types of a try statement, for each handler (`None` for a catch-all).
    pub fn handler_types(
        &self,
        function: FunctionId,
        try_index: usize,
    ) -> Vec<Option<&[(ExceptionId, TextRange)]>> {
        self.resolved[function.index()].handlers[try_index]
            .iter()
            .map(|catch| match catch {
                Catch::All => None,
                Catch::Types(types) => Some(types.as_slice()),
            })
            .collect()
    }

    /// Finds the method that `function` overrides, if it is a method.
    pub fn overridden(&self, function: FunctionId) -> Option<FunctionId> {
        let facts = self.facts(function);
        let class = facts.class.as_ref()?;
        let file = self.file(function);
        let class_name = match &file.module {
            Some(module) => format!("{module}.{class}"),
            None => class.clone(),
        };
        let name = facts.qualname.rsplit('.').next()?;
        if name.starts_with("__") && name.ends_with("__") && name != "__call__" {
            return None;
        }
        let entry = self.classes.get(&class_name)?;
        entry
            .ancestors
            .iter()
            .find_map(|ancestor| self.classes.get(ancestor)?.methods.get(name).copied())
    }

    /// Follows an exception from `function` back to where it is raised or declared.
    pub fn chain(&self, function: FunctionId, exception: ExceptionId) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut visited = FxHashSet::default();
        let mut current = function;
        while visited.insert(current) {
            let Some(trace) = self.result(current).exceptions.get(&exception) else {
                break;
            };
            let facts = self.facts(current);
            match trace.via {
                Via::Raise { site } => {
                    steps.push(Step {
                        function: current,
                        kind: StepKind::Raise {
                            range: facts.raises[site].range,
                        },
                    });
                    break;
                }
                Via::Declared(source) => {
                    steps.push(Step {
                        function: current,
                        kind: StepKind::Declared(source),
                    });
                    break;
                }
                Via::Call { site, callee } => {
                    steps.push(Step {
                        function: current,
                        kind: StepKind::Call {
                            range: facts.calls[site].range,
                            callee,
                        },
                    });
                    current = callee;
                }
            }
        }
        steps
    }

    /// Finds functions by `path::qualname`, `path` (every function in the file), or
    /// `module.qualname`.
    pub fn find(&self, target: &str) -> Vec<FunctionId> {
        let (path, symbol) = match target.split_once("::") {
            Some((path, symbol)) => (Some(path), Some(symbol)),
            None if is_python_path(target) || target.contains('/') => (Some(target), None),
            None => (None, Some(target)),
        };
        let path = path.map(|path| normalize_target_path(&self.root, path));
        self.function_ids()
            .filter(|id| {
                let file = self.file(*id);
                let facts = self.facts(*id);
                match (&path, symbol) {
                    (Some(path), Some(symbol)) => {
                        file.relative_path == *path && facts.qualname == symbol
                    }
                    (Some(path), None) => file.relative_path == *path,
                    (None, Some(symbol)) => self.node(*id).symbol == symbol,
                    (None, None) => false,
                }
            })
            .collect()
    }
}

fn normalize_target_path(root: &Path, path: &str) -> String {
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let normalized = absolute.canonicalize().unwrap_or(absolute);
    match normalized.strip_prefix(root) {
        Ok(relative) => slash_path(relative),
        Err(_) => slash_path(path),
    }
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

const TRANSPARENT_DECORATORS: &[&str] = &[
    "abstractmethod",
    "asynccontextmanager",
    "cache",
    "cached_property",
    "classmethod",
    "contextmanager",
    "deleter",
    "final",
    "getter",
    "lru_cache",
    "overload",
    "override",
    "property",
    "setter",
    "singledispatch",
    "singledispatchmethod",
    "staticmethod",
    "throws",
    "wraps",
];

fn decorator_matches(name: &str, pattern: &str) -> bool {
    name == pattern || name.rsplit('.').next() == pattern.rsplit('.').next()
}

struct Builder<'a> {
    project: &'a TyProject,
    config: &'a ThrowsConfig,
    root: PathBuf,
    files: Vec<AnalyzedFile>,
    file_ids: Vec<FileId>,
    functions: Vec<FunctionNode>,
    hierarchy: ExceptionHierarchy,
    definitions: FxHashMap<(FileId, TextSize), Definition>,
    symbols: FxHashMap<String, FunctionId>,
    class_names: Vec<String>,
    classes: FxHashMap<String, ClassEntry>,
}

impl<'a> Builder<'a> {
    fn new(project: &'a TyProject, loaded: &'a LoadedConfig) -> Result<Self> {
        let root = loaded.root.canonicalize().unwrap_or(loaded.root.clone());
        let filter = FileFilter::new(&loaded.config.include, &loaded.config.exclude)?;
        let boundaries = FunctionMatcher::new(&loaded.config.boundaries)?;
        let mut builder = Self {
            project,
            config: &loaded.config,
            root: root.clone(),
            files: Vec::new(),
            file_ids: Vec::new(),
            functions: Vec::new(),
            hierarchy: ExceptionHierarchy::default(),
            definitions: FxHashMap::default(),
            symbols: FxHashMap::default(),
            class_names: Vec::new(),
            classes: FxHashMap::default(),
        };

        for id in project.file_ids() {
            let project_file = project.file(id);
            let relative_path = project_file
                .path
                .strip_prefix(&loaded.root)
                .or_else(|_| project_file.path.strip_prefix(&root))
                .map_or_else(|_| slash_path(&project_file.path), slash_path);
            if !filter.matches(&relative_path) {
                continue;
            }
            let facts = project.extract(id);
            let source = project.source(id);
            builder.file_ids.push(id);
            builder.files.push(AnalyzedFile {
                path: project_file.path.clone(),
                relative_path,
                module: project_file.module.clone(),
                facts,
                line_index: LineIndex::from_source_text(&source),
                source,
            });
        }

        for (position, file) in builder.files.iter().enumerate() {
            let file_id = builder.file_ids[position];
            for (index, function) in file.facts.functions.iter().enumerate() {
                let symbol = match &file.module {
                    Some(module) => format!("{module}.{}", function.qualname),
                    None => format!("{}::{}", file.relative_path, function.qualname),
                };
                let id = FunctionId(u32::try_from(builder.functions.len())?);
                let is_suppressing = function.decorators.iter().any(|decorator| {
                    loaded
                        .config
                        .suppressing_decorators
                        .iter()
                        .any(|pattern| decorator_matches(&decorator.name, pattern))
                });
                let has_opaque_decorator = function.decorators.iter().any(|decorator| {
                    !TRANSPARENT_DECORATORS
                        .iter()
                        .copied()
                        .chain(
                            loaded
                                .config
                                .transparent_decorators
                                .iter()
                                .map(String::as_str),
                        )
                        .chain(
                            loaded
                                .config
                                .suppressing_decorators
                                .iter()
                                .map(String::as_str),
                        )
                        .any(|pattern| decorator_matches(&decorator.name, pattern))
                });
                builder.definitions.insert(
                    (file_id, function.name_range.start()),
                    Definition::Function(id),
                );
                builder.symbols.insert(symbol.clone(), id);
                builder.functions.push(FunctionNode {
                    file: file_id,
                    index,
                    is_boundary: boundaries.matches(&file.relative_path, &symbol),
                    symbol,
                    is_suppressing,
                    has_opaque_decorator,
                });
            }
        }
        Ok(builder)
    }

    fn position(&self, file: FileId) -> Option<usize> {
        self.file_ids.binary_search(&file).ok()
    }

    fn build(mut self) -> Analysis {
        self.index_classes();
        let resolved: Vec<ResolvedFunction> = (0..self.functions.len())
            .map(|index| self.resolve_function(index))
            .collect();
        let calls: Vec<Vec<CallResolution>> = resolved
            .iter()
            .map(|function| {
                function
                    .calls
                    .iter()
                    .map(|call| call.resolution.clone())
                    .collect()
            })
            .collect();

        let mut analysis = Analysis {
            config: self.config.clone(),
            root: self.root,
            files: self.files,
            results: vec![FunctionResult::default(); self.functions.len()],
            functions: self.functions,
            hierarchy: self.hierarchy,
            resolved,
            classes: self.classes,
            calls,
        };
        propagate(&mut analysis);
        analysis
    }

    fn index_classes(&mut self) {
        for position in 0..self.files.len() {
            let file_id = self.file_ids[position];
            let classes: Vec<_> = self.files[position].facts.classes.clone();
            let module = self.files[position].module.clone();
            for class in classes {
                let info = self.project.class_at(file_id, class.name_range);
                let qualified_name = match (&info, &module) {
                    (Some(info), _) => info.qualified_name.clone(),
                    (None, Some(module)) => format!("{module}.{}", class.qualname),
                    (None, None) => class.qualname.clone(),
                };
                let ancestors = info.map(|info| info.ancestors).unwrap_or_default();
                if ancestors
                    .iter()
                    .any(|ancestor| ancestor == "builtins.BaseException")
                {
                    let id = self.hierarchy.intern(&qualified_name);
                    self.hierarchy.set_ancestors(id, &ancestors);
                    self.hierarchy.mark_project_defined(id);
                }
                // Methods are keyed by the module-qualified class path from extraction so that
                // they match `FunctionFacts::class`.
                let key = match &module {
                    Some(module) => format!("{module}.{}", class.qualname),
                    None => class.qualname.clone(),
                };
                self.definitions.insert(
                    (file_id, class.name_range.start()),
                    Definition::Class(self.class_names.len()),
                );
                self.class_names.push(key.clone());
                self.classes.entry(key).or_default().ancestors = ancestors;
            }
        }
        for id in 0..self.functions.len() {
            let node = &self.functions[id];
            let Some(position) = self.position(node.file) else {
                continue;
            };
            let file = &self.files[position];
            let facts = &file.facts.functions[node.index];
            if let Some(class) = &facts.class
                && let Some(name) = facts.qualname.rsplit('.').next()
            {
                let key = match &file.module {
                    Some(module) => format!("{module}.{class}"),
                    None => class.clone(),
                };
                self.classes.entry(key).or_default().methods.insert(
                    name.to_string(),
                    FunctionId(u32::try_from(id).unwrap_or(u32::MAX)),
                );
            }
        }
    }

    fn resolve_function(&mut self, index: usize) -> ResolvedFunction {
        let node = &self.functions[index];
        let file_id = node.file;
        let Some(position) = self.position(file_id) else {
            return ResolvedFunction::default();
        };
        let facts = self.files[position].facts.functions[node.index].clone();

        let handlers = facts
            .tries
            .iter()
            .map(|try_block| {
                try_block
                    .handlers
                    .iter()
                    .map(|handler| match &handler.types {
                        HandlerTypes::Bare | HandlerTypes::Dynamic => Catch::All,
                        HandlerTypes::Types(types) => self.catch(file_id, position, types),
                    })
                    .collect()
            })
            .collect();

        let raises = facts
            .raises
            .iter()
            .map(|raise| ResolvedRaise {
                exception: match &raise.kind {
                    RaiseKind::Exception(exception) => self
                        .exception(file_id, position, exception)
                        .map_or(RaisedException::Unknown, RaisedException::Class),
                    RaiseKind::Reraise {
                        try_index,
                        handler_index,
                    } => RaisedException::Reraise {
                        try_index: *try_index,
                        handler_index: *handler_index,
                    },
                },
                guards: self.guards(file_id, position, &raise.guards),
                suppression: self.site_suppression(position, raise.range),
            })
            .collect();

        let calls = facts
            .calls
            .iter()
            .map(|call| ResolvedCall {
                resolution: self.resolve_call(file_id, position, &facts, call),
                guards: self.guards(file_id, position, &call.guards),
                suppression: self.site_suppression(position, call.range),
            })
            .collect();

        let declared = facts.declared.as_ref().map(|refs| {
            refs.iter()
                .filter_map(|exception| self.exception(file_id, position, exception))
                .collect()
        });
        let documented = facts.docstring_raises.as_ref().map(|names| {
            names
                .iter()
                .map(|name| self.exception_by_name(position, name))
                .collect()
        });

        ResolvedFunction {
            raises,
            calls,
            handlers,
            declared,
            documented,
        }
    }

    fn site_suppression(&self, position: usize, range: TextRange) -> SiteSuppression {
        let mut names = Vec::new();
        for suppression in self.files[position].suppressions(range) {
            match &suppression.codes {
                None => return SiteSuppression::All,
                Some(codes) => names.extend(
                    codes
                        .iter()
                        .filter(|code| !code.starts_with("THROWS") && !code.contains('-'))
                        .cloned(),
                ),
            }
        }
        if names.is_empty() {
            SiteSuppression::None
        } else {
            SiteSuppression::Names(names)
        }
    }

    fn guards(&mut self, file: FileId, position: usize, guards: &[Guard]) -> Vec<ResolvedGuard> {
        guards
            .iter()
            .map(|guard| match guard {
                Guard::Try(index) => ResolvedGuard::Try(*index),
                Guard::Suppress(types) => ResolvedGuard::Catch(self.catch(file, position, types)),
                Guard::CollectAll => ResolvedGuard::Catch(Catch::All),
            })
            .collect()
    }

    fn catch(&mut self, file: FileId, position: usize, types: &[ExceptionRef]) -> Catch {
        let mut resolved = Vec::new();
        for exception in types {
            match self.exception(file, position, exception) {
                Some(id) => resolved.push((id, exception.range)),
                // An unknown handler type might catch anything, so treat it as a catch-all
                // rather than report exceptions it handles.
                None => return Catch::All,
            }
        }
        Catch::Types(resolved)
    }

    fn exception(
        &mut self,
        file: FileId,
        position: usize,
        exception: &ExceptionRef,
    ) -> Option<ExceptionId> {
        if let Some(info) = self.project.exception_class(file, exception.range) {
            if info.qualified_name == "builtins.object" {
                return None;
            }
            let id = self.hierarchy.intern(&info.qualified_name);
            self.hierarchy.set_ancestors(id, &info.ancestors);
            return Some(id);
        }
        let name = self.qualify_name(position, &exception.text)?;
        Some(self.hierarchy.intern(&name))
    }

    fn exception_by_name(&mut self, position: usize, name: &str) -> ExceptionId {
        let qualified = self
            .qualify_name(position, name)
            .unwrap_or_else(|| name.to_string());
        self.hierarchy.intern(&qualified)
    }

    /// Resolves a dotted name written in a module to a qualified name, using the module's
    /// imports, its own classes, and the built-in exceptions.
    fn qualify_name(&self, position: usize, name: &str) -> Option<String> {
        let file = &self.files[position];
        let (head, rest) = match name.split_once('.') {
            Some((head, rest)) => (head, Some(rest)),
            None => (name, None),
        };
        if let Some(import) = file
            .facts
            .imports
            .iter()
            .find(|import| import.local == head)
        {
            return Some(match rest {
                Some(rest) => format!("{}.{rest}", import.target),
                None => import.target.clone(),
            });
        }
        if file
            .facts
            .classes
            .iter()
            .any(|class| class.qualname == name)
        {
            return Some(match &file.module {
                Some(module) => format!("{module}.{name}"),
                None => name.to_string(),
            });
        }
        if rest.is_none() && is_builtin_exception(name) {
            return Some(format!("builtins.{name}"));
        }
        if self.hierarchy.get(name).is_some() {
            return Some(name.to_string());
        }
        self.hierarchy
            .ids()
            .find(|id| {
                self.hierarchy.is_project_defined(*id)
                    && name_matches(self.hierarchy.name(*id), name)
            })
            .map(|id| self.hierarchy.name(id).to_string())
    }

    fn resolve_call(
        &self,
        file: FileId,
        position: usize,
        facts: &FunctionFacts,
        call: &CallSite,
    ) -> CallResolution {
        let Some(targets) = self.project.call_targets(file, call.callee_offset) else {
            return self.resolve_call_statically(position, facts, call);
        };
        let mut functions = Vec::new();
        let mut external = false;
        for target in targets {
            match target {
                CallTarget::External => external = true,
                CallTarget::Project { file, focus } => {
                    match self.definitions.get(&(file, focus.start())) {
                        Some(Definition::Function(id)) => functions.push(*id),
                        Some(Definition::Class(class)) => {
                            functions.extend(self.constructor(&self.class_names[*class]));
                        }
                        None => return CallResolution::Unresolved,
                    }
                }
            }
        }
        if functions.is_empty() && external {
            return CallResolution::External {
                passes_callbacks: self.passes_callbacks(file, call),
            };
        }
        functions.sort_unstable();
        functions.dedup();
        CallResolution::Project(functions)
    }

    fn constructor(&self, class: &str) -> Option<FunctionId> {
        let entry = self.classes.get(class)?;
        std::iter::once(class)
            .chain(entry.ancestors.iter().map(String::as_str))
            .find_map(|class| self.classes.get(class)?.methods.get("__init__").copied())
    }

    fn passes_callbacks(&self, file: FileId, call: &CallSite) -> bool {
        call.callback_offsets.iter().any(|offset| {
            self.project
                .call_targets(file, *offset)
                .into_iter()
                .flatten()
                .any(|target| match target {
                    CallTarget::Project { file, focus } => matches!(
                        self.definitions.get(&(file, focus.start())),
                        Some(Definition::Function(_))
                    ),
                    CallTarget::External => false,
                })
        })
    }

    /// Resolves `name()`, `self.method()`, and `module.name()` from imports and scopes when the
    /// type checker cannot.
    fn resolve_call_statically(
        &self,
        position: usize,
        facts: &FunctionFacts,
        call: &CallSite,
    ) -> CallResolution {
        let file = &self.files[position];
        let module_symbol = |name: &str| match &file.module {
            Some(module) => format!("{module}.{name}"),
            None => format!("{}::{name}", file.relative_path),
        };
        let found = match call.callee.split_once('.') {
            Some(("self" | "cls", method)) if !method.contains('.') => {
                facts.class.as_ref().and_then(|class| {
                    let class = module_symbol(class);
                    let entry = self.classes.get(&class)?;
                    std::iter::once(class.as_str())
                        .chain(entry.ancestors.iter().map(String::as_str))
                        .find_map(|class| self.classes.get(class)?.methods.get(method).copied())
                })
            }
            _ => {
                let (head, rest) = call
                    .callee
                    .split_once('.')
                    .map_or((call.callee.as_str(), None), |(head, rest)| {
                        (head, Some(rest))
                    });
                let symbol = match file
                    .facts
                    .imports
                    .iter()
                    .find(|import| import.local == head)
                {
                    Some(import) => match rest {
                        Some(rest) => format!("{}.{rest}", import.target),
                        None => import.target.clone(),
                    },
                    None => module_symbol(&call.callee),
                };
                self.symbols.get(&symbol).copied()
            }
        };
        found.map_or(CallResolution::Unresolved, |function| {
            CallResolution::Project(vec![function])
        })
    }
}

fn propagate(analysis: &mut Analysis) {
    let edges: Vec<Vec<usize>> = analysis
        .calls
        .iter()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| match call {
                    CallResolution::Project(targets) => Some(targets),
                    _ => None,
                })
                .flatten()
                .map(|target| target.index())
                .collect()
        })
        .collect();

    for component in strongly_connected_components(&edges) {
        loop {
            let mut changed = false;
            for &index in &component {
                let function = FunctionId(u32::try_from(index).unwrap_or(u32::MAX));
                let result = evaluate(analysis, function);
                let previous = &analysis.results[index];
                if result.exceptions != previous.exceptions
                    || result.incomplete != previous.incomplete
                {
                    changed = true;
                }
                analysis.results[index] = result;
            }
            if !changed {
                break;
            }
        }
    }
}

struct Evaluation<'a> {
    analysis: &'a Analysis,
    resolved: &'a ResolvedFunction,
    exceptions: BTreeMap<ExceptionId, Trace>,
    tries: Vec<TryOutcome>,
    caught: Vec<Vec<BTreeMap<ExceptionId, Trace>>>,
}

impl Evaluation<'_> {
    /// Sends an exception from a site outward through the site's guards.
    fn offer(
        &mut self,
        exception: ExceptionId,
        trace: Trace,
        guards: &[ResolvedGuard],
        suppression: &SiteSuppression,
    ) {
        let hierarchy = &self.analysis.hierarchy;
        if suppression.drops(hierarchy, exception) {
            return;
        }
        for guard in guards.iter().rev() {
            match guard {
                ResolvedGuard::Try(try_index) => {
                    self.tries[*try_index].reaching.insert(exception);
                    let handlers = &self.resolved.handlers[*try_index];
                    if let Some(handler_index) = handlers
                        .iter()
                        .position(|catch| catch.catches(hierarchy, exception))
                    {
                        insert_trace(
                            &mut self.caught[*try_index][handler_index],
                            exception,
                            trace,
                        );
                        return;
                    }
                }
                ResolvedGuard::Catch(catch) => {
                    if catch.catches(hierarchy, exception) {
                        return;
                    }
                }
            }
        }
        insert_trace(&mut self.exceptions, exception, trace);
    }

    fn mark_incomplete(&mut self, guards: &[ResolvedGuard]) {
        for guard in guards {
            if let ResolvedGuard::Try(try_index) = guard {
                self.tries[*try_index].incomplete = true;
            }
        }
    }
}

fn insert_trace(map: &mut BTreeMap<ExceptionId, Trace>, exception: ExceptionId, trace: Trace) {
    map.entry(exception)
        .and_modify(|existing| {
            if trace.depth < existing.depth {
                *existing = trace;
            }
        })
        .or_insert(trace);
}

fn evaluate(analysis: &Analysis, function: FunctionId) -> FunctionResult {
    let resolved = &analysis.resolved[function.index()];
    let config = &analysis.config;
    let mut evaluation = Evaluation {
        analysis,
        resolved,
        exceptions: BTreeMap::new(),
        tries: vec![TryOutcome::default(); resolved.handlers.len()],
        caught: resolved
            .handlers
            .iter()
            .map(|handlers| vec![BTreeMap::new(); handlers.len()])
            .collect(),
    };
    let mut incomplete = false;

    if config.uses(SourceKind::Raise) {
        for (site, raise) in resolved.raises.iter().enumerate() {
            match raise.exception {
                RaisedException::Class(exception) => evaluation.offer(
                    exception,
                    Trace {
                        depth: 0,
                        via: Via::Raise { site },
                    },
                    &raise.guards,
                    &raise.suppression,
                ),
                RaisedException::Unknown => {
                    incomplete = true;
                    evaluation.mark_incomplete(&raise.guards);
                }
                RaisedException::Reraise { .. } => {}
            }
        }
    }

    for (site, call) in resolved.calls.iter().enumerate() {
        match &call.resolution {
            CallResolution::Project(targets) => {
                for &callee in targets {
                    let callee_node = analysis.node(callee);
                    let callee_result = analysis.result(callee);
                    let mut call_incomplete =
                        callee_result.incomplete || callee_node.has_opaque_decorator;
                    if callee_node.is_boundary && callee != function {
                        call_incomplete |= !callee_result.exceptions.is_empty();
                    } else {
                        for (&exception, callee_trace) in &callee_result.exceptions {
                            let depth = callee_trace.depth + 1;
                            if config.max_depth.is_some_and(|max_depth| depth > max_depth) {
                                call_incomplete = true;
                                continue;
                            }
                            evaluation.offer(
                                exception,
                                Trace {
                                    depth,
                                    via: Via::Call { site, callee },
                                },
                                &call.guards,
                                &call.suppression,
                            );
                        }
                    }
                    if call_incomplete {
                        incomplete = true;
                        evaluation.mark_incomplete(&call.guards);
                    }
                }
            }
            CallResolution::External { passes_callbacks } => {
                if *passes_callbacks {
                    incomplete = true;
                    evaluation.mark_incomplete(&call.guards);
                }
            }
            CallResolution::Unresolved => {
                incomplete = true;
                evaluation.mark_incomplete(&call.guards);
            }
        }
    }

    // Re-raises forward whatever their handler caught. Handlers can be nested, so repeat until
    // no handler catches anything new.
    loop {
        let before: usize = evaluation.caught.iter().flatten().map(BTreeMap::len).sum();
        for raise in &resolved.raises {
            if let RaisedException::Reraise {
                try_index,
                handler_index,
            } = raise.exception
                && let Some(caught) = evaluation
                    .caught
                    .get(try_index)
                    .and_then(|handlers| handlers.get(handler_index))
            {
                for (exception, trace) in caught.clone() {
                    evaluation.offer(exception, trace, &raise.guards, &raise.suppression);
                }
            }
        }
        let after: usize = evaluation.caught.iter().flatten().map(BTreeMap::len).sum();
        if after == before {
            break;
        }
    }

    let node = analysis.node(function);
    let mut exceptions = evaluation.exceptions;
    for (declared, source) in [
        (&resolved.declared, ContractSource::Decorator),
        (&resolved.documented, ContractSource::Docstring),
    ] {
        let enabled = match source {
            ContractSource::Decorator => config.uses(SourceKind::Decorator),
            ContractSource::Docstring => config.uses(SourceKind::Docstring),
        };
        if enabled && let Some(declared) = declared {
            for &exception in declared {
                exceptions.entry(exception).or_insert(Trace {
                    depth: 0,
                    via: Via::Declared(source),
                });
            }
        }
    }
    if node.is_suppressing {
        exceptions.clear();
    }

    FunctionResult {
        exceptions,
        incomplete,
        tries: evaluation.tries,
    }
}

/// Tarjan's algorithm, iterative. Components are returned callees-first: every component
/// appears after all components reachable from it.
fn strongly_connected_components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: usize = usize::MAX;
    let count = edges.len();
    let mut index = vec![UNVISITED; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut components = Vec::new();
    let mut next_index = 0;

    for start in 0..count {
        if index[start] != UNVISITED {
            continue;
        }
        let mut work = vec![(start, 0)];
        index[start] = next_index;
        low[start] = next_index;
        next_index += 1;
        stack.push(start);
        on_stack[start] = true;

        while let Some(&mut (node, ref mut edge)) = work.last_mut() {
            if let Some(&next) = edges[node].get(*edge) {
                *edge += 1;
                if index[next] == UNVISITED {
                    index[next] = next_index;
                    low[next] = next_index;
                    next_index += 1;
                    stack.push(next);
                    on_stack[next] = true;
                    work.push((next, 0));
                } else if on_stack[next] {
                    low[node] = low[node].min(index[next]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                component.sort_unstable();
                components.push(component);
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_are_callees_first() {
        // 0 -> 1 -> 2 -> 1, 0 -> 3
        let edges = vec![vec![1, 3], vec![2], vec![1], vec![]];
        let components = strongly_connected_components(&edges);
        assert_eq!(components, vec![vec![1, 2], vec![3], vec![0]]);
    }

    #[test]
    fn self_loop_is_one_component() {
        let edges = vec![vec![0]];
        assert_eq!(strongly_connected_components(&edges), vec![vec![0]]);
    }
}
