use super::{
    AnyNodeRef, Context, Edit, Expr, Finding, Fix, LintRule, Ranged, Result, RuleOptions,
    SourceOrderVisitor, TextRange, TextSize, TraversalSignal, all_nodes, assigned_value, ast,
    reject_options, walk_node,
};

#[derive(Debug)]
enum Scopes {
    Sleep,
    Classmethod,
}
pub(super) fn sleep(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Scopes::Sleep))
}
pub(super) fn classmethod(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Scopes::Classmethod))
}

fn time_sleep(expression: &Expr, c: &Context<'_, '_, '_>, seen: &mut Vec<TextRange>) -> bool {
    if c.qualified_name(expression).as_deref() == Some("time.sleep") {
        return true;
    }
    let Expr::Name(n) = expression else {
        return false;
    };
    if let Some(binding) = c.binding_range(n) {
        if seen.contains(&binding) {
            return false;
        }
        seen.push(binding);
        let result = assigned_value(n, c).is_some_and(|e| time_sleep(e, c, seen));
        seen.pop();
        result
    } else {
        n.id == "sleep" && !shadows_sleep(c) && all_nodes(c.module.into()).iter().any(|n| matches!(n, AnyNodeRef::StmtImportFrom(i) if i.start() < expression.start() && i.module.as_ref().is_some_and(|m| m == "time") && i.names.iter().any(|a| a.name.as_str() == "*")))
    }
}
fn shadows_sleep(c: &Context<'_, '_, '_>) -> bool {
    struct Bindings(bool);
    impl<'a> SourceOrderVisitor<'a> for Bindings {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            match node {
                AnyNodeRef::StmtFunctionDef(f) => {
                    self.0 |= f.name.as_str() == "sleep";
                    return TraversalSignal::Skip;
                }
                AnyNodeRef::StmtClassDef(f) => {
                    self.0 |= f.name.as_str() == "sleep";
                    return TraversalSignal::Skip;
                }
                AnyNodeRef::ExprLambda(_)
                | AnyNodeRef::ExprListComp(_)
                | AnyNodeRef::ExprSetComp(_)
                | AnyNodeRef::ExprDictComp(_)
                | AnyNodeRef::ExprGenerator(_) => return TraversalSignal::Skip,
                AnyNodeRef::ExprName(n) if n.id == "sleep" && n.ctx.is_store() => self.0 = true,
                AnyNodeRef::StmtImport(i) => {
                    self.0 |= i
                        .names
                        .iter()
                        .any(|a| a.asname.as_ref().unwrap_or(&a.name) == "sleep")
                }
                AnyNodeRef::StmtImportFrom(i) => {
                    self.0 |= i
                        .names
                        .iter()
                        .any(|a| a.asname.as_ref().unwrap_or(&a.name) == "sleep")
                }
                _ => {}
            }
            TraversalSignal::Traverse
        }
    }
    let mut bindings = Bindings(false);
    if let Some(f) = c.enclosing_function() {
        if f.parameters.includes("sleep") {
            return true;
        }
        for statement in &f.body {
            walk_node(&mut bindings, statement.into());
        }
    } else {
        for statement in &c.module.body {
            walk_node(&mut bindings, statement.into());
        }
    }
    bindings.0
}
impl LintRule for Scopes {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        match (self, node) {
            (Self::Sleep, AnyNodeRef::ExprCall(call)) => {
                if c.enclosing_function().is_some_and(|f| f.is_async)
                    && time_sleep(&call.func, c, &mut Vec::new())
                {
                    out.push(Finding::new("use-async-sleep-in-async-def", call.range(), "Do not call blocking time.sleep inside async functions; use asyncio.sleep or an async runtime sleep."));
                }
            }
            (Self::Classmethod, AnyNodeRef::StmtFunctionDef(function)) => {
                if !function.decorator_list.iter().any(|d| {
                    c.qualified_name(&d.expression).as_deref() == Some("builtins.classmethod")
                }) {
                    return;
                }
                let first = function
                    .parameters
                    .posonlyargs
                    .first()
                    .or_else(|| function.parameters.args.first());
                if first.is_some_and(|p| p.parameter.name.as_str() == "cls") {
                    return;
                }
                let range = first.map_or(function.name.range(), |p| p.parameter.name.range());
                let mut finding = Finding::new(
                    "use-cls-in-classmethod",
                    range,
                    "When using @classmethod, the first argument must be `cls`.",
                );
                if let Some(first) = first {
                    if let Some(fix) = rename(function, &first.parameter, c) {
                        finding = finding.with_fix(fix, "Rename classmethod receiver to cls");
                    }
                } else if !function.parameters.iter().any(|p| p.name() == "cls") {
                    let start = function.parameters.start() + TextSize::new(1);
                    let suffix = if function.parameters.is_empty() {
                        ""
                    } else {
                        ", "
                    };
                    finding = finding.with_fix(
                        Fix::unsafe_edit(Edit::insertion(format!("cls{suffix}"), start)),
                        "Add cls parameter",
                    );
                }
                out.push(finding);
            }
            _ => {}
        }
    }
}

