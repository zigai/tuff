use std::collections::HashSet;

use anyhow::Result;
use ruff_diagnostics::{Edit, Fix};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, NodeKind, PythonVersion};
use ruff_python_semantic::{Imported, TypingOnlyBindingsStatus};
use ruff_text_size::{Ranged, TextRange, TextSize};
use serde::Deserialize;

use crate::{
    Context, Finding, LintRule, RuleDefinition, RuleOptions, parse_options, reject_options,
};

pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! { name: "no-bare-object-annotations", code: "TUF017", explanation: "Use narrower types instead of bare object annotations, except comparison parameters and excluded paths.", node_kinds: &[NodeKind::StmtFunctionDef, NodeKind::StmtAnnAssign], checks_comments: false, fixable: false, create: object_annotations },
    crate::native_rule! { name: "use-callable-ellipsis", code: "TUF018", explanation: "Use Callable[..., T] for arbitrary callable parameters.", node_kinds: &[NodeKind::ExprSubscript], checks_comments: false, fixable: true, create: callable_ellipsis },
    crate::native_rule! { name: "use-types-from-typing", code: "TUF019", explanation: "Before Python 3.9, use typing aliases instead of builtin generic annotations.", node_kinds: &[NodeKind::ExprSubscript], checks_comments: false, fixable: true, create: types_from_typing }.replacing(&["UP006", "UP035"]).before(PythonVersion::PY39),
];

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ObjectAnnotations {
    excluded_path_parts: Vec<String>,
}
impl Default for ObjectAnnotations {
    fn default() -> Self {
        Self {
            excluded_path_parts: vec!["tests".into()],
        }
    }
}
fn object_annotations(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    Ok(Box::new(parse_options::<ObjectAnnotations>(o)?))
}
#[derive(Debug)]
struct CallableEllipsis;
fn callable_ellipsis(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(CallableEllipsis))
}
#[derive(Debug)]
struct TypesFromTyping;
fn types_from_typing(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(TypesFromTyping))
}

// Look up active source bindings as well as semantic qualified names. The source lookup
// follows assignment aliases without confusing a later rebinding with the current use.
#[derive(Clone, Debug)]
pub(super) enum Binding<'a> {
    Qualified(String),
    Value(&'a Expr),
    TypeAlias(&'a Expr),
    Shadow,
}
pub(super) fn lookup<'a>(c: &Context<'_, 'a, '_>, name: &str, at: TextSize) -> Option<Binding<'a>> {
    let semantic = c.resolver.semantic_model()?;
    let scope = c.scope_at(at)?;
    let lexical = semantic
        .lookup_symbol_in_scope(name, scope, false)
        .binding_id();
    let runtime = semantic.simulate_runtime_load_at_location_in_scope(
        name,
        TextRange::empty(at),
        scope,
        TypingOnlyBindingsStatus::Allowed,
    );
    // Completed scopes have already cleared exception targets after the handler.
    let id = runtime
        .filter(|id| {
            matches!(
                semantic.binding(*id).kind,
                ruff_python_semantic::BindingKind::BoundException
            )
        })
        .or(lexical)
        .or(runtime)?;
    let binding = semantic.binding(id);
    if let Some(import) = binding.as_any_import() {
        return Some(Binding::Qualified(
            import.qualified_name().segments().join("."),
        ));
    }
    if matches!(binding.kind, ruff_python_semantic::BindingKind::Builtin) {
        return None;
    }
    Some(bound_value(c, binding.range()))
}

