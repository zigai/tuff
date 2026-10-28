use super::{
    AnyNodeRef, Context, Edit, Expr, Finding, Fix, LintRule, Ranged, Result, RuleOptions,
    TextRange, all_nodes, assigned_value, ast, fixed, has_comments, reject_options,
};
use ruff_python_semantic::ScopeKind;
use serde::Deserialize;

use super::super::typing;

#[derive(Debug)]
enum Strings {
    Redundant,
    Annotation,
    FString { max_length: i64 },
}
pub(super) fn redundant(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Strings::Redundant))
}
pub(super) fn annotation(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Strings::Annotation))
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    simple_expression_max_length: i64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            simple_expression_max_length: 30,
        }
    }
}
pub(super) fn fstring(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let options: Options = crate::parse_options(o)?;
    Ok(Box::new(Strings::FString {
        max_length: options.simple_expression_max_length,
    }))
}

fn quote_parts(text: &str) -> Option<(&str, &str, &str)> {
    let start = text.find(['\'', '"'])?;
    let rest = &text[start..];
    let width = if rest.starts_with("\"\"\"") || rest.starts_with("'''") {
        3
    } else {
        1
    };
    if text.len() < start + 2 * width {
        return None;
    }
    let quote = &text[start..start + width];
    Some((
        &text[..start],
        quote,
        &text[start + width..text.len() - width],
    ))
}
fn first_expression(c: &Context<'_, '_, '_>, expr: &ast::ExprFString) -> bool {
    let Some(AnyNodeRef::StmtExpr(statement)) = c.ancestors.last() else {
        return false;
    };
    if statement.value.range() != expr.range() {
        return false;
    }
    let start = statement.start();
    for parent in c.ancestors.iter().rev().skip(1) {
        let first = match parent {
            AnyNodeRef::ModModule(m) => m.body.first(),
            AnyNodeRef::StmtFunctionDef(f) => f.body.first(),
            AnyNodeRef::StmtClassDef(f) => f.body.first(),
            AnyNodeRef::StmtIf(s) => s.body.first(),
            AnyNodeRef::ElifElseClause(s) => s.body.first(),
            AnyNodeRef::StmtWith(s) => s.body.first(),
            AnyNodeRef::StmtFor(s) => [&*s.body, &*s.orelse]
                .into_iter()
                .find(|body| body.iter().any(|s| s.start() == start))
                .and_then(|body| body.first()),
            AnyNodeRef::StmtWhile(s) => [&*s.body, &*s.orelse]
                .into_iter()
                .find(|body| body.iter().any(|s| s.start() == start))
                .and_then(|body| body.first()),
            AnyNodeRef::StmtTry(s) => [&*s.body, &*s.orelse, &*s.finalbody]
                .into_iter()
                .find(|body| body.iter().any(|s| s.start() == start))
                .and_then(|body| body.first()),
            AnyNodeRef::ExceptHandlerExceptHandler(s) => s.body.first(),
            AnyNodeRef::MatchCase(s) => s.body.first(),
            _ => continue,
        };
        // LibCST excludes one-line suites other than function and class suites.
        if !matches!(
            parent,
            AnyNodeRef::ModModule(_) | AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_)
        ) && !c
            .text(TextRange::new(c.line_start(start), start))
            .trim()
            .is_empty()
        {
            return false;
        }
        return first.is_some_and(|s| s.start() == start);
    }
    false
}
fn protected_annotation(c: &Context<'_, '_, '_>, range: TextRange) -> bool {
    for parent in c.ancestors {
        let AnyNodeRef::ExprSubscript(s) = parent else {
            continue;
        };
        if typing::imported(
            c,
            &s.value,
            &["typing.Literal", "typing_extensions.Literal"],
            &["typing", "typing_extensions"],
            "Literal",
        ) {
            return true;
        }
        if typing::imported(
            c,
            &s.value,
            &["typing.Annotated", "typing_extensions.Annotated"],
            &["typing", "typing_extensions"],
            "Annotated",
        ) && let Expr::Tuple(t) = &*s.slice
            && t.elts
                .iter()
                .skip(1)
                .any(|e| e.range().contains_range(range))
        {
            return true;
        }
    }
    false
}