fn rename(
    function: &ast::StmtFunctionDef,
    parameter: &ast::Parameter,
    c: &Context<'_, '_, '_>,
) -> Option<Fix> {
    let old = parameter.name.as_str();
    let nodes = all_nodes(function.into());
    if function.parameters.iter().any(|p| p.name() == "cls") {
        return None;
    }
    for node in &nodes {
        match node {
            AnyNodeRef::ExprCall(call) if matches!(&*call.func, Expr::Name(n) if matches!(n.id.as_str(), "eval" | "exec" | "locals" | "vars")) =>
            {
                return None;
            }
            AnyNodeRef::StmtGlobal(g) if g.names.iter().any(|n| n == "cls") => return None,
            AnyNodeRef::StmtNonlocal(g) if g.names.iter().any(|n| n == "cls" || n == old) => {
                return None;
            }
            // An existing cls reference or binding could change lookup after the
            // rename; do not guess whether a dynamic caller can observe it.
            AnyNodeRef::ExprName(n) if n.id == "cls" => return None,
            _ => {}
        }
    }
    struct Bindings<'a> {
        old: &'a str,
        ranges: Vec<TextRange>,
        unsafe_binding: bool,
    }
    impl<'a> SourceOrderVisitor<'a> for Bindings<'_> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            match node {
                AnyNodeRef::StmtFunctionDef(f) => {
                    self.unsafe_binding |= f.name.as_str() == self.old;
                    return TraversalSignal::Skip;
                }
                AnyNodeRef::StmtClassDef(f) => {
                    self.unsafe_binding |= f.name.as_str() == self.old;
                    return TraversalSignal::Skip;
                }
                AnyNodeRef::ExprLambda(_)
                | AnyNodeRef::ExprListComp(_)
                | AnyNodeRef::ExprSetComp(_)
                | AnyNodeRef::ExprDictComp(_)
                | AnyNodeRef::ExprGenerator(_) => return TraversalSignal::Skip,
                AnyNodeRef::StmtImport(i) => {
                    self.unsafe_binding |= i.names.iter().any(|a| {
                        a.asname.as_ref().map_or_else(
                            || a.name.as_str().split('.').next().unwrap_or(""),
                            |n| n.as_str(),
                        ) == self.old
                    });
                }
                AnyNodeRef::StmtImportFrom(i) => {
                    self.unsafe_binding |= i
                        .names
                        .iter()
                        .any(|a| a.asname.as_ref().unwrap_or(&a.name) == self.old);
                }
                AnyNodeRef::ExceptHandlerExceptHandler(e) => {
                    self.unsafe_binding |= e.name.as_ref().is_some_and(|n| n == self.old);
                }
                AnyNodeRef::ExprName(n) if n.id == self.old && n.ctx.is_store() => {
                    self.ranges.push(n.range())
                }
                _ => {}
            }
            TraversalSignal::Traverse
        }
    }
    let mut bindings = Bindings {
        old,
        ranges: vec![parameter.name.range()],
        unsafe_binding: false,
    };
    for statement in &function.body {
        walk_node(&mut bindings, statement.into());
    }
    if bindings.unsafe_binding {
        return None;
    }
    let mut edits = vec![Edit::range_replacement(
        "cls".into(),
        parameter.name.range(),
    )];
    for node in nodes {
        let AnyNodeRef::ExprName(n) = node else {
            continue;
        };
        if n.id != old {
            continue;
        }
        let belongs = bindings.ranges.contains(&n.range())
            || c.binding_range(n).is_some_and(|r| {
                bindings.ranges.contains(&r) || parameter.range().contains_range(r)
            });
        if belongs {
            edits.push(Edit::range_replacement("cls".into(), n.range()));
        }
    }
    let mut edits = edits.into_iter();
    Some(Fix::unsafe_edits(edits.next()?, edits))
}