fn is_type_alias(c: &Context<'_, '_, '_>, annotation: &Expr) -> bool {
    c.qualified_name(annotation).is_some_and(|n| {
        matches!(
            n.as_str(),
            "typing.TypeAlias" | "typing_extensions.TypeAlias"
        )
    }) || matches!(annotation, Expr::Name(n) if n.id == "TypeAlias" && c.binding_range(n).is_none())
}
fn bound_value<'a>(c: &Context<'_, 'a, '_>, range: TextRange) -> Binding<'a> {
    struct Value<'a, 'b, 'c, 'd> {
        c: &'a Context<'b, 'c, 'd>,
        range: TextRange,
        found: Option<Binding<'c>>,
    }
    impl<'a> Value<'_, '_, 'a, '_> {
        fn target(&mut self, target: &'a Expr, value: &'a Expr, alias: bool) {
            let sequence = |e: &'a Expr| -> Option<&'a [Expr]> {
                match e {
                    Expr::List(n) => Some(&n.elts),
                    Expr::Tuple(n) => Some(&n.elts),
                    _ => None,
                }
            };
            if let (Some(targets), Some(values)) = (sequence(target), sequence(value)) {
                if targets.len() == values.len() {
                    for (t, v) in targets.iter().zip(values) {
                        self.target(t, v, alias);
                    }
                }
            } else if target.range() == self.range && matches!(target, Expr::Name(_)) {
                self.found = Some(if alias {
                    Binding::TypeAlias(value)
                } else {
                    Binding::Value(value)
                });
            }
        }
    }
    impl<'a> SourceOrderVisitor<'a> for Value<'_, '_, 'a, '_> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            match node {
                AnyNodeRef::StmtAssign(a) => {
                    for target in &a.targets {
                        self.target(target, &a.value, false);
                    }
                }
                AnyNodeRef::StmtAnnAssign(a) => {
                    if let Some(value) = &a.value {
                        self.target(&a.target, value, is_type_alias(self.c, &a.annotation));
                    }
                }
                AnyNodeRef::StmtTypeAlias(a) => self.target(&a.name, &a.value, true),
                AnyNodeRef::ExprNamed(a) => self.target(&a.target, &a.value, false),
                _ => {}
            }
            TraversalSignal::Traverse
        }
    }
    let mut visitor = Value {
        c,
        range,
        found: None,
    };
    AnyNodeRef::ModModule(c.module).visit_source_order(&mut visitor);
    visitor.found.unwrap_or(Binding::Shadow)
}
fn active_binding<'a>(c: &Context<'_, 'a, '_>, n: &ast::ExprName) -> Option<Binding<'a>> {
    c.binding_range(n).map(|range| {
        if let Some(import) = c
            .resolver
            .semantic_model()
            .and_then(|semantic| {
                semantic
                    .bindings
                    .iter()
                    .find(|binding| binding.range() == range)
            })
            .and_then(|binding| binding.as_any_import())
        {
            Binding::Qualified(import.qualified_name().segments().join("."))
        } else {
            bound_value(c, range)
        }
    })
}
pub(super) fn resolve<'a>(
    c: &Context<'_, 'a, '_>,
    mut expression: &'a Expr,
    star_modules: &[&str],
) -> Option<String> {
    let mut seen = HashSet::new();
    let mut attributes = Vec::new();
    let mut symbol = loop {
        if !seen.insert(expression.range()) {
            return None;
        }
        if let Some(qualified) = c.qualified_name(expression) {
            break qualified;
        }
        match expression {
            Expr::Name(name) => match active_binding(c, name) {
                Some(Binding::Qualified(qualified)) => break qualified,
                Some(Binding::Value(value) | Binding::TypeAlias(value)) => expression = value,
                None => {
                    let mut imports = star_modules
                        .iter()
                        .filter(|module| has_star(c, &[module], name.start()));
                    let module = imports.next()?;
                    if imports.next().is_some() {
                        return None;
                    }
                    break format!("{module}.{}", name.id);
                }
                _ => return None,
            },
            Expr::Attribute(attribute) => {
                attributes.push(attribute.attr.as_str());
                expression = &attribute.value;
            }
            Expr::Named(named) => expression = &named.value,
            _ => return None,
        }
    };
    for attribute in attributes.into_iter().rev() {
        symbol.push('.');
        symbol.push_str(attribute);
    }
    Some(symbol)
}

pub(super) fn has_star(c: &Context<'_, '_, '_>, modules: &[&str], at: TextSize) -> bool {
    struct Stars<'a> {
        modules: &'a [&'a str],
        at: TextSize,
        found: bool,
    }
    impl<'a> SourceOrderVisitor<'a> for Stars<'_> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            if node.start() >= self.at {
                return TraversalSignal::Skip;
            }
            match node {
                AnyNodeRef::StmtImportFrom(i)
                    if i.level == 0 && i.names.iter().any(|a| a.name.as_str() == "*") =>
                {
                    if i.module
                        .as_ref()
                        .is_some_and(|m| self.modules.contains(&m.as_str()))
                    {
                        self.found = true;
                    }
                }
                AnyNodeRef::StmtFunctionDef(f) if !f.range().contains(self.at) => {
                    return TraversalSignal::Skip;
                }
                AnyNodeRef::StmtClassDef(f) if !f.range().contains(self.at) => {
                    return TraversalSignal::Skip;
                }
                _ => {}
            }
            TraversalSignal::Traverse
        }
    }
    let mut visitor = Stars {
        modules,
        at,
        found: false,
    };
    AnyNodeRef::ModModule(c.module).visit_source_order(&mut visitor);
    visitor.found
}

