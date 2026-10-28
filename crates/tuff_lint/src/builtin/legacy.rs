use crate::{Context, Finding, LintRule, RuleDefinition, RuleOptions, reject_options};
use anyhow::Result;
use ruff_diagnostics::{Edit, Fix};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_node};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, NodeKind};
use ruff_text_size::{Ranged, TextRange, TextSize};

mod assertions;
mod collections;
mod comparisons;
mod scopes;
mod strings;

// All superficially similar Ruff rules differ in their accepted shapes or fix
// policy. Keep the compatibility rules native instead of enabling a superset.
pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! { name: "collapse-isinstance-checks", code: "TUF201", explanation: "Combine repeated isinstance checks on a name.", node_kinds: &[NodeKind::ExprBoolOp], checks_comments: false, fixable: false, create: collapse }.replacing(&["SIM101"]),
    crate::native_rule! { name: "deprecated-unittest-asserts", code: "TUF202", explanation: "Replace deprecated unittest assertion aliases.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: true, create: assertions::deprecated }.replacing(&["UP005"]),
    crate::native_rule! { name: "no-inherit-from-object", code: "TUF203", explanation: "Remove redundant object inheritance.", node_kinds: &[NodeKind::StmtClassDef], checks_comments: false, fixable: true, create: object }.replacing(&["UP004"]),
    crate::native_rule! { name: "no-or-in-except", code: "TUF204", explanation: "Use tuples to catch multiple exception types.", node_kinds: &[NodeKind::ExceptHandlerExceptHandler], checks_comments: false, fixable: false, create: except }.replacing(&["PLW0711"]),
    crate::native_rule! { name: "no-redundant-arguments-super", code: "TUF205", explanation: "Prefer zero-argument super calls.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: false, create: super_call }.replacing(&["UP008"]),
    crate::native_rule! { name: "no-redundant-f-string", code: "TUF206", explanation: "Remove f prefixes from strings without replacement fields, without creating docstrings.", node_kinds: &[NodeKind::ExprFString], checks_comments: false, fixable: true, create: strings::redundant }.replacing(&["F541"]),
    crate::native_rule! { name: "no-redundant-lambda", code: "TUF207", explanation: "Avoid lambdas that only forward their arguments.", node_kinds: &[NodeKind::ExprLambda], checks_comments: false, fixable: false, create: lambda }.replacing(&["PLW0108"]),
    crate::native_rule! { name: "no-redundant-list-comprehension", code: "TUF208", explanation: "Prefer generators inside any and all.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: false, create: list_comprehension }.replacing(&["C419"]),
    crate::native_rule! { name: "no-string-type-annotation", code: "TUF209", explanation: "Unquote annotations when future annotations are enabled.", node_kinds: &[NodeKind::ExprStringLiteral, NodeKind::ExprBytesLiteral], checks_comments: false, fixable: true, create: strings::annotation }.replacing(&["UP037", "PYI020"]),
    crate::native_rule! { name: "replace-union-with-optional", code: "TUF210", explanation: "Prefer Optional[T] over two-member Union containing None.", node_kinds: &[NodeKind::Parameter, NodeKind::StmtFunctionDef, NodeKind::StmtAnnAssign], checks_comments: false, fixable: false, create: optional }.replacing(&["UP007", "UP045"]),
    crate::native_rule! { name: "use-assert-equal", code: "TUF211", explanation: "Prefer assertEqual and assertNotEqual for equality assertions.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: true, create: assertions::equal }.replacing(&["PT009"]),
    crate::native_rule! { name: "use-assert-in", code: "TUF212", explanation: "Prefer assertIn and assertNotIn for membership assertions.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: true, create: assertions::membership }.replacing(&["PT009"]),
    crate::native_rule! { name: "use-assert-is-not-none", code: "TUF213", explanation: "Prefer assertIsNone and assertIsNotNone for None assertions.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: true, create: assertions::none }.replacing(&["PT009"]),
    crate::native_rule! { name: "use-async-sleep-in-async-def", code: "TUF214", explanation: "Avoid blocking time.sleep in async functions, including aliases.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: false, create: scopes::sleep }.replacing(&["ASYNC251"]),
    crate::native_rule! { name: "use-cls-in-classmethod", code: "TUF215", explanation: "Use cls as the first classmethod parameter.", node_kinds: &[NodeKind::StmtFunctionDef], checks_comments: false, fixable: true, create: scopes::classmethod }.replacing(&["N804"]),
    crate::native_rule! { name: "use-collections-abc", code: "TUF216", explanation: "Import abstract base classes from collections.abc.", node_kinds: &[NodeKind::StmtImportFrom, NodeKind::StmtImport, NodeKind::StmtClassDef], checks_comments: false, fixable: true, create: collections::abc }.replacing(&["UP035"]),
    crate::native_rule! { name: "use-comprehension", code: "TUF217", explanation: "Prefer comprehensions to redundant collection constructors.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: false, create: comprehension }.replacing(&["C400", "C401", "C402", "C403", "C404", "C411"]),
    crate::native_rule! { name: "use-eq-for-primitives", code: "TUF218", explanation: "Compare numbers and strings by value.", node_kinds: &[NodeKind::ExprCompare], checks_comments: false, fixable: true, create: comparisons::primitives }.replacing(&["F632"]),
    crate::native_rule! { name: "use-f-string", code: "TUF219", explanation: "Prefer f-strings to percent formatting and str.format.", node_kinds: &[NodeKind::ExprBinOp, NodeKind::ExprCall], checks_comments: false, fixable: true, create: strings::fstring }.replacing(&["UP031", "UP032"]),
    crate::native_rule! { name: "use-is-for-singletons", code: "TUF220", explanation: "Compare None, True, and False by identity.", node_kinds: &[NodeKind::ExprCompare], checks_comments: false, fixable: true, create: comparisons::singletons }.replacing(&["E711", "E712"]),
    crate::native_rule! { name: "use-literal", code: "TUF221", explanation: "Prefer collection literals to redundant constructors.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: true, create: collections::literal }.replacing(&["C405", "C406", "C408", "C409", "C410"]),
];

#[derive(Debug)]
enum Legacy {
    Collapse,
    Object,
    Except,
    Super,
    Lambda,
    ListComprehension,
    Optional,
    Comprehension,
}
fn collapse(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Collapse))
}
fn object(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Object))
}
fn except(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Except))
}
fn super_call(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Super))
}
fn lambda(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Lambda))
}
fn list_comprehension(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::ListComprehension))
}
fn optional(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Optional))
}
fn comprehension(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Legacy::Comprehension))
}

