use super::{
    AnyNodeRef, Context, Edit, Expr, Finding, Fix, LintRule, Ranged, Result, RuleOptions,
    TextRange, ast, has_comments, name, reject_options,
};

#[derive(Debug)]
enum Assertions {
    Deprecated,
    Equal,
    Membership,
    None,
}
pub(super) fn deprecated(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Assertions::Deprecated))
}
pub(super) fn equal(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Assertions::Equal))
}
pub(super) fn membership(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Assertions::Membership))
}
pub(super) fn none(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Assertions::None))
}

fn overridden(c: &Context<'_, '_, '_>, method: &str) -> bool {
    c.enclosing_class().is_some_and(|class| {
        class
            .body
            .iter()
            .any(|stmt| matches!(stmt, ast::Stmt::FunctionDef(f) if f.name.as_str() == method))
    })
}

impl LintRule for Assertions {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ExprCall(call) = node else {
            return;
        };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        let attr = attribute.attr.as_str();
        if matches!(self, Self::Deprecated) {
            let receiver = &*attribute.value;
            if !(matches!(receiver, Expr::Name(n) if matches!(n.id.as_str(), "self" | "case" | "cls"))
                || matches!(receiver, Expr::Call(s) if name(&s.func, "super") && s.arguments.is_empty()))
            {
                return;
            }
            let replacement = match attr {
                "assertEquals" => "assertEqual",
                "assertNotEquals" => "assertNotEqual",
                "assertAlmostEquals" => "assertAlmostEqual",
                "assertNotAlmostEquals" => "assertNotAlmostEqual",
                "assertRegexpMatches" => "assertRegex",
                "assertNotRegexpMatches" => "assertNotRegex",
                "assertRaisesRegexp" => "assertRaisesRegex",
                _ => return,
            };
            out.push(
                Finding::new(
                    "deprecated-unittest-asserts",
                    call.range(),
                    format!("`{attr}` is deprecated; use `{replacement}` instead."),
                )
                .with_fix(
                    Fix::unsafe_edit(Edit::range_replacement(
                        replacement.to_string(),
                        attribute.attr.range(),
                    )),
                    "Replace deprecated assertion",
                ),
            );
            return;
        }
        if !name(&attribute.value, "self")
            || !matches!(attr, "assertTrue" | "assertFalse")
            || overridden(c, attr)
        {
            return;
        }
        let args = call.arguments.iter_source_order().collect::<Vec<_>>();
        let Some(first) = args.first() else {
            return;
        };
        if matches!(self, Self::Equal)
            && (attr != "assertTrue"
                || !matches!(first, ast::ArgOrKeyword::Arg(e) if !e.is_starred_expr()))
        {
            return;
        }
        let value = match first.value() {
            Expr::Starred(s) => &*s.value,
            e => e,
        };
        let (comparison, negated) = match value {
            Expr::Compare(cmp) => (cmp, false),
            Expr::UnaryOp(u) if u.op == ast::UnaryOp::Not => match &*u.operand {
                Expr::Compare(cmp) => (cmp, true),
                _ => return,
            },
            _ => return,
        };
        let Some((left, operator, right)) = comparison.as_single() else {
            return;
        };
        let (rule, destination, message, only_left) = match self {
            Self::Equal => {
                let destination = match (operator, negated) {
                    (ast::CmpOp::Eq, false) | (ast::CmpOp::NotEq, true) => "assertEqual",
                    (ast::CmpOp::NotEq, false) | (ast::CmpOp::Eq, true) => "assertNotEqual",
                    _ => return,
                };
                (
                    "use-assert-equal",
                    destination,
                    "Use `assertEqual()` or `assertNotEqual()` instead of wrapping an equality comparison in `assertTrue()`.",
                    false,
                )
            }
            Self::Membership => {
                let destination = match (attr, operator, negated) {
                    ("assertTrue", ast::CmpOp::In, false) => "assertIn",
                    ("assertTrue", ast::CmpOp::In, true)
                    | ("assertTrue", ast::CmpOp::NotIn, false)
                    | ("assertFalse", ast::CmpOp::In, false) => "assertNotIn",
                    _ => return,
                };
                (
                    "use-assert-in",
                    destination,
                    "Use `assertIn()` or `assertNotIn()` for membership checks.",
                    false,
                )
            }
            Self::None => {
                if !right.is_none_literal_expr()
                    || !matches!(operator, ast::CmpOp::Is | ast::CmpOp::IsNot)
                {
                    return;
                }
                let negative = (attr == "assertFalse") ^ negated ^ (*operator == ast::CmpOp::IsNot);
                (
                    "use-assert-is-not-none",
                    if negative {
                        "assertIsNotNone"
                    } else {
                        "assertIsNone"
                    },
                    "Use `assertIsNone()` or `assertIsNotNone()` for `None` checks.",
                    true,
                )
            }
            Self::Deprecated => return,
        };
        if overridden(c, destination) {
            return;
        }
        let mut finding = Finding::new(rule, call.range(), message);
        // Comments adjacent to the first argument can lie outside its AST range.
        let no_comments = if matches!(self, Self::Equal) {
            let end = args.get(1).map_or(call.arguments.end(), Ranged::start);
            !has_comments(c.text(TextRange::new(call.arguments.start(), end)))
        } else {
            !has_comments(c.text(call.arguments.range()))
        };
        if no_comments && !(matches!(self, Self::Equal) && negated) {
            let mut replacement_args = vec![c.text(left.range()).to_string()];
            if !only_left {
                replacement_args.push(c.text(right.range()).to_string());
            }
            replacement_args.extend(
                args.iter()
                    .skip(1)
                    .map(|arg| c.text(arg.range()).to_string()),
            );
            let replacement = format!("self.{destination}({})", replacement_args.join(", "));
            finding = finding.with_fix(
                Fix::unsafe_edit(Edit::range_replacement(replacement, call.range())),
                "Use specific assertion",
            );
        }
        out.push(finding);
    }
}