pub(super) fn imported(
    c: &Context<'_, '_, '_>,
    e: &Expr,
    qualified: &[&str],
    stars: &[&str],
    plain: &str,
) -> bool {
    resolve(c, e, &[]).is_some_and(|q| qualified.contains(&q.as_str()))
        || matches!(e, Expr::Name(n) if n.id == plain && lookup(c, plain, n.start()).is_none() && c.binding_range(n).is_none() && has_star(c, stars, n.start()))
}
fn items(e: &Expr) -> &[Expr] {
    match e {
        Expr::Tuple(t) => &t.elts,
        _ => std::slice::from_ref(e),
    }
}
fn annotation_name(
    c: &Context<'_, '_, '_>,
    e: &Expr,
    quoted_at: Option<TextSize>,
) -> Option<String> {
    let Some(at) = quoted_at else {
        return c.qualified_name(e);
    };
    match e {
        Expr::Name(name) => match lookup(c, name.id.as_str(), at) {
            Some(Binding::Qualified(qualified)) => Some(qualified),
            None if matches!(
                name.id.as_str(),
                "builtins" | "typing" | "typing_extensions"
            ) =>
            {
                Some(name.id.to_string())
            }
            _ => None,
        },
        Expr::Attribute(attribute) => Some(format!(
            "{}.{}",
            annotation_name(c, &attribute.value, quoted_at)?,
            attribute.attr
        )),
        _ => None,
    }
}
fn typing_name(
    c: &Context<'_, '_, '_>,
    e: &Expr,
    names: &[&str],
    plain: &str,
    quoted_at: Option<TextSize>,
) -> bool {
    annotation_name(c, e, quoted_at).is_some_and(|q| names.contains(&q.as_str()))
        || matches!(e, Expr::Name(n) if n.id == plain && lookup(c, plain, quoted_at.unwrap_or(n.start())).is_none() && (quoted_at.is_some() || c.binding_range(n).is_none()))
}
fn object_atom(
    c: &Context<'_, '_, '_>,
    e: &Expr,
    seen: &mut HashSet<TextRange>,
    quoted_at: Option<TextSize>,
) -> bool {
    if let Some(at) = quoted_at {
        return match e {
            Expr::Name(name) => match lookup(c, name.id.as_str(), at) {
                Some(Binding::TypeAlias(value)) => {
                    alias_name_is_bare(c, name.id.as_str(), at, value, seen)
                }
                Some(Binding::Qualified(qualified)) => qualified == "builtins.object",
                None => name.id == "object",
                _ => false,
            },
            Expr::Attribute(_) => {
                annotation_name(c, e, quoted_at).as_deref() == Some("builtins.object")
            }
            _ => false,
        };
    }
    let qualified = c.qualified_name(e);
    if qualified.as_deref() == Some("builtins.object") {
        return true;
    }
    if qualified.is_none() {
        if matches!(e, Expr::Name(n) if n.id == "object" && c.binding_range(n).is_none() && lookup(c, "object", n.start()).is_none())
        {
            return true;
        }
        fn root(e: &Expr) -> &Expr {
            match e {
                Expr::Attribute(a) => root(&a.value),
                _ => e,
            }
        }
        if dotted_name(e).as_deref() == Some("builtins.object") {
            let head = root(e);
            if c.qualified_name(head).as_deref() == Some("builtins")
                || matches!(head, Expr::Name(n) if c.binding_range(n).is_none() && lookup(c, n.id.as_str(), n.start()).is_none())
            {
                return true;
            }
        }
    }
    // Ordinary assignment aliases are not explicit type aliases.
    if let Expr::Name(n) = e {
        if let Some(Binding::TypeAlias(v)) = active_binding(c, n) {
            return alias_name_is_bare(c, n.id.as_str(), e.start(), v, seen);
        }
    }
    false
}
fn alias_name_is_bare(
    c: &Context<'_, '_, '_>,
    name: &str,
    at: TextSize,
    value: &Expr,
    seen: &mut HashSet<TextRange>,
) -> bool {
    let Some(semantic) = c.resolver.semantic_model() else {
        return false;
    };
    let Some(scope) = c.scope_at(value.start()) else {
        return false;
    };
    let Some(id) = semantic
        .lookup_symbol_in_scope(name, scope, false)
        .binding_id()
    else {
        return false;
    };
    let mut found = false;
    for id in semantic.scopes[semantic.binding(id).scope].get_all(name) {
        let binding = semantic.binding(id);
        let Binding::TypeAlias(value) = bound_value(c, binding.range()) else {
            return false;
        };
        if binding.start() >= at || !seen.insert(binding.range()) {
            return false;
        }
        let bare = bare_object(c, value, seen, None);
        seen.remove(&binding.range());
        if !bare {
            return false;
        }
        found = true;
    }
    found
}
fn object_none(
    c: &Context<'_, '_, '_>,
    a: &Expr,
    b: &Expr,
    seen: &mut HashSet<TextRange>,
    quoted_at: Option<TextSize>,
) -> bool {
    (matches!(a, Expr::NoneLiteral(_)) && object_atom(c, b, seen, quoted_at))
        || (matches!(b, Expr::NoneLiteral(_)) && object_atom(c, a, seen, quoted_at))
}
fn bare_object(
    c: &Context<'_, '_, '_>,
    e: &Expr,
    seen: &mut HashSet<TextRange>,
    quoted_at: Option<TextSize>,
) -> bool {
    if object_atom(c, e, seen, quoted_at) {
        return true;
    }
    match e {
        Expr::StringLiteral(s) => {
            let Ok(parsed) = ruff_python_parser::parse_expression(s.value.to_str()) else {
                return false;
            };
            bare_object(
                c,
                &parsed.syntax().body,
                seen,
                quoted_at.or(Some(e.start())),
            )
        }
        Expr::BinOp(b) if b.op == ast::Operator::BitOr => {
            object_none(c, &b.left, &b.right, seen, quoted_at)
        }
        Expr::Subscript(s) => {
            let elements = items(&s.slice);
            if typing_name(c, &s.value, &["typing.Optional"], "Optional", quoted_at)
                && elements.len() == 1
            {
                return bare_object(c, &elements[0], seen, quoted_at);
            }
            if typing_name(c, &s.value, &["typing.Union"], "Union", quoted_at)
                && elements.len() == 2
            {
                return object_none(c, &elements[0], &elements[1], seen, quoted_at);
            }
            if typing_name(
                c,
                &s.value,
                &["typing.Annotated", "typing_extensions.Annotated"],
                "Annotated",
                quoted_at,
            ) {
                return elements
                    .first()
                    .is_some_and(|e| bare_object(c, e, seen, quoted_at));
            }
            false
        }
        _ => false,
    }
}

