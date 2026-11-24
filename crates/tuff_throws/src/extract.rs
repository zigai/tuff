//! Per-file exception facts collected from a parsed Python module.
//!
//! Extraction is purely syntactic. Every expression that needs semantic information later (raised
//! exception types, handler types, call targets) is recorded by range or offset so the graph
//! builder can ask the type checker about it.

use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{self as ast, ExceptHandler, Expr, ModModule, Stmt};
use ruff_python_parser::Parsed;
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextRange, TextSize};

/// A reference to an exception class in source, such as `ValueError` in `raise ValueError(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionRef {
    /// Dotted source text, without call arguments (`errors.NotFound` for `errors.NotFound(...)`).
    pub text: String,
    /// Range of the full expression, used to infer its type.
    pub range: TextRange,
}

/// A construct that stops exceptions from leaving a region of code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// The body of the try statement at this index in [`FunctionFacts::tries`].
    Try(usize),
    /// The body of `with contextlib.suppress(...)`.
    Suppress(Vec<ExceptionRef>),
    /// The arguments of `asyncio.gather(..., return_exceptions=True)`.
    CollectAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerTypes {
    /// `except:`
    Bare,
    Types(Vec<ExceptionRef>),
    /// A handler type that is neither a class reference nor a tuple of them, such as a variable
    /// holding a tuple.
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    pub range: TextRange,
    pub types: HandlerTypes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryBlock {
    pub range: TextRange,
    pub handlers: Vec<Handler>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaiseKind {
    /// `raise X`, `raise X(...)`, or `raise X from Y`.
    Exception(ExceptionRef),
    /// A bare `raise` (or `raise err` for the handler's bound name) inside an except handler.
    Reraise {
        try_index: usize,
        handler_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaiseSite {
    pub kind: RaiseKind,
    pub range: TextRange,
    /// Active guards, outermost first.
    pub guards: Vec<Guard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// Dotted source text of the callee, used for display and fallback resolution.
    pub callee: String,
    /// Offset of the callee's name (`fetch` in `client.fetch()`), used for call resolution.
    pub callee_offset: TextSize,
    pub range: TextRange,
    /// Active guards, outermost first.
    pub guards: Vec<Guard>,
    /// Offsets of plain name or attribute arguments, which may be callbacks the callee invokes.
    pub callback_offsets: Vec<TextSize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decorator {
    pub name: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionFacts {
    /// Dotted path within the module, such as `OrderService.run` or `outer.inner`.
    pub qualname: String,
    pub name_range: TextRange,
    pub range: TextRange,
    /// Qualified name of the class whose body directly contains this function.
    pub class: Option<String>,
    pub decorators: Vec<Decorator>,
    /// Exceptions declared with `@throws(...)`, if the function has that decorator.
    pub declared: Option<Vec<ExceptionRef>>,
    /// Exception names listed in a `Raises` docstring section, if the docstring has one.
    pub docstring_raises: Option<Vec<String>>,
    pub raises: Vec<RaiseSite>,
    pub calls: Vec<CallSite>,
    pub tries: Vec<TryBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassFacts {
    pub qualname: String,
    pub name_range: TextRange,
}

/// A name bound by an import statement, mapped to the qualified name it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub local: String,
    pub target: String,
}

/// An inline `# tuff: ignore[...]` or `# throws: ignore[...]` comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// One-based line number.
    pub line: usize,
    /// Diagnostic codes, rule names, or exception names. `None` suppresses everything.
    pub codes: Option<Vec<String>>,
}

impl Suppression {
    pub fn matches(&self, candidates: &[&str]) -> bool {
        self.codes.as_ref().is_none_or(|codes| {
            codes
                .iter()
                .any(|code| candidates.iter().any(|candidate| candidate == code))
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleFacts {
    pub functions: Vec<FunctionFacts>,
    pub classes: Vec<ClassFacts>,
    pub imports: Vec<ImportBinding>,
    pub suppressions: Vec<Suppression>,
}

impl ModuleFacts {
    pub fn suppressions_on_line(&self, line: usize) -> impl Iterator<Item = &Suppression> {
        self.suppressions
            .iter()
            .filter(move |suppression| suppression.line == line)
    }
}

/// Module identity used to resolve relative imports.
#[derive(Debug, Clone, Copy)]
pub struct ModuleContext<'a> {
    pub name: Option<&'a str>,
    /// Whether the file is a package's `__init__.py`.
    pub is_package: bool,
}

pub fn extract(parsed: &Parsed<ModModule>, source: &str, module: ModuleContext) -> ModuleFacts {
    let mut collector = Collector {
        source,
        module,
        facts: ModuleFacts::default(),
        scopes: Vec::new(),
        current: None,
        guards: Vec::new(),
        handlers: Vec::new(),
    };
    collector.visit_body(&parsed.syntax().body);
    let mut facts = collector.facts;
    facts.suppressions = suppressions(parsed, source);
    facts
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Class,
    Function,
}

struct ActiveHandler {
    try_index: usize,
    handler_index: usize,
    name: Option<String>,
}

struct Collector<'src> {
    source: &'src str,
    module: ModuleContext<'src>,
    facts: ModuleFacts,
    scopes: Vec<(ScopeKind, String)>,
    current: Option<usize>,
    guards: Vec<Guard>,
    handlers: Vec<ActiveHandler>,
}

impl Collector<'_> {
    fn qualname(&self, name: &str) -> String {
        let mut parts: Vec<&str> = self.scopes.iter().map(|(_, name)| name.as_str()).collect();
        parts.push(name);
        parts.join(".")
    }

    fn function(&mut self) -> Option<&mut FunctionFacts> {
        let index = self.current?;
        self.facts.functions.get_mut(index)
    }

    fn exception_ref(&self, expr: &Expr) -> ExceptionRef {
        let target = match expr {
            Expr::Call(call) => &call.func,
            _ => expr,
        };
        ExceptionRef {
            text: dotted_name(target).unwrap_or_else(|| self.source[target.range()].to_string()),
            range: expr.range(),
        }
    }

    fn handler_types(&self, type_: Option<&Expr>) -> HandlerTypes {
        match type_ {
            None => HandlerTypes::Bare,
            Some(Expr::Tuple(tuple)) => {
                if tuple.elts.iter().all(|elt| dotted_name(elt).is_some()) {
                    HandlerTypes::Types(
                        tuple
                            .elts
                            .iter()
                            .map(|elt| self.exception_ref(elt))
                            .collect(),
                    )
                } else {
                    HandlerTypes::Dynamic
                }
            }
            Some(expr) if dotted_name(expr).is_some() => {
                HandlerTypes::Types(vec![self.exception_ref(expr)])
            }
            Some(_) => HandlerTypes::Dynamic,
        }
    }

    fn visit_function(&mut self, function: &ast::StmtFunctionDef) {
        for decorator in &function.decorator_list {
            self.visit_decorator(decorator);
        }
        self.visit_parameters(&function.parameters);

        let qualname = self.qualname(&function.name);
        let class = match self.scopes.last() {
            Some((ScopeKind::Class, _)) => Some(self.scopes_qualname()),
            _ => None,
        };
        let declared = function
            .decorator_list
            .iter()
            .filter_map(|decorator| self.throws_decorator(&decorator.expression))
            .reduce(|mut all, more| {
                all.extend(more);
                all
            });
        let facts = FunctionFacts {
            qualname,
            name_range: function.name.range(),
            range: function.range(),
            class,
            decorators: function
                .decorator_list
                .iter()
                .filter_map(|decorator| {
                    let target = match &decorator.expression {
                        Expr::Call(call) => &call.func,
                        expr => expr,
                    };
                    Some(Decorator {
                        name: dotted_name(target)?,
                        range: decorator.range(),
                    })
                })
                .collect(),
            declared,
            docstring_raises: docstring(&function.body)
                .as_deref()
                .and_then(docstring_raises),
            raises: Vec::new(),
            calls: Vec::new(),
            tries: Vec::new(),
        };
        self.facts.functions.push(facts);

        let outer_current = self.current.replace(self.facts.functions.len() - 1);
        let outer_guards = std::mem::take(&mut self.guards);
        let outer_handlers = std::mem::take(&mut self.handlers);
        self.scopes
            .push((ScopeKind::Function, function.name.to_string()));

        self.visit_body(&function.body);

        self.scopes.pop();
        self.handlers = outer_handlers;
        self.guards = outer_guards;
        self.current = outer_current;
    }

    fn scopes_qualname(&self) -> String {
        self.scopes
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Returns the exceptions listed in `@throws(...)`.
    ///
    /// Accepted forms are `@throws(A)`, `@throws(A, "message")`, `@throws(A, B)`, and
    /// `@throws((A, "message"), (B, "message"))`.
    fn throws_decorator(&self, expr: &Expr) -> Option<Vec<ExceptionRef>> {
        let Expr::Call(call) = expr else {
            return None;
        };
        let name = dotted_name(&call.func)?;
        if name.rsplit('.').next() != Some("throws") {
            return None;
        }
        let refs = call
            .arguments
            .args
            .iter()
            .filter_map(|arg| match arg {
                Expr::Tuple(tuple) => tuple.elts.first(),
                Expr::StringLiteral(_) => None,
                expr => Some(expr),
            })
            .map(|expr| self.exception_ref(expr))
            .collect();
        Some(refs)
    }

    fn visit_try(&mut self, stmt: &ast::StmtTry) {
        let handlers = stmt
            .handlers
            .iter()
            .map(|handler| {
                let ExceptHandler::ExceptHandler(handler) = handler;
                Handler {
                    range: handler.range(),
                    types: self.handler_types(handler.type_.as_deref()),
                }
            })
            .collect();
        let index = self.function().map(|function| {
            function.tries.push(TryBlock {
                range: stmt.range(),
                handlers,
            });
            function.tries.len() - 1
        });

        if let Some(index) = index {
            self.guards.push(Guard::Try(index));
        }
        self.visit_body(&stmt.body);
        if index.is_some() {
            self.guards.pop();
        }

        for (handler_index, handler) in stmt.handlers.iter().enumerate() {
            let ExceptHandler::ExceptHandler(handler) = handler;
            if let Some(type_) = &handler.type_ {
                self.visit_expr(type_);
            }
            if let Some(index) = index {
                self.handlers.push(ActiveHandler {
                    try_index: index,
                    handler_index,
                    name: handler.name.as_ref().map(ToString::to_string),
                });
            }
            self.visit_body(&handler.body);
            if index.is_some() {
                self.handlers.pop();
            }
        }
        self.visit_body(&stmt.orelse);
        self.visit_body(&stmt.finalbody);
    }

    fn visit_with(&mut self, stmt: &ast::StmtWith) {
        let mut pushed = 0;
        for item in &stmt.items {
            self.visit_expr(&item.context_expr);
            if let Some(vars) = &item.optional_vars {
                self.visit_expr(vars);
            }
            if let Expr::Call(call) = &item.context_expr
                && let Some(name) = dotted_name(&call.func)
                && matches!(name.as_str(), "suppress" | "contextlib.suppress")
            {
                let types = call
                    .arguments
                    .args
                    .iter()
                    .map(|arg| self.exception_ref(arg))
                    .collect();
                self.guards.push(Guard::Suppress(types));
                pushed += 1;
            }
        }
        self.visit_body(&stmt.body);
        self.guards.truncate(self.guards.len() - pushed);
    }

    fn visit_raise(&mut self, stmt: &ast::StmtRaise) {
        if let Some(exc) = &stmt.exc {
            self.visit_expr(exc);
        }
        if let Some(cause) = &stmt.cause {
            self.visit_expr(cause);
        }
        let kind = match stmt.exc.as_deref() {
            None => self.handlers.last().map(|handler| RaiseKind::Reraise {
                try_index: handler.try_index,
                handler_index: handler.handler_index,
            }),
            Some(Expr::Name(name))
                if let Some(handler) = self
                    .handlers
                    .iter()
                    .rev()
                    .find(|handler| handler.name.as_deref() == Some(name.id.as_str())) =>
            {
                Some(RaiseKind::Reraise {
                    try_index: handler.try_index,
                    handler_index: handler.handler_index,
                })
            }
            Some(expr) => Some(RaiseKind::Exception(self.exception_ref(expr))),
        };
        let guards = self.guards.clone();
        if let Some(kind) = kind
            && let Some(function) = self.function()
        {
            function.raises.push(RaiseSite {
                kind,
                range: stmt.range(),
                guards,
            });
        }
    }

    fn visit_call(&mut self, call: &ast::ExprCall) {
        let callee = dotted_name(&call.func);
        let callee_offset = match call.func.as_ref() {
            Expr::Attribute(attribute) => attribute.attr.start(),
            func => func.start(),
        };
        let callback_offsets = call
            .arguments
            .args
            .iter()
            .chain(call.arguments.keywords.iter().map(|keyword| &keyword.value))
            .filter_map(|arg| match arg {
                Expr::Name(name) => Some(name.start()),
                Expr::Attribute(attribute) => Some(attribute.attr.start()),
                _ => None,
            })
            .collect();
        let guards = self.guards.clone();
        let source = self.source;
        if let Some(function) = self.function() {
            function.calls.push(CallSite {
                callee: callee
                    .clone()
                    .unwrap_or_else(|| source[call.func.range()].to_string()),
                callee_offset,
                range: call.range(),
                guards,
                callback_offsets,
            });
        }

        self.visit_expr(&call.func);
        let collects_exceptions = callee
            .as_deref()
            .is_some_and(|name| name == "gather" || name.ends_with(".gather"))
            && call.arguments.keywords.iter().any(|keyword| {
                keyword.arg.as_deref() == Some("return_exceptions")
                    && matches!(&keyword.value, Expr::BooleanLiteral(literal) if literal.value)
            });
        if collects_exceptions {
            self.guards.push(Guard::CollectAll);
        }
        self.visit_arguments(&call.arguments);
        if collects_exceptions {
            self.guards.pop();
        }
    }

    fn record_import(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let target = alias.name.to_string();
                    let local = alias.asname.as_ref().map_or_else(
                        || target.split('.').next().unwrap_or(&target).to_string(),
                        ToString::to_string,
                    );
                    let target = if alias.asname.is_some() {
                        target
                    } else {
                        local.clone()
                    };
                    self.facts.imports.push(ImportBinding { local, target });
                }
            }
            Stmt::ImportFrom(import) => {
                let Some(base) =
                    resolve_relative(self.module, import.module.as_deref(), import.level)
                else {
                    return;
                };
                for alias in &import.names {
                    if &alias.name == "*" {
                        continue;
                    }
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                    let target = if base.is_empty() {
                        alias.name.to_string()
                    } else {
                        format!("{base}.{}", alias.name)
                    };
                    self.facts.imports.push(ImportBinding { local, target });
                }
            }
            _ => {}
        }
    }
}

impl<'a> Visitor<'a> for Collector<'_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(function) => self.visit_function(function),
            Stmt::ClassDef(class) => {
                for decorator in &class.decorator_list {
                    self.visit_decorator(decorator);
                }
                if let Some(arguments) = &class.arguments {
                    self.visit_arguments(arguments);
                }
                self.facts.classes.push(ClassFacts {
                    qualname: self.qualname(&class.name),
                    name_range: class.name.range(),
                });
                self.scopes.push((ScopeKind::Class, class.name.to_string()));
                // Class bodies run when the class is defined, not when an enclosing function's
                // caller runs, so they are not attributed to any function.
                let outer_current = self.current.take();
                let outer_guards = std::mem::take(&mut self.guards);
                let outer_handlers = std::mem::take(&mut self.handlers);
                self.visit_body(&class.body);
                self.handlers = outer_handlers;
                self.guards = outer_guards;
                self.current = outer_current;
                self.scopes.pop();
            }
            Stmt::Try(try_stmt) => self.visit_try(try_stmt),
            Stmt::With(with) => self.visit_with(with),
            Stmt::Raise(raise) => self.visit_raise(raise),
            Stmt::Import(_) | Stmt::ImportFrom(_) => {
                if self.scopes.is_empty() {
                    self.record_import(stmt);
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_annotation(&mut self, _expr: &'a Expr) {}

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Call(call) => self.visit_call(call),
            // Lambda bodies run when the lambda is called, which is not a call site we can see.
            Expr::Lambda(_) => {}
            _ => walk_expr(self, expr),
        }
    }
}

fn resolve_relative(context: ModuleContext, module: Option<&str>, level: u32) -> Option<String> {
    if level == 0 {
        return Some(module.unwrap_or_default().to_string());
    }
    let mut parts: Vec<&str> = context.name?.split('.').collect();
    let pops = if context.is_package { level - 1 } else { level };
    for _ in 0..pops {
        parts.pop()?;
    }
    if let Some(module) = module {
        parts.push(module);
    }
    Some(parts.join("."))
}

/// Returns `a.b.c` for a name or attribute chain.
pub fn dotted_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.to_string()),
        Expr::Attribute(attribute) => {
            let mut prefix = dotted_name(&attribute.value)?;
            prefix.push('.');
            prefix.push_str(&attribute.attr);
            Some(prefix)
        }
        _ => None,
    }
}

fn docstring(body: &[Stmt]) -> Option<String> {
    let Some(Stmt::Expr(ast::StmtExpr { value, .. })) = body.first() else {
        return None;
    };
    let Expr::StringLiteral(literal) = value.as_ref() else {
        return None;
    };
    Some(literal.value.to_str().to_string())
}

/// Returns the exception names listed in a docstring's raises section.
///
/// Supports Google (`Raises:` followed by indented `Name: description` entries), NumPy (`Raises`
/// underlined with dashes, followed by `Name` lines), and Sphinx (`:raises Name: description`)
/// styles. Returns `None` when the docstring has no raises section.
pub fn docstring_raises(doc: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = doc.lines().collect();
    let mut names = Vec::new();
    let mut found = false;

    for line in &lines {
        let trimmed = line.trim();
        for prefix in [":raises ", ":raise ", ":except ", ":exception "] {
            if let Some(rest) = trimmed.strip_prefix(prefix)
                && let Some((entry, _)) = rest.split_once(':')
            {
                found = true;
                names.extend(
                    entry
                        .split(',')
                        .map(str::trim)
                        .filter(|name| is_dotted_identifier(name))
                        .map(ToString::to_string),
                );
            }
        }
    }

    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        let indent = indentation(line);
        let numpy = matches!(trimmed, "Raises" | "Raise")
            && lines
                .get(index + 1)
                .is_some_and(|next| is_underline(next.trim()));
        if !(numpy || matches!(trimmed, "Raises:" | "Raise:")) {
            index += 1;
            continue;
        }
        found = true;
        index += if numpy { 2 } else { 1 };
        let mut entry_indent = None;
        while let Some(line) = lines.get(index) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                index += 1;
                if numpy {
                    continue;
                }
                // A blank line followed by a dedented line ends a Google section.
                if lines
                    .get(index)
                    .is_some_and(|next| indentation(next) <= indent && !next.trim().is_empty())
                {
                    break;
                }
                continue;
            }
            let line_indent = indentation(line);
            if numpy {
                if lines
                    .get(index + 1)
                    .is_some_and(|next| is_underline(next.trim()))
                {
                    break;
                }
            } else if line_indent <= indent {
                break;
            }
            let expected = *entry_indent.get_or_insert(line_indent);
            if line_indent == expected {
                let entry = trimmed.split(':').next().unwrap_or_default();
                names.extend(
                    entry
                        .split(',')
                        .map(str::trim)
                        .filter(|name| is_dotted_identifier(name))
                        .map(ToString::to_string),
                );
            } else if line_indent < expected {
                break;
            }
            index += 1;
        }
    }

    found.then_some(names)
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_underline(line: &str) -> bool {
    line.len() >= 3 && line.chars().all(|char| char == '-')
}

