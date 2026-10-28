use super::{
    AnyNodeRef, Context, Edit, Expr, Finding, Fix, LintRule, Ranged, Result, RuleOptions,
    TextRange, TextSize, ast, builtin_origin, dotted, has_comments, name, pair, reject_options,
};

#[derive(Debug)]
enum Collections {
    Literal,
    Abc,
}
pub(super) fn literal(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Collections::Literal))
}
pub(super) fn abc(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Collections::Abc))
}

const ABCS: &[&str] = &[
    "AsyncGenerator",
    "AsyncIterable",
    "AsyncIterator",
    "Awaitable",
    "Buffer",
    "ByteString",
    "Callable",
    "Collection",
    "Container",
    "Coroutine",
    "Generator",
    "Hashable",
    "ItemsView",
    "Iterable",
    "Iterator",
    "KeysView",
    "Mapping",
    "MappingView",
    "MutableMapping",
    "MutableSequence",
    "MutableSet",
    "Reversible",
    "Sequence",
    "Set",
    "Sized",
    "ValuesView",
];
fn abc_name(s: &str) -> bool {
    ABCS.contains(&s)
}
fn old_abc(expression: &Expr) -> Option<&str> {
    let Expr::Attribute(a) = expression else {
        return None;
    };
    (name(&a.value, "collections") && abc_name(a.attr.as_str())).then_some(a.attr.as_str())
}
fn handles_import_error(expression: &Expr) -> bool {
    if dotted(expression).is_some_and(|s| s.rsplit('.').next() == Some("ImportError")) {
        return true;
    }
    matches!(expression, Expr::Tuple(t) if t.elts.iter().any(|e| dotted(e).is_some_and(|s| s.rsplit('.').next() == Some("ImportError"))))
}
fn fallback(c: &Context<'_, '_, '_>) -> bool {
    c.ancestors
        .iter()
        .rev()
        .find_map(|n| match n {
            AnyNodeRef::ExceptHandlerExceptHandler(e) => {
                Some(e.type_.as_deref().is_some_and(handles_import_error))
            }
            _ => None,
        })
        .unwrap_or(false)
}

