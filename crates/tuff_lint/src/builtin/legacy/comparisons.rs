use super::{
    AnyNodeRef, Context, Edit, Expr, Finding, Fix, LintRule, Ranged, Result, RuleOptions,
    TextRange, assigned_value, ast, has_comments, reject_options,
};
use ruff_python_ast::token::TokenKind;

#[derive(Debug)]
struct Comparisons {
    singletons: bool,
}
pub(super) fn primitives(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Comparisons { singletons: false }))
}
pub(super) fn singletons(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Comparisons { singletons: true }))
}

fn singleton(e: &Expr) -> bool {
    matches!(e, Expr::BooleanLiteral(_) | Expr::NoneLiteral(_))
}
fn number(e: &Expr, signed: bool) -> bool {
    e.is_number_literal_expr()
        || signed
            && matches!(e, Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::UAdd | ast::UnaryOp::USub) && u.operand.is_number_literal_expr())
}
fn boolean_number(left: &Expr, right: &Expr, signed: bool) -> bool {
    left.is_boolean_literal_expr() && number(right, signed)
        || right.is_boolean_literal_expr() && number(left, signed)
}
fn primitive(e: &Expr) -> bool {
    matches!(
        e,
        Expr::NumberLiteral(_) | Expr::StringLiteral(_) | Expr::BytesLiteral(_) | Expr::FString(_)
    )
}
fn known_literal(
    e: &Expr,
    c: &Context<'_, '_, '_>,
    allow_numbers: bool,
    seen: &mut Vec<TextRange>,
) -> bool {
    match e {
        Expr::NumberLiteral(_) => allow_numbers,
        Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::FString(_)
        | Expr::NoneLiteral(_)
        | Expr::BooleanLiteral(_) => true,
        Expr::List(list) => list
            .elts
            .iter()
            .all(|element| known_literal(element, c, allow_numbers, seen)),
        Expr::Tuple(tuple) => tuple
            .elts
            .iter()
            .all(|element| known_literal(element, c, allow_numbers, seen)),
        Expr::Set(set) => set
            .elts
            .iter()
            .all(|element| known_literal(element, c, allow_numbers, seen)),
        Expr::Dict(dict) => dict.items.iter().all(|item| {
            item.key
                .as_ref()
                .is_some_and(|key| known_literal(key, c, allow_numbers, seen))
                && known_literal(&item.value, c, allow_numbers, seen)
        }),
        Expr::Name(n) => {
            let Some(binding) = c.binding_range(n) else {
                return false;
            };
            if seen.contains(&binding) {
                return false;
            }
            seen.push(binding);
            let known =
                assigned_value(n, c).is_some_and(|v| known_literal(v, c, allow_numbers, seen));
            seen.pop();
            known
        }
        _ => false,
    }
}
impl LintRule for Comparisons {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ExprCompare(compare) = node else {
            return;
        };
        let mut edits = Vec::new();
        let mut safe = true;
        let mut fixable = true;
        let mut needs_report = false;
        let parsed = ruff_python_parser::parse_expression(c.text(compare.range())).ok();
        for (left, op, right) in compare.iter() {
            if boolean_number(left, right, self.singletons) {
                continue;
            }
            let replace = if self.singletons {
                matches!(op, ast::CmpOp::Eq | ast::CmpOp::NotEq)
                    && (singleton(left) || singleton(right))
            } else {
                matches!(op, ast::CmpOp::Is | ast::CmpOp::IsNot)
                    && (primitive(left) || primitive(right))
            };
            if !replace {
                continue;
            }
            needs_report = true;
            if self.singletons {
                let is_safe = singleton(left) && singleton(right) || {
                    let (one, other) = if singleton(left) {
                        (left, right)
                    } else {
                        (right, left)
                    };
                    known_literal(other, c, one.is_none_literal_expr(), &mut Vec::new())
                };
                safe &= is_safe;
            } else {
                safe &= known_literal(left, c, true, &mut Vec::new())
                    && known_literal(right, c, true, &mut Vec::new());
            }
            let Some(parsed) = &parsed else {
                fixable = false;
                continue;
            };
            let between = TextRange::new(
                left.end() - compare.start(),
                right.start() - compare.start(),
            );
            let operators = parsed
                .tokens()
                .in_range(between)
                .iter()
                .filter(|t| {
                    matches!(
                        t.kind(),
                        TokenKind::Is | TokenKind::Not | TokenKind::EqEqual | TokenKind::NotEqual
                    )
                })
                .collect::<Vec<_>>();
            let (Some(first), Some(last)) = (operators.first(), operators.last()) else {
                fixable = false;
                continue;
            };
            let range = TextRange::new(
                compare.start() + first.start(),
                compare.start() + last.end(),
            );
            if has_comments(c.text(range)) {
                fixable = false;
                continue;
            }
            let replacement = match op {
                ast::CmpOp::Is => "==",
                ast::CmpOp::IsNot => "!=",
                ast::CmpOp::Eq => "is",
                ast::CmpOp::NotEq => "is not",
                _ => continue,
            };
            let replacement = if self.singletons {
                let before = c.text(TextRange::new(left.end(), range.start()));
                let after = c.text(TextRange::new(range.end(), right.start()));
                format!(
                    "{}{replacement}{}",
                    if before.is_empty() { " " } else { "" },
                    if after.is_empty() { " " } else { "" }
                )
            } else {
                replacement.to_string()
            };
            edits.push(Edit::range_replacement(replacement, range));
        }
        if !needs_report {
            return;
        }
        let (rule, message) = if self.singletons {
            (
                "use-is-for-singletons",
                "Compare `None`, `True`, and `False` with `is` or `is not`.",
            )
        } else {
            (
                "use-eq-for-primitives",
                "Use `==` or `!=` for numeric and string values; `is` tests object identity.",
            )
        };
        let mut finding = Finding::new(rule, compare.range(), message);
        if fixable && (!self.singletons || safe) {
            let mut edits = edits.into_iter();
            if let Some(first) = edits.next() {
                finding = finding.with_fix(
                    if safe {
                        Fix::safe_edits(first, edits)
                    } else {
                        Fix::unsafe_edits(first, edits)
                    },
                    "Replace comparison operators",
                );
            }
        }
        out.push(finding);
    }
}