impl LintRule for Legacy {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        match (self, node) {
            (Self::Collapse, AnyNodeRef::ExprBoolOp(boolean)) if boolean.op == ast::BoolOp::Or => {
                // LibCST's left-associated chain is one diagnostic, not one per pair.
                if c.ancestors.last().is_some_and(|p| matches!(p, AnyNodeRef::ExprBoolOp(b) if b.op == ast::BoolOp::Or && b.values.first().is_some_and(|v| v.range() == boolean.range()))) { return; }
                let mut seen = Vec::new();
                let mut duplicate = false;
                let mut operands = Vec::new();
                or_operands(boolean, &mut operands);
                for operand in operands {
                    let Expr::Call(call) = operand else {
                        continue;
                    };
                    let args = call
                        .arguments
                        .iter_source_order()
                        .map(|arg| unstar(arg.value()))
                        .collect::<Vec<_>>();
                    if args.len() != 2
                        || args[1].is_tuple_expr()
                        || c.qualified_name(&call.func).as_deref() != Some("builtins.isinstance")
                    {
                        continue;
                    }
                    let Expr::Name(target) = args[0] else {
                        continue;
                    };
                    let spelling = c.text(target.range());
                    if seen.contains(&spelling) {
                        duplicate = true;
                    } else {
                        seen.push(spelling);
                    }
                }
                if duplicate {
                    out.push(Finding::new("collapse-isinstance-checks", boolean.range(), "Multiple isinstance calls with the same target but different types can be collapsed into a single call with a tuple of types."));
                }
            }
            (Self::Except, AnyNodeRef::ExceptHandlerExceptHandler(handler)) => {
                if let Some(expression) = handler.type_.as_deref() {
                    if all_nodes(expression.into())
                        .iter()
                        .any(|n| matches!(n, AnyNodeRef::ExprBoolOp(b) if b.op == ast::BoolOp::Or))
                    {
                        out.push(Finding::new("no-or-in-except", expression.range(), "Use `except (ValueError, TypeError):`; `except ValueError or TypeError:` catches only `ValueError`."));
                    }
                }
            }
            (Self::Object, AnyNodeRef::StmtClassDef(class)) => {
                let Some(args) = &class.arguments else {
                    return;
                };
                if !args
                    .args
                    .iter()
                    .any(|base| c.qualified_name(base).as_deref() == Some("builtins.object"))
                {
                    return;
                }
                let mut finding = Finding::new(
                    "no-inherit-from-object",
                    class.range(),
                    "Remove the redundant `object` base class.",
                );
                if args.args.len() == 1
                    && args.keywords.is_empty()
                    && !has_comments(c.text(class.range()))
                {
                    finding = finding.with_fix(
                        Fix::safe_edit(Edit::range_deletion(args.range())),
                        "Remove redundant object base",
                    );
                }
                out.push(finding);
            }
            (Self::Super, AnyNodeRef::ExprCall(call)) => {
                if !name(&call.func, "super") {
                    return;
                }
                let args = call
                    .arguments
                    .iter_source_order()
                    .map(|a| unstar(a.value()))
                    .collect::<Vec<_>>();
                let [class, receiver] = args.as_slice() else {
                    return;
                };
                let Some(function) = c.enclosing_function() else {
                    return;
                };
                let Some(param) = function
                    .parameters
                    .posonlyargs
                    .first()
                    .or_else(|| function.parameters.args.first())
                else {
                    return;
                };
                if !name(receiver, param.parameter.name.as_str()) {
                    return;
                }
                let chain = c
                    .ancestors
                    .iter()
                    .filter_map(|n| match n {
                        AnyNodeRef::StmtClassDef(class) => Some(class.name.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(".");
                if !chain.is_empty() && dotted(class).as_deref() == Some(chain.as_str()) {
                    out.push(Finding::new(
                        "no-redundant-arguments-super",
                        call.range(),
                        "Call `super()` without arguments.",
                    ));
                }
            }
            (Self::Lambda, AnyNodeRef::ExprLambda(lambda)) => {
                if c.in_class_body() {
                    return;
                }
                let Expr::Call(call) = &*lambda.body else {
                    return;
                };
                let Expr::Name(function) = &*call.func else {
                    return;
                };
                if !call.arguments.keywords.is_empty() {
                    return;
                }
                let parameters = lambda.parameters.as_ref();
                if parameters.is_some_and(|p| {
                    !p.posonlyargs.is_empty()
                        || !p.kwonlyargs.is_empty()
                        || p.vararg.is_some()
                        || p.kwarg.is_some()
                        || p.args.iter().any(|a| a.default.is_some())
                }) {
                    return;
                }
                let params = parameters.map_or(&[][..], |p| &*p.args);
                if params.len() == call.arguments.args.len()
                    && params
                        .iter()
                        .zip(call.arguments.args.iter())
                        .all(|(p, a)| name(a, p.parameter.name.as_str()))
                {
                    out.push(Finding::new("no-redundant-lambda", lambda.range(), format!("The lambda that wraps {} is redundant and can be replaced by the callable.", function.id)));
                }
            }
            (Self::ListComprehension | Self::Comprehension, AnyNodeRef::ExprCall(call)) => {
                let Expr::Name(function) = &*call.func else {
                    return;
                };
                let func = function.id.as_str();
                let args = call
                    .arguments
                    .iter_source_order()
                    .map(|a| unstar(a.value()))
                    .collect::<Vec<_>>();
                let [argument] = args.as_slice() else {
                    return;
                };
                if !builtin_origin(&call.func, c, func) {
                    return;
                }
                if matches!(self, Self::ListComprehension) {
                    if matches!(func, "any" | "all") && argument.is_list_comp_expr() {
                        out.push(Finding::new("no-redundant-list-comprehension", call.range(), format!("Unnecessary list comprehension inside {func}(). Use a generator expression instead.")));
                    }
                } else if matches!(func, "list" | "set" | "dict") {
                    let (elt, list) = match argument {
                        Expr::Generator(g) => (&*g.elt, false),
                        Expr::ListComp(g) => (&*g.elt, true),
                        _ => return,
                    };
                    if func == "dict"
                        && !pair(elt).is_some_and(|v| v.iter().all(|e| !e.is_starred_expr()))
                    {
                        return;
                    }
                    let message = if list {
                        format!(
                            "Replace the list comprehension inside {func}() with the equivalent comprehension."
                        )
                    } else {
                        format!("Replace this {func}() call with the equivalent comprehension.")
                    };
                    out.push(Finding::new("use-comprehension", call.range(), message));
                }
            }
            (Self::Optional, _) => {
                let annotation = match node {
                    AnyNodeRef::Parameter(p) => p.annotation.as_deref(),
                    AnyNodeRef::StmtFunctionDef(f) => f.returns.as_deref(),
                    AnyNodeRef::StmtAnnAssign(a) => Some(&*a.annotation),
                    _ => None,
                };
                let Some(Expr::Subscript(subscript)) = annotation else {
                    return;
                };
                if name(&subscript.value, "Union")
                    && pair(&subscript.slice)
                        .is_some_and(|p| p.iter().any(ruff_python_ast::Expr::is_none_literal_expr))
                {
                    out.push(Finding::new(
                        "replace-union-with-optional",
                        subscript.range(),
                        "`Optional[T]` is preferred over `Union[T, None]` or `Union[None, T]`.",
                    ));
                }
            }
            _ => {}
        }
    }
}

fn or_operands<'a>(boolean: &'a ast::ExprBoolOp, out: &mut Vec<&'a Expr>) {
    for (index, value) in boolean.values.iter().enumerate() {
        if let Expr::BoolOp(b) = value {
            if index == 0 && b.op == ast::BoolOp::Or {
                or_operands(b, out);
                continue;
            }
        }
        out.push(value);
    }
}
fn unstar(expression: &Expr) -> &Expr {
    match expression {
        Expr::Starred(s) => &s.value,
        e => e,
    }
}
fn name(expression: &Expr, expected: &str) -> bool {
    matches!(expression, Expr::Name(n) if n.id == expected)
}
fn builtin_origin(expression: &Expr, c: &Context<'_, '_, '_>, expected: &str) -> bool {
    matches!(expression, Expr::Name(n) if n.id == expected && c.binding_range(n).is_none_or(ruff_text_size::TextRange::is_empty))
        && c.qualified_name(expression).as_deref() == Some(format!("builtins.{expected}").as_str())
}
fn dotted(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Name(n) => Some(n.id.to_string()),
        Expr::Attribute(a) => Some(format!("{}.{}", dotted(&a.value)?, a.attr)),
        _ => None,
    }
}
fn pair(expression: &Expr) -> Option<&[Expr]> {
    let elts = match expression {
        Expr::Tuple(t) => &*t.elts,
        Expr::List(l) => &*l.elts,
        _ => return None,
    };
    (elts.len() == 2).then_some(elts)
}
fn all_nodes(node: AnyNodeRef<'_>) -> Vec<AnyNodeRef<'_>> {
    struct Collector<'a>(Vec<AnyNodeRef<'a>>);
    impl<'a> SourceOrderVisitor<'a> for Collector<'a> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            self.0.push(node);
            TraversalSignal::Traverse
        }
    }
    let mut collector = Collector(Vec::new());
    walk_node(&mut collector, node);
    collector.0
}
fn has_comments(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' => return true,
            quote @ (b'\'' | b'"') => {
                let triple = bytes
                    .get(i..i + 3)
                    .is_some_and(|s| s == [quote, quote, quote]);
                let width = if triple { 3 } else { 1 };
                i += width;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes
                        .get(i..i + width)
                        .is_some_and(|s| s.iter().all(|b| *b == quote))
                    {
                        i += width;
                        break;
                    }
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    false
}
fn fixed(
    rule: &'static str,
    range: TextRange,
    message: impl Into<String>,
    replacement: String,
) -> Finding {
    Finding::new(rule, range, message).with_fix(
        Fix::safe_edit(Edit::range_replacement(replacement, range)),
        "Apply recommended replacement",
    )
}

// Find the expression assigned to the semantic binding, not merely the last
// textual assignment with the same spelling (which may be in another scope).
fn assigned_value<'a>(n: &ast::ExprName, c: &Context<'_, 'a, '_>) -> Option<&'a Expr> {
    let binding = c.binding_range(n)?;
    all_nodes(c.module.into())
        .into_iter()
        .find_map(|node| match node {
            AnyNodeRef::StmtAssign(a)
                if a.targets.iter().any(|t| t.range().contains_range(binding)) =>
            {
                Some(&*a.value)
            }
            AnyNodeRef::StmtAnnAssign(a) if a.target.range().contains_range(binding) => {
                a.value.as_deref()
            }
            AnyNodeRef::ExprNamed(n) if n.target.range().contains_range(binding) => Some(&*n.value),
            _ => None,
        })
}