impl LintRule for Strings {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        match (self, node) {
            (Self::Redundant, AnyNodeRef::ExprFString(expr)) => {
                let has_fields = expr.value.f_strings().any(|f| {
                    f.elements
                        .iter()
                        .any(ast::InterpolatedStringElement::is_interpolation)
                });
                if !has_fields && first_expression(c, expr) {
                    return;
                }
                for f in expr.value.f_strings() {
                    if f.elements
                        .iter()
                        .any(ast::InterpolatedStringElement::is_interpolation)
                    {
                        continue;
                    }
                    let Some((prefix, quote, inner)) = quote_parts(c.text(f.range())) else {
                        continue;
                    };
                    let mut replacement = format!(
                        "{}{}{}{}",
                        prefix.replace(['f', 'F'], ""),
                        quote,
                        inner.replace("{{", "{").replace("}}", "}"),
                        quote
                    );
                    if c.text(TextRange::up_to(f.start()))
                        .chars()
                        .next_back()
                        .is_some_and(|ch| replacement.starts_with(ch))
                    {
                        replacement.insert(0, ' ');
                    }
                    out.push(fixed(
                        "no-redundant-f-string",
                        f.range(),
                        "Remove the `f` prefix; this f-string has no replacement fields.",
                        replacement,
                    ));
                }
            }
            (
                Self::Annotation,
                AnyNodeRef::ExprStringLiteral(_) | AnyNodeRef::ExprBytesLiteral(_),
            ) => {
                if !c.in_annotation(node.range())
                    || c.ancestors
                        .iter()
                        .any(|n| matches!(n, AnyNodeRef::StmtTypeAlias(_)))
                {
                    return;
                }
                let future = c.module.body.iter().any(|s| matches!(s, ast::Stmt::ImportFrom(i) if i.module.as_ref().is_some_and(|m| m == "__future__") && i.names.iter().any(|n| n.name.as_str() == "annotations")));
                if !future || protected_annotation(c, node.range()) {
                    return;
                }
                let mut finding = Finding::new(
                    "no-string-type-annotation",
                    node.range(),
                    "Remove the quotes from this annotation; postponed evaluation is already enabled.",
                );
                if let AnyNodeRef::ExprStringLiteral(s) = node {
                    let value = s.value.to_str();
                    if !has_comments(value)
                        && !has_comments(c.text(node.range()))
                        && let Ok(parsed) = ruff_python_parser::parse_expression(value)
                    {
                        let replacement = if value.contains(['\n', '\r'])
                            || parsed.expr().is_tuple_expr() && !value.trim_start().starts_with('(')
                        {
                            format!("({value})")
                        } else {
                            value.to_string()
                        };
                        finding = finding.with_fix(
                            Fix::safe_edit(Edit::range_replacement(replacement, node.range())),
                            "Remove annotation quotes",
                        );
                    }
                }
                out.push(finding);
            }
            (Self::FString { .. }, AnyNodeRef::ExprCall(call)) => {
                let Expr::Attribute(a) = &*call.func else {
                    return;
                };
                if a.attr.as_str() == "format"
                    && matches!(&*a.value, Expr::StringLiteral(s) if !s.value.is_implicit_concatenated())
                    || a.attr.as_str() == "format"
                        && matches!(&*a.value, Expr::BytesLiteral(s) if !s.value.is_implicit_concatenated())
                {
                    out.push(Finding::new(
                        "use-f-string",
                        call.range(),
                        "Use an f-string instead of `%` formatting or `str.format()`.",
                    ));
                }
            }
            (Self::FString { max_length }, AnyNodeRef::ExprBinOp(binary))
                if binary.op == ast::Operator::Mod =>
            {
                let Expr::StringLiteral(s) = &*binary.left else {
                    return;
                };
                if s.value.is_implicit_concatenated() {
                    return;
                }
                let mut finding = Finding::new(
                    "use-f-string",
                    binary.range(),
                    "Use an f-string instead of `%` formatting or `str.format()`.",
                );
                if let Some(mut replacement) = percent_fix(binary, c, *max_length) {
                    if c.text(TextRange::up_to(binary.start()))
                        .chars()
                        .next_back()
                        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                    {
                        replacement.insert(0, ' ');
                    }
                    finding = finding.with_fix(
                        Fix::safe_edit(Edit::range_replacement(replacement, binary.range())),
                        "Convert to f-string",
                    );
                }
                out.push(finding);
            }
            _ => {}
        }
    }
}