fn dotted_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(n) => Some(n.id.to_string()),
        Expr::Attribute(a) => dotted_name(&a.value).map(|q| format!("{q}.{}", a.attr)),
        _ => None,
    }
}

impl LintRule for ObjectAnnotations {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let filename = c.path.file_name().and_then(|p| p.to_str()).unwrap_or("");
        if filename.starts_with("test_")
            || filename.ends_with("_test.py")
            || c.path.components().any(|part| {
                self.excluded_path_parts
                    .iter()
                    .any(|p| part.as_os_str() == std::ffi::OsStr::new(p))
            })
        {
            return;
        }
        let mut check = |e: &Expr| {
            if bare_object(c, e, &mut HashSet::new(), None) {
                out.push(Finding::new(
                    "no-bare-object-annotations",
                    e.range(),
                    "Replace this bare `object` annotation with a type that describes the value.",
                ));
            }
        };
        match node {
            AnyNodeRef::StmtFunctionDef(f) => {
                for parameter in &f.parameters {
                    if matches!(f.name.as_str(), "__eq__" | "__ne__")
                        && matches!(parameter.name().as_str(), "other" | "o")
                    {
                        continue;
                    }
                    if let Some(a) = parameter.annotation() {
                        check(a);
                    }
                }
                if let Some(a) = &f.returns {
                    check(a);
                }
            }
            AnyNodeRef::StmtAnnAssign(a) => check(&a.annotation),
            _ => {}
        }
    }
}
impl LintRule for CallableEllipsis {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ExprSubscript(s) = node else {
            return;
        };
        if !imported(
            c,
            &s.value,
            &[
                "typing.Callable",
                "typing_extensions.Callable",
                "collections.abc.Callable",
            ],
            &["typing", "typing_extensions", "collections.abc"],
            "Callable",
        ) {
            return;
        }
        let elements = items(&s.slice);
        let [Expr::List(parameters), _] = elements else {
            return;
        };
        if !matches!(parameters.elts.as_slice(), [Expr::EllipsisLiteral(_)]) {
            return;
        }
        let mut finding = Finding::new(
            "use-callable-ellipsis",
            s.range(),
            "Use Callable[..., T] instead of Callable[[...], T].",
        );
        if !c.text(parameters.range()).contains('#') {
            finding = finding.with_fix(
                Fix::safe_edit(Edit::range_replacement("...".into(), parameters.range())),
                "Replace parameter list with ellipsis",
            );
        }
        out.push(finding);
    }
}