fn is_dotted_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
                && chars.all(|char| char.is_alphanumeric() || char == '_')
        })
}

fn suppressions(parsed: &Parsed<ModModule>, source: &str) -> Vec<Suppression> {
    let line_index = LineIndex::from_source_text(source);
    parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .filter_map(|token| {
            parse_suppression(
                &source[token.range()],
                line_index.line_index(token.start()).get(),
            )
        })
        .collect()
}

/// Parses `# tuff: ignore`, `# throws: ignore[...]`, and the same forms after other comments.
fn parse_suppression(comment: &str, line: usize) -> Option<Suppression> {
    comment.split('#').find_map(|part| {
        let part = part.trim();
        let rest = part
            .strip_prefix("tuff:")
            .or_else(|| part.strip_prefix("throws:"))?
            .trim_start()
            .strip_prefix("ignore")?;
        let codes = if let Some(codes) = rest.strip_prefix('[') {
            let (codes, _) = codes.split_once(']')?;
            Some(
                codes
                    .split(',')
                    .map(str::trim)
                    .filter(|code| !code.is_empty())
                    .map(ToString::to_string)
                    .collect(),
            )
        } else if rest.trim().is_empty() {
            None
        } else {
            return None;
        };
        Some(Suppression { line, codes })
    })
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use ruff_python_parser::parse_module;

    use super::*;

    fn facts(source: &str) -> ModuleFacts {
        let parsed = parse_module(source).unwrap();
        extract(
            &parsed,
            source,
            ModuleContext {
                name: Some("pkg.module"),
                is_package: false,
            },
        )
    }

    fn function<'a>(facts: &'a ModuleFacts, qualname: &str) -> &'a FunctionFacts {
        facts
            .functions
            .iter()
            .find(|function| function.qualname == qualname)
            .unwrap()
    }

    fn raised(function: &FunctionFacts) -> Vec<&str> {
        function
            .raises
            .iter()
            .filter_map(|raise| match &raise.kind {
                RaiseKind::Exception(exception) => Some(exception.text.as_str()),
                RaiseKind::Reraise { .. } => None,
            })
            .collect()
    }

    #[test]
    fn raise_forms() {
        let facts = facts(indoc! {"
            def f(x):
                if x:
                    raise ValueError('bad')
                raise errors.NotFound from None

            def g():
                raise KeyError
        "});
        assert_eq!(
            raised(function(&facts, "f")),
            ["ValueError", "errors.NotFound"]
        );
        assert_eq!(raised(function(&facts, "g")), ["KeyError"]);
    }

    #[test]
    fn reraise_inside_handler() {
        let facts = facts(indoc! {"
            def f():
                try:
                    g()
                except ValueError:
                    raise
                except KeyError as err:
                    raise err
                except OSError as err:
                    raise RuntimeError() from err
        "});
        let f = function(&facts, "f");
        let kinds: Vec<_> = f.raises.iter().map(|raise| raise.kind.clone()).collect();
        assert_eq!(
            kinds[..2],
            [
                RaiseKind::Reraise {
                    try_index: 0,
                    handler_index: 0
                },
                RaiseKind::Reraise {
                    try_index: 0,
                    handler_index: 1
                },
            ]
        );
        assert_eq!(raised(f), ["RuntimeError"]);
        // Handlers are not guarded by their own try statement.
        assert!(f.raises.iter().all(|raise| raise.guards.is_empty()));
    }

    #[test]
    fn bare_raise_outside_handler_is_ignored() {
        let facts = facts("def f():\n    raise\n");
        assert!(function(&facts, "f").raises.is_empty());
    }

    #[test]
    fn calls_carry_guards() {
        let facts = facts(indoc! {"
            import contextlib

            def f():
                a()
                try:
                    with contextlib.suppress(KeyError, OSError):
                        b()
                except (ValueError, errors.Domain):
                    c()
                else:
                    d()
        "});
        let f = function(&facts, "f");
        let guards: Vec<_> = f
            .calls
            .iter()
            .map(|call| (call.callee.as_str(), call.guards.len()))
            .collect();
        assert_eq!(
            guards,
            [
                ("a", 0),
                ("contextlib.suppress", 1),
                ("b", 2),
                ("c", 0),
                ("d", 0)
            ]
        );
        let b = &f.calls[2];
        let Guard::Suppress(types) = &b.guards[1] else {
            panic!("expected a suppress guard");
        };
        let names: Vec<_> = types.iter().map(|ty| ty.text.as_str()).collect();
        assert_eq!(names, ["KeyError", "OSError"]);
        let HandlerTypes::Types(types) = &f.tries[0].handlers[0].types else {
            panic!("expected handler types");
        };
        let names: Vec<_> = types.iter().map(|ty| ty.text.as_str()).collect();
        assert_eq!(names, ["ValueError", "errors.Domain"]);
    }

    #[test]
    fn handler_type_shapes() {
        let facts = facts(indoc! {"
            def f():
                try:
                    pass
                except:
                    pass
                except ERRORS:
                    pass
                except tuple(x):
                    pass
        "});
        let handlers: Vec<_> = function(&facts, "f").tries[0]
            .handlers
            .iter()
            .map(|handler| handler.types.clone())
            .collect();
        assert_eq!(handlers[0], HandlerTypes::Bare);
        assert!(matches!(handlers[1], HandlerTypes::Types(_)));
        assert_eq!(handlers[2], HandlerTypes::Dynamic);
    }

    #[test]
    fn gather_with_return_exceptions_collects() {
        let facts = facts(indoc! {"
            async def f():
                await asyncio.gather(a(), b())
                await asyncio.gather(c(), return_exceptions=True)
        "});
        let guarded: Vec<_> = function(&facts, "f")
            .calls
            .iter()
            .map(|call| (call.callee.as_str(), call.guards == [Guard::CollectAll]))
            .collect();
        assert_eq!(
            guarded,
            [
                ("asyncio.gather", false),
                ("a", false),
                ("b", false),
                ("asyncio.gather", false),
                ("c", true),
            ]
        );
    }

    #[test]
    fn methods_nested_functions_and_class_bodies() {
        let facts = facts(indoc! {"
            class Service(Base):
                setting = load()

                @property
                def name(self):
                    return self.compute()

                def run(self):
                    def helper():
                        inner()
                    helper()
        "});
        let qualnames: Vec<_> = facts
            .functions
            .iter()
            .map(|function| (function.qualname.as_str(), function.class.as_deref()))
            .collect();
        assert_eq!(
            qualnames,
            [
                ("Service.name", Some("Service")),
                ("Service.run", Some("Service")),
                ("Service.run.helper", None),
            ]
        );
        let callees: Vec<_> = function(&facts, "Service.run")
            .calls
            .iter()
            .map(|call| call.callee.as_str())
            .collect();
        assert_eq!(callees, ["helper"]);
        assert_eq!(
            function(&facts, "Service.name").decorators[0].name,
            "property"
        );
        assert_eq!(facts.classes[0].qualname, "Service");
    }

    #[test]
    fn lambda_bodies_are_not_call_sites() {
        let facts = facts("def f():\n    return lambda: g()\n");
        assert!(function(&facts, "f").calls.is_empty());
    }

    #[test]
    fn callback_arguments() {
        let facts = facts("def f():\n    run(handler, key=self.on_done, value=compute())\n");
        let call = &function(&facts, "f").calls[0];
        assert_eq!(call.callback_offsets.len(), 2);
    }

    #[test]
    fn throws_decorator_forms() {
        let facts = facts(indoc! {r#"
            @throws(ValueError, "bad value")
            @lib.throws((KeyError, "missing"), errors.Domain)
            def f():
                pass

            @other(ValueError)
            def g():
                pass
        "#});
        let declared: Vec<_> = function(&facts, "f")
            .declared
            .as_ref()
            .unwrap()
            .iter()
            .map(|exception| exception.text.as_str())
            .collect();
        assert_eq!(declared, ["ValueError", "KeyError", "errors.Domain"]);
        assert_eq!(function(&facts, "g").declared, None);
    }

    #[test]
    fn google_docstring() {
        let doc = indoc! {"
            Load a record.

            Args:
                key: The key.

            Raises:
                NotFound: If the key is missing.
                    Continued description: with a colon.
                errors.Invalid, TypeError: On bad input.

            Returns:
                The record.
        "};
        assert_eq!(
            docstring_raises(doc),
            Some(vec![
                "NotFound".to_string(),
                "errors.Invalid".to_string(),
                "TypeError".to_string()
            ])
        );
    }

    #[test]
    fn numpy_docstring() {
        let doc = indoc! {"
            Load a record.

            Raises
            ------
            NotFound
                If the key is missing.
            ValueError
                If the key is malformed.

            Returns
            -------
            Record
        "};
        assert_eq!(
            docstring_raises(doc),
            Some(vec!["NotFound".to_string(), "ValueError".to_string()])
        );
    }

    #[test]
    fn sphinx_docstring() {
        let doc = indoc! {"
            Load a record.

            :param key: The key.
            :raises NotFound: If the key is missing.
            :raise ValueError: If the key is malformed.
        "};
        assert_eq!(
            docstring_raises(doc),
            Some(vec!["NotFound".to_string(), "ValueError".to_string()])
        );
    }

    #[test]
    fn docstring_without_raises_section() {
        assert_eq!(docstring_raises("Do things.\n\nReturns:\n    x\n"), None);
    }

    #[test]
    fn docstring_attached_to_function() {
        let facts = facts(indoc! {r#"
            def f():
                """Do it.

                Raises:
                    KeyError: Missing.
                """
        "#});
        assert_eq!(
            function(&facts, "f").docstring_raises,
            Some(vec!["KeyError".to_string()])
        );
    }

    #[test]
    fn suppression_comments() {
        let facts = facts(indoc! {r##"
            def f():
                a()  # tuff: ignore[THROWS001, ValueError]
                b()  # throws: ignore
                c()  # noqa: E501 # throws: ignore[KeyError]
                d("# tuff: ignore")
                e()  # tuff: ignored
        "##});
        assert_eq!(
            facts.suppressions,
            [
                Suppression {
                    line: 2,
                    codes: Some(vec!["THROWS001".to_string(), "ValueError".to_string()])
                },
                Suppression {
                    line: 3,
                    codes: None
                },
                Suppression {
                    line: 4,
                    codes: Some(vec!["KeyError".to_string()])
                },
            ]
        );
    }

    #[test]
    fn imports() {
        let facts = facts(indoc! {"
            import os.path
            import json as j
            from . import sibling
            from ..errors import NotFound as Missing
            from app.models import Order
        "});
        let bindings: Vec<_> = facts
            .imports
            .iter()
            .map(|import| (import.local.as_str(), import.target.as_str()))
            .collect();
        assert_eq!(
            bindings,
            [
                ("os", "os"),
                ("j", "json"),
                ("sibling", "pkg.sibling"),
                ("Missing", "errors.NotFound"),
                ("Order", "app.models.Order"),
            ]
        );
    }
}