fn known_non_tuple(e: &Expr, c: &Context<'_, '_, '_>, seen: &mut Vec<TextRange>) -> bool {
    match e {
        Expr::StringLiteral(s) => !s.value.is_implicit_concatenated(),
        Expr::BytesLiteral(s) => !s.value.is_implicit_concatenated(),
        Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::List(_)
        | Expr::Dict(_)
        | Expr::Set(_)
        | Expr::ListComp(_)
        | Expr::DictComp(_)
        | Expr::SetComp(_)
        | Expr::Generator(_)
        | Expr::Lambda(_) => true,
        Expr::Name(n) => {
            let Some(binding) = c.binding_range(n) else {
                return false;
            };
            if seen.contains(&binding) {
                return false;
            }
            seen.push(binding);
            // Multiple assignments to the binding must all be non-tuples. Follow
            // cycles conservatively rather than deriving a type from text alone.
            let result = assigned_value(n, c).is_some_and(|v| known_non_tuple(v, c, seen));
            seen.pop();
            result
        }
        Expr::Call(call) => {
            let Expr::Name(n) = &*call.func else {
                return false;
            };
            let Some(binding) = c.binding_range(n) else {
                return false;
            };
            let Some(semantic) = c.resolver.semantic_model() else {
                return false;
            };
            semantic.scopes.iter().any(|scope| matches!(scope.kind, ScopeKind::Class(class) if class.name.range() == binding && class.arguments.as_ref().is_none_or(|a| a.is_empty()) && class.decorator_list.is_empty() && !scope.has("__new__")))
        }
        _ => false,
    }
}
fn percent_fix(
    binary: &ast::ExprBinOp,
    c: &Context<'_, '_, '_>,
    max_length: i64,
) -> Option<String> {
    let (prefix, quote, raw) = quote_parts(c.text(binary.left.range()))?;
    let tokens = raw.split("%s").collect::<Vec<_>>();
    let count = tokens.len().checked_sub(1)?;
    if count == 0 || tokens.iter().any(|s| s.contains('%')) {
        return None;
    }
    let original = &*binary.right;
    let expressions = if let Expr::Tuple(t) = original {
        t.elts.iter().collect::<Vec<_>>()
    } else {
        vec![original]
    };
    let short = |e: &Expr| {
        i64::try_from(c.text(e.range()).chars().count()).is_ok_and(|len| len <= max_length)
    };
    if !(short(original) || original.is_tuple_expr() && expressions.iter().all(|e| short(e))) {
        return None;
    }
    if has_comments(c.text(original.range()))
        || expressions.iter().any(|e| e.is_starred_expr())
        || expressions.len() != count
    {
        return None;
    }
    if count == 1 && !original.is_tuple_expr() && !known_non_tuple(original, c, &mut Vec::new()) {
        return None;
    }
    if count > 1
        && !expressions.iter().all(|e| {
            matches!(
                e,
                Expr::NumberLiteral(_)
                    | Expr::StringLiteral(_)
                    | Expr::BytesLiteral(_)
                    | Expr::BooleanLiteral(_)
                    | Expr::NoneLiteral(_)
            )
        })
    {
        return None;
    }
    let mut result = format!(
        "f{}{quote}{}",
        prefix.replace(['u', 'U'], ""),
        tokens[0].replace('{', "{{").replace('}', "}}")
    );
    for (i, expr) in expressions.into_iter().enumerate() {
        let code = c.text(expr.range());
        if code.contains(['\\', '\n', '\r'])
            || all_nodes(expr.into())
                .iter()
                .any(|n| matches!(n, AnyNodeRef::ExprFString(_)))
        {
            return None;
        }
        let mut edits = Vec::new();
        for node in all_nodes(expr.into()) {
            let AnyNodeRef::ExprStringLiteral(s) = node else {
                continue;
            };
            for part in &s.value {
                let (p, q, raw) = quote_parts(c.text(part.range()))?;
                if q != quote {
                    continue;
                }
                let new_quote = ["'", "\"", "'''", "\"\"\""]
                    .into_iter()
                    .find(|candidate| *candidate != q && !raw.contains(candidate))?;
                edits.push((part.range(), format!("{p}{new_quote}{raw}{new_quote}")));
            }
        }
        let mut expression = code.to_string();
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start()));
        for (range, text) in edits {
            expression.replace_range(
                usize::from(range.start() - expr.start())..usize::from(range.end() - expr.start()),
                &text,
            );
        }
        if matches!(
            expr,
            Expr::Lambda(_)
                | Expr::Named(_)
                | Expr::Generator(_)
                | Expr::Yield(_)
                | Expr::YieldFrom(_)
        ) {
            expression = format!("({expression})");
        }
        result.push('{');
        result.push_str(&expression);
        result.push_str("!s}");
        result.push_str(&tokens[i + 1].replace('{', "{{").replace('}', "}}"));
    }
    result.push_str(quote);
    // Refuse transformations which aren't valid even on pre-PEP-701 Python.
    if ruff_python_parser::parse_expression(&result).is_err() {
        return None;
    }
    Some(result)
}