fn unknown_star(c: &Context<'_, '_, '_>, at: TextSize) -> bool {
    struct Unknown {
        at: TextSize,
        found: bool,
    }
    impl<'a> SourceOrderVisitor<'a> for Unknown {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            if node.start() >= self.at {
                return TraversalSignal::Skip;
            }
            if let AnyNodeRef::StmtImportFrom(i) = node {
                if i.names.iter().any(|a| a.name.as_str() == "*")
                    && !i
                        .module
                        .as_ref()
                        .is_some_and(|m| matches!(m.as_str(), "builtins" | "typing"))
                {
                    self.found = true;
                }
            }
            TraversalSignal::Traverse
        }
    }
    let mut visitor = Unknown { at, found: false };
    AnyNodeRef::ModModule(c.module).visit_source_order(&mut visitor);
    visitor.found
}
fn unambiguous_typing_import(c: &Context<'_, '_, '_>, name: &str, at: TextSize) -> bool {
    let Some(semantic) = c.resolver.semantic_model() else {
        return false;
    };
    let Some(scope) = c.scope_at(at) else {
        return false;
    };
    let Some(id) = semantic
        .lookup_symbol_in_scope(name, scope, false)
        .binding_id()
    else {
        return false;
    };
    let binding = semantic.binding(id);
    semantic.scopes[binding.scope].get_all(name).all(|id| {
        let binding = semantic.binding(id);
        binding
            .as_any_import()
            .is_some_and(|import| import.qualified_name().segments() == ["typing", name])
            && binding.scope.is_global()
            && binding.end() < at
    })
}
impl LintRule for TypesFromTyping {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ExprSubscript(s) = node else {
            return;
        };
        if !c.in_annotation(s.range())
            || c.ancestors
                .iter()
                .any(|n| matches!(n, AnyNodeRef::StmtTypeAlias(_)))
        {
            return;
        }
        let Some(qualified) = resolve(c, &s.value, &[]) else {
            return;
        };
        let (builtin, correct) = match qualified.as_str() {
            "builtins.dict" => ("dict", "Dict"),
            "builtins.list" => ("list", "List"),
            "builtins.set" => ("set", "Set"),
            "builtins.tuple" => ("tuple", "Tuple"),
            _ => return,
        };
        if matches!(&*s.value, Expr::Name(n) if n.id == builtin) && unknown_star(c, s.start()) {
            return;
        }
        let mut finding = Finding::new(
            "use-types-from-typing",
            s.value.range(),
            format!(
                "Python 3.8 does not support `{builtin}[...]` annotations; use `typing.{correct}` instead."
            ),
        );
        // Only unconditional, module-level imports under the exact replacement
        // name are eligible; introducing imports would exceed the original fix.
        let available = c.module.body.iter().any(|statement| {
            matches!(statement, ast::Stmt::ImportFrom(i) if i.level == 0 && i.module.as_ref().is_some_and(|m| m.as_str() == "typing") && i.names.iter().any(|a| a.name.as_str() == correct && a.asname.as_ref().is_none_or(|n| n.as_str() == correct)))
        });
        if available && unambiguous_typing_import(c, correct, s.start()) {
            finding = finding.with_fix(
                Fix::safe_edit(Edit::range_replacement(correct.into(), s.value.range())),
                format!("Replace with {correct}"),
            );
        }
        out.push(finding);
    }
}
