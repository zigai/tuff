use std::collections::{HashMap, HashSet};

use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_expr, walk_stmt,
};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, ExprContext, Stmt};
use ruff_text_size::{Ranged, TextRange, TextSize};

use super::facts::{assigned_nodes, target_names};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScopeKind {
    Module,
    Function,
    Class,
    Type,
}

struct LocalBinding {
    range: TextRange,
    available_after: TextSize,
}
#[derive(Default)]
struct Bindings {
    local: HashMap<String, Vec<LocalBinding>>,
    global: HashSet<String>,
    nonlocal: HashSet<String>,
    available_after: Option<TextSize>,
}
impl Bindings {
    fn bind(&mut self, name: &str, range: TextRange) {
        self.local
            .entry(name.to_string())
            .or_default()
            .push(LocalBinding {
                range,
                available_after: self.available_after.unwrap_or(range.end()),
            });
    }
    fn body(body: &[Stmt]) -> Self {
        let mut result = Self::default();
        result.visit_body(body);
        result
    }
    fn parameters(&mut self, parameters: &ast::Parameters) {
        for parameter in parameters {
            self.bind(parameter.name().as_str(), parameter.name().range());
        }
    }
}
impl<'a> SourceOrderVisitor<'a> for Bindings {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(s) => {
                self.available_after = Some(s.end());
                self.bind(s.name.as_str(), s.name.range());
                self.available_after = None;
            }
            Stmt::ClassDef(s) => {
                self.available_after = Some(s.end());
                self.bind(s.name.as_str(), s.name.range());
                self.available_after = None;
            }
            Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::AugAssign(_) => {
                let previous = self.available_after.replace(stmt.end());
                walk_stmt(self, stmt);
                self.available_after = previous;
            }
            Stmt::Global(s) => self.global.extend(s.names.iter().map(ToString::to_string)),
            Stmt::Nonlocal(s) => self
                .nonlocal
                .extend(s.names.iter().map(ToString::to_string)),
            Stmt::Import(s) => {
                for alias in &s.names {
                    let n = alias.asname.as_ref().unwrap_or(&alias.name);
                    self.bind(
                        n.as_str().split('.').next().unwrap_or(n.as_str()),
                        n.range(),
                    );
                }
            }
            Stmt::ImportFrom(s) => {
                for alias in &s.names {
                    let n = alias.asname.as_ref().unwrap_or(&alias.name);
                    self.bind(n.as_str(), n.range());
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => {}
            // Iteration targets belong to the comprehension, not its containing scope.
            Expr::ListComp(s) => {
                for g in &s.generators {
                    self.visit_expr(&g.iter);
                    for e in &g.ifs {
                        self.visit_expr(e);
                    }
                }
                self.visit_expr(&s.elt);
            }
            Expr::SetComp(s) => {
                for g in &s.generators {
                    self.visit_expr(&g.iter);
                    for e in &g.ifs {
                        self.visit_expr(e);
                    }
                }
                self.visit_expr(&s.elt);
            }
            Expr::DictComp(s) => {
                for g in &s.generators {
                    self.visit_expr(&g.iter);
                    for e in &g.ifs {
                        self.visit_expr(e);
                    }
                }
                if let Some(key) = &s.key {
                    self.visit_expr(key);
                }
                self.visit_expr(&s.value);
            }
            Expr::Generator(s) => {
                for g in &s.generators {
                    self.visit_expr(&g.iter);
                    for e in &g.ifs {
                        self.visit_expr(e);
                    }
                }
                self.visit_expr(&s.elt);
            }
            Expr::Name(n) if n.ctx != ExprContext::Load => self.bind(n.id.as_str(), n.range()),
            _ => walk_expr(self, expr),
        }
    }
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        match node {
            AnyNodeRef::ExceptHandlerExceptHandler(h) => {
                if let Some(n) = &h.name {
                    self.bind(n.as_str(), n.range());
                }
            }
            AnyNodeRef::PatternMatchAs(p) => {
                if let Some(n) = &p.name {
                    self.bind(n.as_str(), n.range());
                }
            }
            AnyNodeRef::PatternMatchStar(p) => {
                if let Some(n) = &p.name {
                    self.bind(n.as_str(), n.range());
                }
            }
            AnyNodeRef::PatternMatchMapping(p) => {
                if let Some(n) = &p.rest {
                    self.bind(n.as_str(), n.range());
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }
}

struct Scope {
    kind: ScopeKind,
    bindings: Bindings,
}
struct Capture {
    candidate: HashMap<String, TextRange>,
    outer: ScopeKind,
    scopes: Vec<Scope>,
    found: bool,
}
impl Capture {
    fn access(&mut self, name: &ast::ExprName) {
        let Some(candidate_range) = self.candidate.get(name.id.as_str()) else {
            return;
        };
        let mut crossed_scope = false;
        for scope in self.scopes.iter().rev() {
            if scope.kind == ScopeKind::Class && crossed_scope {
                continue;
            }
            if scope.bindings.global.contains(name.id.as_str()) {
                if self.outer == ScopeKind::Module {
                    self.found = true;
                }
                return;
            }
            if scope.bindings.nonlocal.contains(name.id.as_str()) {
                crossed_scope = true;
                continue;
            }
            if let Some(bindings) = scope.bindings.local.get(name.id.as_str()) {
                if scope.kind != ScopeKind::Class
                    || bindings.iter().any(|binding| {
                        binding.available_after <= name.start() && binding.range != *candidate_range
                    })
                {
                    return;
                }
            }
            crossed_scope |= scope.kind != ScopeKind::Type;
        }
        // Neither methods nor nested classes close over an enclosing class namespace.
        if !(crossed_scope && self.outer == ScopeKind::Class)
            && candidate_range.start() < name.start()
        {
            self.found = true;
        }
    }
    fn defaults(&mut self, parameters: &ast::Parameters) {
        for parameter in parameters.iter_non_variadic_params() {
            if let Some(value) = &parameter.default {
                self.visit_expr(value);
            }
        }
        for parameter in parameters {
            if let Some(annotation) = parameter.annotation() {
                self.visit_expr(annotation);
            }
        }
    }
    fn function(&mut self, function: &ast::StmtFunctionDef) {
        for decorator in &function.decorator_list {
            self.visit_expr(&decorator.expression);
        }
        if let Some(parameters) = &function.type_params {
            self.type_parameters(parameters);
        }
        self.defaults(&function.parameters);
        if let Some(returns) = &function.returns {
            self.visit_expr(returns);
        }
        let mut bindings = Bindings::body(&function.body);
        bindings.parameters(&function.parameters);
        self.scopes.push(Scope {
            kind: ScopeKind::Function,
            bindings,
        });
        self.visit_body(&function.body);
        self.scopes.pop();
        if function.type_params.is_some() {
            self.scopes.pop();
        }
    }
    fn class(&mut self, class: &ast::StmtClassDef) {
        for decorator in &class.decorator_list {
            self.visit_expr(&decorator.expression);
        }
        if let Some(parameters) = &class.type_params {
            self.type_parameters(parameters);
        }
        if let Some(arguments) = &class.arguments {
            self.visit_arguments(arguments);
        }
        self.scopes.push(Scope {
            kind: ScopeKind::Class,
            bindings: Bindings::body(&class.body),
        });
        self.visit_body(&class.body);
        self.scopes.pop();
        if class.type_params.is_some() {
            self.scopes.pop();
        }
    }
    fn type_parameters(&mut self, parameters: &ast::TypeParams) {
        let mut bindings = Bindings::default();
        for parameter in parameters {
            bindings.bind(parameter.name().as_str(), parameter.name().range());
        }
        self.scopes.push(Scope {
            kind: ScopeKind::Type,
            bindings,
        });
        self.visit_type_params(parameters);
    }
    fn comprehension(&mut self, generators: &[ast::Comprehension], expressions: &[&Expr]) {
        let Some(first) = generators.first() else {
            return;
        };
        self.visit_expr(&first.iter);
        let mut bindings = Bindings::default();
        for generator in generators {
            let mut nodes = Vec::new();
            target_names(&generator.target, &mut nodes);
            for name in nodes {
                bindings.bind(name.id.as_str(), name.range());
            }
        }
        self.scopes.push(Scope {
            kind: ScopeKind::Function,
            bindings,
        });
        for (index, generator) in generators.iter().enumerate() {
            if index > 0 {
                self.visit_expr(&generator.iter);
            }
            for condition in &generator.ifs {
                self.visit_expr(condition);
            }
        }
        for expression in expressions {
            self.visit_expr(expression);
        }
        self.scopes.pop();
    }
}
impl<'a> SourceOrderVisitor<'a> for Capture {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(s) => self.function(s),
            Stmt::ClassDef(s) => self.class(s),
            Stmt::TypeAlias(alias) => {
                if let Some(parameters) = &alias.type_params {
                    self.type_parameters(parameters);
                }
                self.visit_expr(&alias.value);
                if alias.type_params.is_some() {
                    self.scopes.pop();
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(n) if n.ctx == ExprContext::Load => self.access(n),
            Expr::Lambda(s) => {
                let mut bindings = Bindings::default();
                if let Some(parameters) = &s.parameters {
                    self.defaults(parameters);
                    bindings.parameters(parameters);
                }
                // Assignment expressions also bind in lambda scope.
                bindings.visit_expr(&s.body);
                self.scopes.push(Scope {
                    kind: ScopeKind::Function,
                    bindings,
                });
                self.visit_expr(&s.body);
                self.scopes.pop();
            }
            Expr::ListComp(s) => self.comprehension(&s.generators, &[&s.elt]),
            Expr::SetComp(s) => self.comprehension(&s.generators, &[&s.elt]),
            Expr::DictComp(s) => {
                let body: Vec<&Expr> = s
                    .key
                    .as_deref()
                    .into_iter()
                    .chain(std::iter::once(s.value.as_ref()))
                    .collect();
                self.comprehension(&s.generators, &body);
            }
            Expr::Generator(s) => self.comprehension(&s.generators, &[&s.elt]),
            _ => walk_expr(self, expr),
        }
    }
}

pub(super) fn captures(assignment: &Stmt, definition: &Stmt, outer: ScopeKind) -> bool {
    let candidate = assigned_nodes(assignment)
        .into_iter()
        .map(|n| (n.id.to_string(), n.range()))
        .collect();
    let mut capture = Capture {
        candidate,
        outer,
        scopes: Vec::new(),
        found: false,
    };
    capture.visit_stmt(definition);
    capture.found
}