impl LintRule for Collections {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        if matches!(self, Self::Abc) && (c.python_version.major, c.python_version.minor) < (3, 3) {
            return;
        }
        match (self, node) {
            (Self::Literal, AnyNodeRef::ExprCall(call)) => literal_call(call, c, out),
            (Self::Abc, AnyNodeRef::StmtImportFrom(import)) => {
                if import.level != 0
                    || import.module.as_ref().is_none_or(|m| m != "collections")
                    || fallback(c)
                {
                    return;
                }
                let moved = import
                    .names
                    .iter()
                    .filter(|a| abc_name(a.name.as_str()))
                    .collect::<Vec<_>>();
                if moved.is_empty() {
                    return;
                }
                let mut finding = Finding::new(
                    "use-collections-abc",
                    import.range(),
                    "Import abstract base classes from `collections.abc`.",
                );
                if moved.len() == import.names.len() {
                    if let Some(module) = &import.module {
                        finding = finding.with_fix(
                            Fix::safe_edit(Edit::range_replacement(
                                "collections.abc".into(),
                                module.range(),
                            )),
                            "Import from collections.abc",
                        );
                    }
                } else if !has_comments(c.text(import.range())) {
                    let line = c.text(TextRange::new(
                        c.line_start(import.start()),
                        c.line_end(import.end()),
                    ));
                    let before =
                        c.text(TextRange::new(c.line_start(import.start()), import.start()));
                    let after = c.text(TextRange::new(import.end(), c.line_end(import.end())));
                    if !line.contains(';')
                        && before.trim().is_empty()
                        && (after.trim().is_empty() || after.trim_start().starts_with('#'))
                    {
                        let kept = import
                            .names
                            .iter()
                            .filter(|a| !abc_name(a.name.as_str()))
                            .map(|a| c.text(a.range()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let moved = moved
                            .iter()
                            .map(|a| c.text(a.range()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let replacement = format!(
                            "from collections import {kept}{}{}from collections.abc import {moved}",
                            c.line_ending(),
                            c.indentation(import.start())
                        );
                        finding = finding.with_fix(
                            Fix::safe_edit(Edit::range_replacement(replacement, import.range())),
                            "Split collections imports",
                        );
                    }
                }
                out.push(finding);
            }
            (Self::Abc, AnyNodeRef::StmtImport(import)) => {
                for alias in &import.names {
                    let Some(member) = alias.name.as_str().strip_prefix("collections.") else {
                        continue;
                    };
                    if abc_name(member) {
                        out.push(
                            Finding::new(
                                "use-collections-abc",
                                alias.range(),
                                "Import abstract base classes from `collections.abc`.",
                            )
                            .with_fix(
                                Fix::safe_edit(Edit::range_replacement(
                                    format!("collections.abc.{member}"),
                                    alias.name.range(),
                                )),
                                "Import from collections.abc",
                            ),
                        );
                    }
                }
            }
            (Self::Abc, AnyNodeRef::StmtClassDef(class)) => {
                let Some(args) = &class.arguments else {
                    return;
                };
                let all_args = args.iter_source_order().collect::<Vec<_>>();
                let mut edits = Vec::new();
                let mut ranges = Vec::new();
                let mut can_fix = true;
                for (index, arg) in all_args.iter().enumerate() {
                    let ast::ArgOrKeyword::Arg(base) = arg else {
                        continue;
                    };
                    let Some(member) = old_abc(base) else {
                        continue;
                    };
                    ranges.push(base.range());
                    let end = all_args
                        .get(index + 1)
                        .map_or(args.end() - TextSize::new(1), Ranged::start);
                    let separator = if index + 1 < all_args.len() { ", " } else { "" };
                    let tail_range = TextRange::new(base.end(), end);
                    // A CST replacement would discard this trivia. Retain the
                    // diagnostic but never silently drop a comment.
                    if has_comments(c.text(tail_range)) {
                        can_fix = false;
                    }
                    edits.push(Edit::range_replacement(
                        format!("collections.abc.{member}{separator}"),
                        TextRange::new(base.start(), end),
                    ));
                }
                // Every base diagnostic carries the same complete class fix.
                let fix = if can_fix && !edits.is_empty() {
                    let mut edits = edits.into_iter();
                    edits.next().map(|first| Fix::safe_edits(first, edits))
                } else {
                    None
                };
                for range in ranges {
                    let mut finding = Finding::new(
                        "use-collections-abc",
                        range,
                        "Import abstract base classes from `collections.abc`.",
                    );
                    if let Some(fix) = &fix {
                        finding =
                            finding.with_fix(fix.clone(), "Replace deprecated collections bases");
                    }
                    out.push(finding);
                }
            }
            _ => {}
        }
    }
}
fn literal_call(call: &ast::ExprCall, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
    let Expr::Name(function) = &*call.func else {
        return;
    };
    let func = function.id.as_str();
    if !matches!(func, "tuple" | "list" | "set" | "dict")
        || !builtin_origin(&call.func, c, func)
        || !call.arguments.keywords.is_empty()
    {
        return;
    }
    let elements = match &*call.arguments.args {
        [] if func != "set" => &[][..],
        [Expr::List(list)] => &*list.elts,
        [Expr::Tuple(tuple)] => &*tuple.elts,
        _ => return,
    };
    let replacement = if func == "dict" {
        let mut entries = Vec::new();
        for element in elements {
            let Some([key, value]) = pair(element) else {
                return;
            };
            if key.is_starred_expr() || value.is_starred_expr() {
                return;
            }
            entries.push(format!(
                "{}: {}",
                c.text(key.range()),
                c.text(value.range())
            ));
        }
        format!("{{{}}}", entries.join(", "))
    } else if elements.is_empty() {
        match func {
            "list" => "[]".to_string(),
            "tuple" => "()".to_string(),
            "set" => "set()".to_string(),
            _ => return,
        }
    } else {
        let (open, close) = match func {
            "list" => ('[', ']'),
            "tuple" => ('(', ')'),
            "set" => ('{', '}'),
            _ => return,
        };
        // Reuse commas and element trivia instead of reprinting expressions.
        let argument = &call.arguments.args[0];
        let text = c.text(argument.range());
        let content = if text.starts_with(['[', '(']) && text.ends_with([']', ')']) {
            &text[1..text.len() - 1]
        } else {
            text
        };
        let comma = if func == "tuple" && elements.len() == 1 && !content.trim_end().ends_with(',')
        {
            ","
        } else {
            ""
        };
        format!("{open}{content}{comma}{close}")
    };
    let message = if call.arguments.is_empty() {
        format!("Replace {func}() with the equivalent empty collection literal.")
    } else {
        format!("Replace this {func}() call with the equivalent collection literal.")
    };
    let mut finding = Finding::new("use-literal", call.range(), message);
    if !has_comments(c.text(call.range())) {
        finding = finding.with_fix(
            Fix::safe_edit(Edit::range_replacement(replacement, call.range())),
            "Replace constructor with literal",
        );
    }
    out.push(finding);
}
