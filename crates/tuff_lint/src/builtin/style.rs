use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use ruff_diagnostics::{Edit, Fix};
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_body, walk_stmt,
};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, NodeKind, Stmt};
use ruff_source_file::LineRanges;
use ruff_text_size::{Ranged, TextRange, TextSize};
use serde::Deserialize;

use crate::{
    Context, Finding, LintRule, RuleDefinition, RuleOptions, parse_options, reject_options,
};

pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! { name: "no-annotated-self", code: "TUF009", explanation: "Omit redundant instance self annotations.", node_kinds: &[NodeKind::StmtFunctionDef], checks_comments: false, fixable: true, create: annotated_self },
    crate::native_rule! { name: "no-exception-message-variables", code: "TUF010", explanation: "Inline throwaway exception messages.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: message_variables }.replacing(&["EM101", "EM102", "EM103"]),
    crate::native_rule! { name: "no-str-exception-translation", code: "TUF011", explanation: "Use fixed messages when translating caught exceptions.", node_kinds: &[NodeKind::StmtRaise], checks_comments: false, fixable: false, create: exception_translation },
    crate::native_rule! { name: "no-underscore-class", code: "TUF012", explanation: "Class names must not start with underscores.", node_kinds: &[NodeKind::StmtClassDef], checks_comments: false, fixable: false, create: underscore_class },
    crate::native_rule! { name: "public-method-order", code: "TUF013", explanation: "Place public methods before private helpers unless order is significant.", node_kinds: &[NodeKind::StmtClassDef], checks_comments: false, fixable: false, create: method_order },
    crate::native_rule! { name: "sorted-attributes", code: "TUF014", explanation: "Sort class assignment groups opted into by a docstring directive.", node_kinds: &[NodeKind::StmtClassDef], checks_comments: false, fixable: true, create: sorted_attributes },
    crate::native_rule! { name: "no-del-unused-argument", code: "TUF024", explanation: "Prefix unused arguments with an underscore instead of deleting them.", node_kinds: &[NodeKind::StmtFunctionDef], checks_comments: false, fixable: true, create: del_unused_argument },
    crate::native_rule! { name: "require-subprocess-run-timeout", code: "TUF025", explanation: "Pass an explicit timeout to subprocess.run.", node_kinds: &[NodeKind::ExprCall], checks_comments: false, fixable: false, create: subprocess_run_timeout },
    crate::native_rule! { name: "require-typed-empty-container-assignment", code: "TUF026", explanation: "Annotate names assigned empty lists, dicts, or sets.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: false, create: typed_empty_containers },
    crate::native_rule! { name: "no-boilerplate-docstrings", code: "TUF028", explanation: "Remove docstrings and argument entries that only restate names.", node_kinds: &[NodeKind::StmtFunctionDef, NodeKind::StmtClassDef], checks_comments: false, fixable: true, create: boilerplate_docstrings },
];

#[derive(Debug)]
enum Style {
    SelfAnnotation,
    MessageVariables,
    Translation,
    UnderscoreClass,
    Sorted,
}
fn annotated_self(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Style::SelfAnnotation))
}
fn message_variables(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Style::MessageVariables))
}
fn exception_translation(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Style::Translation))
}
fn underscore_class(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Style::UnderscoreClass))
}
fn sorted_attributes(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Style::Sorted))
}

impl LintRule for Style {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        match (self, node) {
            (Self::SelfAnnotation, AnyNodeRef::StmtFunctionDef(f)) => check_self(f, c, out),
            (Self::UnderscoreClass, AnyNodeRef::StmtClassDef(class))
                if class.name.starts_with('_') =>
            {
                out.push(Finding::new(
                    "no-underscore-class",
                    class.name.range(),
                    "Class names must not start with an underscore prefix.",
                ));
            }
            (Self::Sorted, AnyNodeRef::StmtClassDef(class)) => check_sorted(class, c, out),
            (Self::MessageVariables, AnyNodeRef::ModModule(module)) => {
                check_messages(module, c, out);
            }
            (Self::Translation, AnyNodeRef::StmtRaise(raise)) => check_translation(raise, c, out),
            _ => {}
        }
    }
}

pub(crate) fn is_name(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Name(n) if n.id == name)
}
pub(crate) fn sequence(e: &Expr) -> Option<&[Expr]> {
    match e {
        Expr::List(n) => Some(&n.elts),
        Expr::Tuple(n) => Some(&n.elts),
        Expr::Set(n) => Some(&n.elts),
        _ => None,
    }
}
pub(crate) fn targets(e: &Expr) -> Vec<&ast::ExprName> {
    match e {
        Expr::Name(n) => vec![n],
        Expr::Tuple(_) | Expr::List(_) => sequence(e)
            .into_iter()
            .flatten()
            .flat_map(targets)
            .collect(),
        Expr::Starred(n) => targets(&n.value),
        _ => vec![],
    }
}
pub(crate) fn static_literal(e: &Expr) -> bool {
    match e {
        Expr::NumberLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => true,
        Expr::UnaryOp(n) => {
            matches!(n.op, ast::UnaryOp::UAdd | ast::UnaryOp::USub)
                && matches!(&*n.operand, Expr::NumberLiteral(_))
        }
        Expr::List(_) | Expr::Set(_) | Expr::Tuple(_) => {
            sequence(e).is_some_and(|items| items.iter().all(static_literal))
        }
        Expr::Dict(n) => n.items.iter().all(|item| {
            item.key.as_ref().is_some_and(static_literal) && static_literal(&item.value)
        }),
        _ => false,
    }
}
pub(crate) fn full_end(c: &Context<'_, '_, '_>, offset: TextSize) -> TextSize {
    c.source.full_line_end(offset)
}
pub(crate) fn simple_line(c: &Context<'_, '_, '_>, stmt: &Stmt) -> bool {
    c.text(TextRange::new(c.line_start(stmt.start()), stmt.start()))
        .trim()
        .is_empty()
        && {
            let tail = c
                .text(TextRange::new(stmt.end(), c.line_end(stmt.end())))
                .trim();
            let tail = tail.strip_prefix(';').unwrap_or(tail).trim_start();
            tail.is_empty() || tail.starts_with('#')
        }
}
pub(crate) fn has_comment(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'#' {
            return true;
        }
        if matches!(ch, b'\'' | b'"') {
            let triple = bytes.get(i..i + 3).is_some_and(|s| s == [ch, ch, ch]);
            let width = if triple { 3 } else { 1 };
            i += width;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes
                    .get(i..i + width)
                    .is_some_and(|s| s.iter().all(|b| *b == ch))
                {
                    i += width;
                    break;
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    false
}
fn callable_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(n) => Some(n.id.to_string()),
        Expr::Attribute(n) => Some(
            callable_name(&n.value)
                .map_or_else(|| n.attr.to_string(), |base| format!("{base}.{}", n.attr)),
        ),
        Expr::Call(n) => callable_name(&n.func),
        Expr::Subscript(n) => callable_name(&n.value),
        _ => None,
    }
}
fn callable_expr(e: &Expr) -> &Expr {
    match e {
        Expr::Call(n) => callable_expr(&n.func),
        Expr::Subscript(n) => callable_expr(&n.value),
        _ => e,
    }
}
fn resolved_tail(e: &Expr, c: &Context<'_, '_, '_>, tails: &[&str]) -> bool {
    c.qualified_name(callable_expr(e))
        .is_some_and(|name| tails.contains(&name.rsplit('.').next().unwrap_or("")))
}

fn annotation_names(e: &Expr, c: &Context<'_, '_, '_>) -> HashSet<String> {
    struct Names(HashSet<String>);
    impl<'a> SourceOrderVisitor<'a> for Names {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            match node {
                AnyNodeRef::ExprName(n) => {
                    self.0.insert(n.id.to_string());
                }
                AnyNodeRef::ExprAttribute(n) => {
                    self.0.insert(n.attr.to_string());
                }
                _ => {}
            }
            TraversalSignal::Traverse
        }
    }
    let simple_string = match e {
        Expr::StringLiteral(s) => Some(s.as_single_part_string().is_some()),
        Expr::BytesLiteral(s) => Some(s.as_single_part_bytestring().is_some()),
        _ => None,
    };
    if let Some(single) = simple_string {
        return if single {
            HashSet::from([c.text(e.range()).trim_matches(['\'', '"']).to_string()])
        } else {
            HashSet::new()
        };
    }
    let mut names = Names(HashSet::new());
    names.visit_expr(e);
    names.0
}
fn check_self(f: &ast::StmtFunctionDef, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
    if !c.in_class_body()
        || f.name.as_str() == "__new__"
        || f.decorator_list.iter().any(|d| {
            c.qualified_name(&d.expression).is_some_and(|n| {
                matches!(n.as_str(), "builtins.staticmethod" | "builtins.classmethod")
            })
        })
    {
        return;
    }
    let Some(p) = f
        .parameters
        .posonlyargs
        .first()
        .or_else(|| f.parameters.args.first())
    else {
        return;
    };
    if p.parameter.name.as_str() != "self" {
        return;
    }
    let Some(annotation) = p.parameter.annotation.as_deref() else {
        return;
    };
    if matches!(annotation, Expr::Subscript(_)) {
        return;
    }
    let names = annotation_names(annotation, c);
    let class_annotation = c
        .enclosing_class()
        .is_some_and(|class| names.contains(class.name.as_str()) || names.contains("Self"));
    if !class_annotation
        && f.returns
            .as_deref()
            .is_some_and(|ret| !names.is_disjoint(&annotation_names(ret, c)))
    {
        return;
    }
    let mut finding = Finding::new(
        "no-annotated-self",
        p.parameter.name.range(),
        "Do not annotate self in instance methods.",
    );
    let range = TextRange::new(p.parameter.name.end(), p.parameter.end());
    if !has_comment(c.text(range)) {
        let safe = deferred_annotations(c)
            || annotation.is_literal_expr()
            || matches!(annotation, Expr::Name(_) if c.qualified_name(annotation).is_some_and(|name| name.starts_with("builtins.")));
        finding = finding.with_fix(
            if safe {
                Fix::safe_edit(Edit::range_deletion(range))
            } else {
                Fix::unsafe_edit(Edit::range_deletion(range))
            },
            "Remove self annotation",
        );
    }
    out.push(finding);
}

fn deferred_annotations(c: &Context<'_, '_, '_>) -> bool {
    c.python_version >= ast::PythonVersion::PY314 || c.module.body.iter().any(|statement| {
        matches!(statement, Stmt::ImportFrom(import) if import.level == 0 && import.module.as_ref().is_some_and(|module| module == "__future__") && import.names.iter().any(|alias| alias.name.as_str() == "annotations"))
    })
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MethodOptions {
    class_name_patterns: Vec<String>,
    excluded_class_name_patterns: Vec<String>,
}
impl Default for MethodOptions {
    fn default() -> Self {
        Self {
            class_name_patterns: vec!["*".into()],
            excluded_class_name_patterns: [
                "*Enum",
                "*Model",
                "*Record",
                "*Schema",
                "*Settings",
                "*Table",
            ]
            .map(str::to_string)
            .to_vec(),
        }
    }
}
#[derive(Debug)]
struct MethodOrder {
    include: Vec<String>,
    exclude: Vec<String>,
}
fn method_order(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let opts: MethodOptions = parse_options(o)?;
    Ok(Box::new(MethodOrder {
        include: opts.class_name_patterns,
        exclude: opts.excluded_class_name_patterns,
    }))
}
impl LintRule for MethodOrder {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        struct TypingStar {
            before: TextSize,
            found: bool,
        }
        impl<'a> SourceOrderVisitor<'a> for TypingStar {
            fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                if node.start() >= self.before {
                    return TraversalSignal::Skip;
                }
                if let AnyNodeRef::StmtImportFrom(import) = node {
                    self.found |= import.level == 0
                        && import
                            .module
                            .as_ref()
                            .is_some_and(|m| matches!(m.as_str(), "typing" | "typing_extensions"))
                        && import.names.iter().any(|n| n.name.as_str() == "*");
                }
                TraversalSignal::Traverse
            }
        }
        let AnyNodeRef::StmtClassDef(class) = node else {
            return;
        };
        if !self.include.iter().any(|p| fnmatch(p, class.name.as_str()))
            || self.exclude.iter().any(|p| fnmatch(p, class.name.as_str()))
        {
            return;
        }
        let bases = [
            "BaseModel",
            "Enum",
            "GenericModel",
            "IntEnum",
            "Protocol",
            "SQLModel",
            "StrEnum",
            "TableBase",
            "TypedDict",
        ];
        if class.arguments.as_ref().is_some_and(|args| {
            args.args.iter().any(|base| {
                callable_name(base)
                    .is_some_and(|n| bases.contains(&n.rsplit('.').next().unwrap_or("")))
                    || resolved_tail(base, c, &bases)
            })
        }) {
            return;
        }
        let decorators = [
            "attr.define",
            "attr.frozen",
            "attrs.define",
            "attrs.frozen",
            "dataclass",
            "dataclasses.dataclass",
            "define",
            "frozen",
            "pydantic.dataclasses.dataclass",
        ];
        if class.decorator_list.iter().any(|d| {
            callable_name(&d.expression).is_some_and(|n| decorators.contains(&n.as_str()))
                || resolved_tail(&d.expression, c, &["define", "frozen", "dataclass"])
        }) {
            return;
        }
        let methods: Vec<_> = class
            .body
            .iter()
            .filter_map(|s| {
                if let Stmt::FunctionDef(f) = s {
                    Some(f)
                } else {
                    None
                }
            })
            .collect();
        let mut typing_star = TypingStar {
            before: class.start(),
            found: false,
        };
        AnyNodeRef::ModModule(c.module).visit_source_order(&mut typing_star);
        let star = typing_star.found;
        let overloads: HashSet<_> = methods.iter().filter(|f| f.decorator_list.iter().any(|d| resolved_tail(&d.expression, c, &["overload"]) || (star && callable_name(&d.expression).as_deref() == Some("overload") && c.qualified_name(callable_expr(&d.expression)).is_none() && matches!(callable_expr(&d.expression), Expr::Name(n) if c.binding_range(n).is_none())))).map(|f| f.name.as_str()).collect();
        let mut private = None;
        let mut seen_overloads = HashSet::new();
        let mut seen_public = HashSet::new();
        for method in methods {
            let name = method.name.as_str();
            if name.starts_with("__") && name.ends_with("__") {
                continue;
            }
            if method.decorator_list.iter().any(|d| {
                callable_name(&d.expression)
                    .is_some_and(|n| n == "singledispatchmethod" || n.ends_with(".register"))
                    || c.qualified_name(callable_expr(&d.expression)).as_deref()
                        == Some("functools.singledispatchmethod")
            }) {
                continue;
            }
            if !name.starts_with('_')
                && seen_public.contains(name)
                && method.decorator_list.iter().any(|d| {
                    callable_name(&d.expression)
                        .is_some_and(|n| n.ends_with(".setter") || n.ends_with(".deleter"))
                        || resolved_tail(&d.expression, c, &["setter", "deleter"])
                })
            {
                continue;
            }
            if overloads.contains(name) {
                if !seen_overloads.insert(name) || name.starts_with('_') {
                    continue;
                }
            } else if name.starts_with('_') {
                if private.is_none() {
                    private = Some(name);
                }
                continue;
            } else {
                seen_public.insert(name);
            }
            if let Some(helper) = private {
                out.push(Finding::new("public-method-order", method.name.range(), format!("Define public methods before underscore-prefixed helper methods. Public method '{name}' appears after private helper '{helper}'.")));
                break;
            }
        }
    }
}

fn assignment(stmt: &Stmt) -> Option<(&ast::ExprName, Option<&Expr>)> {
    let (target, value) = match stmt {
        Stmt::Assign(s) if s.targets.len() == 1 => (&s.targets[0], Some(&*s.value)),
        Stmt::AnnAssign(s) => (&*s.target, s.value.as_deref()),
        _ => return None,
    };
    if let Expr::Name(name) = target {
        Some((name, value))
    } else {
        None
    }
}
fn check_sorted(class: &ast::StmtClassDef, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
    fn flush(
        group: &mut Vec<(&Stmt, &str, TextSize)>,
        c: &Context<'_, '_, '_>,
        edits: &mut Vec<Edit>,
        safe: &mut bool,
    ) {
        if group.len() < 2 {
            group.clear();
            return;
        }
        if group.windows(2).all(|w| w[0].1 <= w[1].1) {
            group.clear();
            return;
        }
        *safe &= group.iter().all(|(s, _, _)| {
            assignment(s).is_some_and(|(_, value)| value.is_none_or(static_literal))
                && match s {
                    Stmt::AnnAssign(statement) => match &*statement.annotation {
                        Expr::StringLiteral(_) | Expr::NoneLiteral(_) => true,
                        Expr::Name(_) => c
                            .qualified_name(&statement.annotation)
                            .is_some_and(|name| name.starts_with("builtins.")),
                        _ => false,
                    },
                    _ => true,
                }
        });
        let start = group[0].2;
        let end = full_end(c, group[group.len() - 1].0.end());
        let mut sorted = group.clone();
        sorted.sort_by_key(|(_, name, _)| *name);
        let first_prefix = c.text(TextRange::new(start, c.line_start(group[0].0.start())));
        let mut text: String = first_prefix
            .split_inclusive('\n')
            .filter(|line| line.trim().is_empty())
            .collect();
        for (s, _, leading) in sorted {
            let statement_start = c.line_start(s.start());
            let prefix = c.text(TextRange::new(leading, statement_start));
            text.extend(
                prefix
                    .split_inclusive('\n')
                    .filter(|line| !line.trim().is_empty()),
            );
            let statement = c.text(TextRange::new(statement_start, full_end(c, s.end())));
            text.push_str(statement);
            if !statement.ends_with('\n') {
                text.push_str(c.line_ending());
            }
        }
        if usize::from(end) == c.source.len() && !c.source.ends_with('\n') {
            text.truncate(text.len().saturating_sub(c.line_ending().len()));
        }
        edits.push(Edit::range_replacement(text, TextRange::new(start, end)));
        group.clear();
    }
    if !class.body.first().is_some_and(|s| matches!(s, Stmt::Expr(s) if matches!(&*s.value, Expr::StringLiteral(v) if v.value.to_string().contains("@sorted-attributes")))) { return; }
    let mut group: Vec<(&Stmt, &str, TextSize)> = Vec::new();
    let mut edits = Vec::new();
    let mut safe = true;
    let mut previous_end = class
        .body
        .first()
        .map_or(class.start(), |s| full_end(c, s.end()));
    for stmt in class.body.iter().skip(1) {
        let line_start = c.line_start(stmt.start());
        let gap = if previous_end <= line_start {
            c.text(TextRange::new(previous_end, line_start))
        } else {
            ""
        };
        let blank = gap.lines().any(|l| l.trim().is_empty());
        if blank {
            flush(&mut group, c, &mut edits, &mut safe);
        }
        let mut leading = previous_end.min(line_start);
        // Blank lines belong to the group boundary, comments belong to the assignment.
        for line in gap.split_inclusive('\n') {
            if line.trim().is_empty() {
                leading += TextSize::of(line);
            } else {
                break;
            }
        }
        if let Some((name, _)) =
            assignment(stmt).filter(|(n, _)| !(n.id.starts_with("__") && n.id.ends_with("__")))
        {
            if simple_line(c, stmt) {
                group.push((stmt, name.id.as_str(), leading));
            } else {
                flush(&mut group, c, &mut edits, &mut safe);
            }
        } else {
            flush(&mut group, c, &mut edits, &mut safe);
        }
        previous_end = full_end(c, stmt.end());
    }
    flush(&mut group, c, &mut edits, &mut safe);
    if edits.is_empty() {
        return;
    }
    let mut finding = Finding::new(
        "sorted-attributes",
        class.name.range(),
        "Class assignments under @sorted-attributes are not sorted; sorting them can change right-hand-side side-effect order.",
    );
    if safe {
        let mut iter = edits.into_iter();
        if let Some(first) = iter.next() {
            finding = finding.with_fix(Fix::safe_edits(first, iter), "Sort class attributes");
        }
    }
    out.push(finding);
}

fn only_message_reference(
    c: &Context<'_, '_, '_>,
    target: &ast::ExprName,
    reference: &ast::ExprName,
) -> bool {
    let Some(semantic) = c.resolver.semantic_model() else {
        return false;
    };
    let Some((binding_id, binding)) = semantic
        .bindings
        .iter_enumerated()
        .find(|(_, binding)| binding.range() == target.range())
    else {
        return false;
    };
    if binding.is_global() || binding.is_nonlocal() || binding.is_deleted() {
        return false;
    }
    // Ruff carries references forward when a name is rebound.
    let mut references = binding.references().filter(|id| {
        let reference = semantic.reference(*id);
        if !reference.is_load() {
            return true;
        }
        match reference
            .expression_id()
            .and_then(|id| semantic.expression(id))
        {
            Some(Expr::Name(name)) => semantic.resolve_name(name) == Some(binding_id),
            _ => true,
        }
    });
    references
        .next()
        .is_some_and(|id| semantic.reference(id).range() == reference.range())
        && references.next().is_none()
}
fn check_messages(module: &ast::ModModule, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
    struct Suites<'ctx, 'model, 'ast, 'src> {
        c: &'ctx Context<'model, 'ast, 'src>,
        out: &'ctx mut Vec<Finding>,
        module_start: TextSize,
    }
    impl<'a> SourceOrderVisitor<'a> for Suites<'_, '_, '_, '_> {
        fn visit_body(&mut self, body: &'a [Stmt]) {
            struct Count<'n> {
                name: &'n str,
                count: usize,
            }
            impl<'a> SourceOrderVisitor<'a> for Count<'_> {
                fn enter_node(&mut self, n: AnyNodeRef<'a>) -> TraversalSignal {
                    match n {
                        AnyNodeRef::ExprName(n) if n.id == self.name => self.count += 1,
                        AnyNodeRef::ExprAttribute(n) if n.attr.as_str() == self.name => {
                            self.count += 1;
                        }
                        _ => {}
                    }
                    TraversalSignal::Traverse
                }
            }
            for (index, pair) in body.windows(2).enumerate() {
                let statement = &pair[0];
                let Some((target, Some(value))) = assignment(statement) else {
                    continue;
                };
                if !simple_line(self.c, statement) {
                    continue;
                }
                let line = self.c.line_start(statement.start());
                let has_leading = if index > 0 {
                    let previous = full_end(self.c, body[index - 1].end());
                    previous < line
                } else if statement.start() == self.module_start {
                    false
                } else {
                    let before = &self.c.source[..usize::from(line)];
                    before
                        .lines()
                        .next_back()
                        .is_some_and(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
                };
                if has_leading {
                    continue;
                }
                if let Expr::Call(call) = value {
                    if callable_name(&call.func).is_some_and(|n| {
                        n.rsplit('.')
                            .next()
                            .is_some_and(|n| n.ends_with("Error") || n.ends_with("Exception"))
                    }) {
                        continue;
                    }
                }
                let Stmt::Raise(raise) = &pair[1] else {
                    continue;
                };
                if !simple_line(self.c, &pair[1]) {
                    continue;
                }
                let Some(Expr::Call(call)) = raise.exc.as_deref() else {
                    continue;
                };
                let arguments = call_arguments(call);
                let references: Vec<_> = arguments
                    .iter()
                    .filter_map(|(arg, _)| {
                        if let Expr::Name(n) = arg {
                            (n.id == target.id).then_some(n)
                        } else {
                            None
                        }
                    })
                    .collect();
                if references.len() != 1 {
                    continue;
                }
                let reference = references[0];
                let mut count = Count {
                    name: target.id.as_str(),
                    count: 0,
                };
                AnyNodeRef::StmtRaise(raise).visit_source_order(&mut count);
                if count.count != 1 {
                    continue;
                }
                // Rattle considers only the first structurally matching pair in a suite.
                if only_message_reference(self.c, target, reference) {
                    let mut finding = Finding::new(
                        "no-exception-message-variables",
                        target.range(),
                        "Inline exception message strings instead of assigning throwaway variables.",
                    );
                    let deletion = TextRange::new(line, full_end(self.c, statement.end()));
                    if static_literal(value) && !has_comment(self.c.text(deletion)) {
                        let before_value = self.c.text(TextRange::new(target.end(), value.start()));
                        let value_start = before_value.rfind('=').map_or(value.start(), |i| {
                            target.end() + TextSize::of(&before_value[..=i])
                        });
                        let replacement = self
                            .c
                            .text(TextRange::new(value_start, statement.end()))
                            .trim()
                            .to_string();
                        let local_annotation = self
                            .c
                            .resolver
                            .semantic_model()
                            .zip(self.c.scope_at(statement.start()))
                            .is_some_and(|(semantic, scope)| {
                                matches!(
                                    semantic.scopes[scope].kind,
                                    ruff_python_semantic::ScopeKind::Function(_)
                                )
                            });
                        let safe = value.is_literal_expr()
                            && (!matches!(statement, Stmt::AnnAssign(_)) || local_annotation);
                        let first = Edit::range_deletion(deletion);
                        let rest = [Edit::range_replacement(replacement, reference.range())];
                        finding = finding.with_fix(
                            if safe {
                                Fix::safe_edits(first, rest)
                            } else {
                                Fix::unsafe_edits(first, rest)
                            },
                            "Inline exception message",
                        );
                    }
                    self.out.push(finding);
                }
                break;
            }
            ruff_python_ast::visitor::source_order::walk_body(self, body);
        }
    }
    let mut suites = Suites {
        c,
        out,
        module_start: module.body.first().map_or(TextSize::new(0), Ranged::start),
    };
    AnyNodeRef::ModModule(module).visit_source_order(&mut suites);
}

struct ExceptionNames {
    names: HashSet<String>,
    until: TextSize,
}
impl ExceptionNames {
    fn discard(&mut self, target: &Expr) {
        for name in targets(target) {
            self.names.remove(name.id.as_str());
        }
    }
    fn assign(&mut self, target: &Expr, value: &Expr) {
        if let Expr::Name(name) = target {
            if matches!(value, Expr::Name(n) if self.names.contains(n.id.as_str())) {
                self.names.insert(name.id.to_string());
            } else {
                self.names.remove(name.id.as_str());
            }
        } else {
            self.discard(target);
        }
    }
}
impl<'a> SourceOrderVisitor<'a> for ExceptionNames {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if node.start() >= self.until {
            return TraversalSignal::Skip;
        }
        match node {
            AnyNodeRef::StmtFunctionDef(f) => {
                self.names.remove(f.name.as_str());
                return TraversalSignal::Skip;
            }
            AnyNodeRef::StmtClassDef(f) => {
                self.names.remove(f.name.as_str());
                return TraversalSignal::Skip;
            }
            AnyNodeRef::ExceptHandlerExceptHandler(_) => return TraversalSignal::Skip,
            AnyNodeRef::StmtAssign(s) => {
                for target in &s.targets {
                    self.assign(target, &s.value);
                }
            }
            AnyNodeRef::StmtAnnAssign(s) => {
                if let Some(v) = &s.value {
                    self.assign(&s.target, v);
                }
            }
            AnyNodeRef::StmtAugAssign(s) => self.discard(&s.target),
            AnyNodeRef::StmtDelete(s) => {
                for target in &s.targets {
                    self.discard(target);
                }
            }
            AnyNodeRef::StmtFor(s) => self.discard(&s.target),
            AnyNodeRef::StmtWith(s) => {
                for item in &s.items {
                    if let Some(target) = &item.optional_vars {
                        self.discard(target);
                    }
                }
            }
            AnyNodeRef::StmtImport(s) => {
                for alias in &s.names {
                    self.names.remove(alias.asname.as_ref().map_or_else(
                        || alias.name.as_str().split('.').next().unwrap_or(""),
                        |n| n.as_str(),
                    ));
                }
            }
            AnyNodeRef::StmtImportFrom(s) => {
                for alias in &s.names {
                    self.names
                        .remove(alias.asname.as_ref().unwrap_or(&alias.name).as_str());
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }
}
fn check_translation(raise: &ast::StmtRaise, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
    fn exception_name(e: &Expr, names: &HashSet<String>) -> bool {
        matches!(e, Expr::Name(n) if names.contains(n.id.as_str()))
    }
    fn translates(e: &Expr, names: &HashSet<String>, c: &Context<'_, '_, '_>) -> bool {
        match e {
            Expr::Call(call) if call.arguments.args.len() + call.arguments.keywords.len() == 1 => {
                let arguments = call_arguments(call);
                let arg = arguments.first().map(|(arg, _)| *arg);
                let builtin = c.qualified_name(&call.func).as_deref() == Some("builtins.str");
                let format = matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "format" && string_template(&a.value));
                (builtin || format) && arg.is_some_and(|arg| exception_name(arg, names))
            }
            Expr::FString(f) => {
                if f.value.is_implicit_concatenated() {
                    return false;
                }
                let mut expressions = Vec::new();
                for element in f.value.elements() {
                    match element {
                        ast::InterpolatedStringElement::Literal(l) if !l.value.is_empty() => {
                            return false;
                        }
                        ast::InterpolatedStringElement::Interpolation(e) => {
                            expressions.push(&*e.expression);
                        }
                        ast::InterpolatedStringElement::Literal(_) => {}
                    }
                }
                expressions.len() == 1 && exception_name(expressions[0], names)
            }
            Expr::BinOp(b) if b.op == ast::Operator::Mod && string_template(&b.left) => {
                exception_name(&b.right, names)
                    || matches!(&*b.right, Expr::Tuple(t) if t.elts.len() == 1 && exception_name(&t.elts[0], names))
            }
            _ => false,
        }
    }
    let mut handler = None;
    for ancestor in c.ancestors.iter().rev() {
        match ancestor {
            AnyNodeRef::ExceptHandlerExceptHandler(h) => {
                handler = Some(*h);
                break;
            }
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => return,
            _ => {}
        }
    }
    let Some(handler) = handler else {
        return;
    };
    let mut tracker = ExceptionNames {
        names: handler.name.iter().map(ToString::to_string).collect(),
        until: raise.start(),
    };
    tracker.visit_body(&handler.body);
    let Some(Expr::Call(call)) = raise.exc.as_deref() else {
        return;
    };
    for (arg, _) in call_arguments(call) {
        if translates(arg, &tracker.names, c) {
            out.push(Finding::new("no-str-exception-translation", arg.range(), "Use a fixed message when translating an exception, and preserve the cause with `raise ... from e`."));
            break;
        }
    }
}

fn fnmatch(pattern: &str, value: &str) -> bool {
    fn matches(
        p: &[char],
        v: &[char],
        i: usize,
        j: usize,
        memo: &mut std::collections::HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(result) = memo.get(&(i, j)) {
            return *result;
        }
        let result = match p.get(i) {
            None => j == v.len(),
            Some('*') => {
                matches(p, v, i + 1, j, memo) || (j < v.len() && matches(p, v, i, j + 1, memo))
            }
            Some('?') => j < v.len() && matches(p, v, i + 1, j + 1, memo),
            Some('[') => {
                let negate = p.get(i + 1) == Some(&'!');
                let begin = i + 1 + usize::from(negate);
                let search = begin + usize::from(p.get(begin) == Some(&']'));
                if let Some(end) = p
                    .iter()
                    .enumerate()
                    .skip(search)
                    .find_map(|(index, ch)| (*ch == ']').then_some(index))
                {
                    let mut hit = false;
                    let mut index = begin;
                    if let Some(ch) = v.get(j) {
                        while index < end {
                            if index + 2 < end && p[index + 1] == '-' {
                                hit |= p[index] <= *ch && *ch <= p[index + 2];
                                index += 3;
                            } else {
                                hit |= p[index] == *ch;
                                index += 1;
                            }
                        }
                    }
                    j < v.len() && (hit != negate) && matches(p, v, end + 1, j + 1, memo)
                } else {
                    v.get(j) == Some(&'[') && matches(p, v, i + 1, j + 1, memo)
                }
            }
            Some(ch) => v.get(j) == Some(ch) && matches(p, v, i + 1, j + 1, memo),
        };
        memo.insert((i, j), result);
        result
    }
    let p: Vec<_> = pattern.chars().collect();
    let v: Vec<_> = value.chars().collect();
    let mut memo = std::collections::HashMap::new();
    matches(&p, &v, 0, 0, &mut memo)
}

pub(crate) fn call_arguments(call: &ast::ExprCall) -> Vec<(&Expr, bool)> {
    call.arguments
        .iter_source_order()
        .map(|argument| match argument {
            ast::ArgOrKeyword::Arg(Expr::Starred(starred)) => (&*starred.value, false),
            ast::ArgOrKeyword::Arg(expression) => (expression, false),
            ast::ArgOrKeyword::Keyword(keyword) => (&keyword.value, keyword.arg.is_some()),
        })
        .collect()
}

fn string_template(expression: &Expr) -> bool {
    matches!(expression, Expr::StringLiteral(_) | Expr::BytesLiteral(_))
        || matches!(expression, Expr::FString(string) if string.value.is_implicit_concatenated())
}

pub(crate) fn test_like_path(c: &Context<'_, '_, '_>, excluded_parts: &[impl AsRef<str>]) -> bool {
    let path = c.path.strip_prefix(c.root).unwrap_or(c.path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    name.starts_with("test_")
        || name.ends_with("_test.py")
        || path.components().any(|part| {
            excluded_parts
                .iter()
                .any(|excluded| part.as_os_str() == excluded.as_ref())
        })
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct DelUnusedArgument {
    excluded_path_parts: Vec<String>,
}
impl Default for DelUnusedArgument {
    fn default() -> Self {
        Self {
            excluded_path_parts: vec!["tests".into(), "benchmarks".into()],
        }
    }
}
fn del_unused_argument(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    Ok(Box::new(parse_options::<DelUnusedArgument>(o)?))
}
fn parameters(parameters: &ast::Parameters) -> impl Iterator<Item = &ast::Parameter> {
    parameters
        .posonlyargs
        .iter()
        .chain(&parameters.args)
        .chain(&parameters.kwonlyargs)
        .map(|parameter| &parameter.parameter)
        .chain(parameters.vararg.as_deref())
        .chain(parameters.kwarg.as_deref())
}
fn deleted_names<'a>(target: &'a Expr, names: &mut Vec<&'a str>) {
    match target {
        Expr::Name(name) => names.push(name.id.as_str()),
        Expr::Tuple(ast::ExprTuple { elts, .. }) | Expr::List(ast::ExprList { elts, .. }) => {
            for element in elts {
                deleted_names(element, names);
            }
        }
        _ => {}
    }
}

struct DeleteSite<'a> {
    stmt: &'a Stmt,
    delete: &'a ast::StmtDelete,
    suite_start: TextSize,
    suite_len: usize,
}

#[derive(Default)]
struct Deletes<'a>(Vec<DeleteSite<'a>>);
impl<'a> SourceOrderVisitor<'a> for Deletes<'a> {
    fn visit_body(&mut self, body: &'a [Stmt]) {
        for stmt in body {
            if let Stmt::Delete(delete) = stmt {
                self.0.push(DeleteSite {
                    stmt,
                    delete,
                    suite_start: body.first().map_or(stmt.start(), Ranged::start),
                    suite_len: body.len(),
                });
            }
        }
        walk_body(self, body);
    }
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if !matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            walk_stmt(self, stmt);
        }
    }
    fn visit_expr(&mut self, _expr: &'a Expr) {}
}

fn references_any(body: &[Stmt], names: &HashSet<&str>) -> bool {
    struct References<'n> {
        names: &'n HashSet<&'n str>,
        found: bool,
    }
    impl<'a> SourceOrderVisitor<'a> for References<'_> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            match node {
                AnyNodeRef::StmtDelete(_) => return TraversalSignal::Skip,
                AnyNodeRef::ExprName(name) if self.names.contains(name.id.as_str()) => {
                    self.found = true;
                }
                _ => {}
            }
            if self.found {
                TraversalSignal::Skip
            } else {
                TraversalSignal::Traverse
            }
        }
    }
    let mut visitor = References {
        names,
        found: false,
    };
    visitor.visit_body(body);
    visitor.found
}

fn remaining_target(
    c: &Context<'_, '_, '_>,
    target: &Expr,
    names: &HashSet<&str>,
) -> Option<String> {
    let (elements, open, close) = match target {
        Expr::Name(name) if names.contains(name.id.as_str()) => return None,
        Expr::Tuple(tuple) => (&tuple.elts, "(", ")"),
        Expr::List(list) => (&list.elts, "[", "]"),
        _ => return Some(c.text(target.range()).to_string()),
    };
    let remaining: Vec<_> = elements
        .iter()
        .filter_map(|element| remaining_target(c, element, names))
        .collect();
    match remaining.as_slice() {
        [] => None,
        [single] => Some(single.clone()),
        _ if remaining.len() == elements.len() => Some(c.text(target.range()).to_string()),
        _ => Some(format!("{open}{}{close}", remaining.join(", "))),
    }
}

impl DelUnusedArgument {
    fn fix(
        f: &ast::StmtFunctionDef,
        c: &Context<'_, '_, '_>,
        deleted: &HashSet<&str>,
        deletes: &[DeleteSite<'_>],
    ) -> Option<Fix> {
        let renamed: Vec<_> = parameters(&f.parameters)
            .filter(|parameter| {
                deleted.contains(parameter.name.as_str()) && !parameter.name.starts_with('_')
            })
            .collect();
        let underscored: Vec<_> = renamed
            .iter()
            .map(|parameter| format!("_{}", parameter.name))
            .collect();
        let underscored: HashSet<&str> = underscored.iter().map(String::as_str).collect();
        if parameters(&f.parameters).any(|parameter| underscored.contains(parameter.name.as_str()))
            || references_any(&f.body, &underscored)
        {
            return None;
        }
        // Only positional-only and variadic parameters can be renamed without breaking callers
        // that pass the argument by keyword.
        let keyword_capable = f
            .parameters
            .args
            .iter()
            .chain(&f.parameters.kwonlyargs)
            .any(|parameter| renamed.contains(&&parameter.parameter));
        if keyword_capable {
            return None;
        }
        let mut edits: Vec<_> = renamed
            .iter()
            .map(|parameter| Edit::insertion("_".into(), parameter.name.start()))
            .collect();
        let mut removed = Vec::new();
        let mut removed_per_suite: HashMap<TextSize, usize> = HashMap::new();
        for site in deletes {
            let delete = site.delete;
            let mut names = Vec::new();
            for target in &delete.targets {
                deleted_names(target, &mut names);
            }
            if !names.iter().any(|name| deleted.contains(name)) {
                continue;
            }
            if has_comment(c.text(delete.range())) {
                return None;
            }
            let remaining: Vec<_> = delete
                .targets
                .iter()
                .filter_map(|target| remaining_target(c, target, deleted))
                .collect();
            if remaining.is_empty() {
                *removed_per_suite.entry(site.suite_start).or_default() += 1;
                removed.push(site);
            } else {
                edits.push(Edit::range_replacement(
                    format!("del {}", remaining.join(", ")),
                    delete.range(),
                ));
            }
        }
        // A suite whose every statement is removed keeps one `pass` so it stays valid.
        let mut padded = HashSet::new();
        for site in removed {
            let emptied = removed_per_suite.get(&site.suite_start) == Some(&site.suite_len);
            edits.push(if emptied && padded.insert(site.suite_start) {
                Edit::range_replacement("pass".into(), site.delete.range())
            } else if simple_line(c, site.stmt) {
                Edit::range_deletion(TextRange::new(
                    c.line_start(site.delete.start()),
                    full_end(c, site.delete.end()),
                ))
            } else {
                return None;
            });
        }
        let mut edits = edits.into_iter();
        let first = edits.next()?;
        Some(Fix::safe_edits(first, edits))
    }
}
impl LintRule for DelUnusedArgument {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::StmtFunctionDef(f) = node else {
            return;
        };
        if test_like_path(c, &self.excluded_path_parts) {
            return;
        }
        let parameter_names: HashSet<&str> = parameters(&f.parameters)
            .map(|parameter| parameter.name.as_str())
            .collect();
        let mut deletes = Deletes::default();
        deletes.visit_body(&f.body);
        let mut names = Vec::new();
        for site in &deletes.0 {
            for target in &site.delete.targets {
                deleted_names(target, &mut names);
            }
        }
        let deleted: HashSet<&str> = names
            .into_iter()
            .filter(|name| parameter_names.contains(name))
            .collect();
        if deleted.is_empty() || references_any(&f.body, &deleted) {
            return;
        }
        let Some(parameter) =
            parameters(&f.parameters).find(|parameter| deleted.contains(parameter.name.as_str()))
        else {
            return;
        };
        let finding = Finding::new(
            "no-del-unused-argument",
            parameter.name.range(),
            "Prefix unused arguments with '_' instead of deleting them.",
        );
        out.push(match Self::fix(f, c, &deleted, &deletes.0) {
            Some(fix) => finding.with_fix(fix, "Prefix the argument with '_' and remove the `del`"),
            None => finding,
        });
    }
}

#[derive(Debug)]
struct SubprocessRunTimeout;
fn subprocess_run_timeout(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(SubprocessRunTimeout))
}
impl LintRule for SubprocessRunTimeout {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ExprCall(call) = node else {
            return;
        };
        if call
            .arguments
            .find_keyword("timeout")
            .is_some_and(|keyword| !keyword.value.is_none_literal_expr())
        {
            return;
        }
        if super::typing::resolve(c, &call.func, &["subprocess"]).as_deref()
            == Some("subprocess.run")
        {
            out.push(Finding::new(
                "require-subprocess-run-timeout",
                call.func.range(),
                "Pass an explicit timeout to subprocess.run.",
            ));
        }
    }
}

#[derive(Debug)]
struct TypedEmptyContainers;
fn typed_empty_containers(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(TypedEmptyContainers))
}
impl LintRule for TypedEmptyContainers {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        struct Bindings<'c, 'ctx, 'ast, 'src> {
            c: &'c Context<'ctx, 'ast, 'src>,
            scopes: Vec<HashMap<String, bool>>,
            out: &'c mut Vec<Finding>,
        }
        impl Bindings<'_, '_, '_, '_> {
            fn typed(&self, name: &str) -> bool {
                self.scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name).copied())
                    .unwrap_or(false)
            }
            fn empty_container(&self, value: &Expr) -> bool {
                match value {
                    Expr::List(list) => list.elts.is_empty(),
                    Expr::Dict(dict) => dict.items.is_empty(),
                    Expr::Call(call) => {
                        call.arguments.is_empty()
                            && self.c.qualified_name(&call.func).is_some_and(|name| {
                                matches!(
                                    name.as_str(),
                                    "builtins.dict" | "builtins.list" | "builtins.set"
                                )
                            })
                    }
                    _ => false,
                }
            }
            fn bind_untyped(&mut self, name: &str) {
                if let Some(scope) = self.scopes.last_mut() {
                    scope.entry(name.to_string()).or_insert(false);
                }
            }
            fn report(&mut self, range: TextRange) {
                self.out.push(Finding::new(
                    "require-typed-empty-container-assignment",
                    range,
                    "Add an explicit type annotation when assigning an empty container.",
                ));
            }
        }
        impl<'a> SourceOrderVisitor<'a> for Bindings<'_, '_, '_, '_> {
            fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                match node {
                    AnyNodeRef::StmtClassDef(_) => self.scopes.push(HashMap::new()),
                    AnyNodeRef::StmtFunctionDef(f) => self.scopes.push(
                        f.parameters
                            .posonlyargs
                            .iter()
                            .chain(&f.parameters.args)
                            .chain(&f.parameters.kwonlyargs)
                            .map(|parameter| {
                                (
                                    parameter.parameter.name.to_string(),
                                    parameter.parameter.annotation.is_some(),
                                )
                            })
                            .collect(),
                    ),
                    AnyNodeRef::StmtAnnAssign(assign) => {
                        if let Expr::Name(name) = &*assign.target
                            && let Some(scope) = self.scopes.last_mut()
                        {
                            scope.insert(name.id.to_string(), true);
                        }
                    }
                    AnyNodeRef::StmtAssign(assign) => {
                        let names: Vec<_> = assign
                            .targets
                            .iter()
                            .filter_map(Expr::as_name_expr)
                            .collect();
                        if self.empty_container(&assign.value)
                            && names.iter().any(|name| !self.typed(name.id.as_str()))
                        {
                            self.report(assign.range());
                        }
                        for name in names {
                            self.bind_untyped(name.id.as_str());
                        }
                    }
                    AnyNodeRef::ExprNamed(named) => {
                        if let Expr::Name(name) = &*named.target
                            && self.empty_container(&named.value)
                        {
                            if !self.typed(name.id.as_str()) {
                                self.report(named.range());
                            }
                            self.bind_untyped(name.id.as_str());
                        }
                    }
                    _ => {}
                }
                TraversalSignal::Traverse
            }
            fn leave_node(&mut self, node: AnyNodeRef<'a>) {
                if matches!(
                    node,
                    AnyNodeRef::StmtClassDef(_) | AnyNodeRef::StmtFunctionDef(_)
                ) {
                    self.scopes.pop();
                }
            }
        }
        let AnyNodeRef::ModModule(module) = node else {
            return;
        };
        let mut visitor = Bindings {
            c,
            scopes: vec![HashMap::new()],
            out,
        };
        visitor.visit_body(&module.body);
    }
}

#[derive(Debug)]
struct BoilerplateDocstrings;
fn boilerplate_docstrings(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(BoilerplateDocstrings))
}
const BOILERPLATE_DOCSTRING: &str =
    "Remove generated boilerplate docstrings that do not add useful API documentation.";
impl LintRule for BoilerplateDocstrings {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let (owner, body, header_end) = match node {
            AnyNodeRef::StmtFunctionDef(f) => (
                f.name.as_str(),
                &f.body,
                f.returns
                    .as_ref()
                    .map_or(f.parameters.end(), |returns| returns.end()),
            ),
            AnyNodeRef::StmtClassDef(class) => (
                class.name.as_str(),
                &class.body,
                class
                    .arguments
                    .as_ref()
                    .map(|arguments| arguments.end())
                    .or(class.type_params.as_ref().map(|params| params.end()))
                    .unwrap_or(class.name.end()),
            ),
            _ => return,
        };
        let Some(first) = body.first() else {
            return;
        };
        let Stmt::Expr(statement) = first else {
            return;
        };
        let Expr::StringLiteral(string) = &*statement.value else {
            return;
        };
        let Some(literal) = string.as_single_part_string() else {
            return;
        };
        // Skip same-line suites such as `def f(): "doc"` and semicolon-separated statements.
        if !simple_line(c, first) {
            return;
        }
        if boilerplate_docstring(literal.as_str(), owner) {
            let leading = TextRange::new(full_end(c, header_end), c.line_start(first.start()));
            let finding = Finding::new(
                "no-boilerplate-docstrings",
                first.range(),
                BOILERPLATE_DOCSTRING,
            );
            out.push(if body.len() > 1 && !has_comment(c.text(leading)) {
                finding.with_fix(
                    Fix::safe_edit(Edit::range_deletion(TextRange::new(
                        c.line_start(first.start()),
                        full_end(c, first.end()),
                    ))),
                    "Remove the docstring",
                )
            } else {
                finding
            });
            return;
        }
        let raw = c.text(literal.range());
        let Some(quote_start) = raw.find(['"', '\'']) else {
            return;
        };
        let Some(quote) = raw
            .get(quote_start..quote_start + 3)
            .filter(|quote| matches!(*quote, "\"\"\"" | "'''"))
        else {
            return;
        };
        let Some(docstring) = raw.get(quote_start + 3..raw.len() - 3) else {
            return;
        };
        let cleaned = without_boilerplate_arguments(docstring);
        if cleaned == docstring {
            return;
        }
        out.push(
            Finding::new(
                "no-boilerplate-docstrings",
                statement.range(),
                BOILERPLATE_DOCSTRING,
            )
            .with_fix(
                Fix::safe_edit(Edit::range_replacement(
                    format!("{}{quote}{cleaned}{quote}", &raw[..quote_start]),
                    literal.range(),
                )),
                "Remove boilerplate argument entries",
            ),
        );
    }
}
fn boilerplate_docstring(value: &str, owner: &str) -> bool {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized == format!("Represent {owner} state and behavior.")
        || normalized == format!("Value for {owner}.")
}
fn boilerplate_argument(line: &str) -> bool {
    static ARGUMENT: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"^\s*(\w+)(?:\s*\([^)]*\))?:\s*(?:Value for|The)\s+(.+?)\.?\s*$").ok()
    });
    ARGUMENT
        .as_ref()
        .and_then(|pattern| pattern.captures(line))
        .is_some_and(|captures| {
            captures[1].replace('_', " ").to_lowercase() == captures[2].to_lowercase()
        })
}
fn argument_section(line: &str) -> bool {
    matches!(line.trim(), "Args:" | "Arguments:")
}
fn indentation_width(line: &str) -> usize {
    line.len() - line.trim_start().len()
}
fn has_section_entries(lines: &[&str], header: usize) -> bool {
    let header_indent = indentation_width(lines[header]);
    lines[header + 1..]
        .iter()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| indentation_width(line) > header_indent)
}
fn without_boilerplate_arguments(docstring: &str) -> String {
    let lines: Vec<_> = docstring.split('\n').collect();
    let kept: Vec<_> = lines
        .iter()
        .copied()
        .filter(|line| !boilerplate_argument(line))
        .collect();
    let mut result: Vec<&str> = Vec::new();
    for (index, line) in kept.iter().enumerate() {
        if argument_section(line) && !has_section_entries(&kept, index) {
            while result.last().is_some_and(|line| line.trim().is_empty()) {
                result.pop();
            }
            continue;
        }
        result.push(line);
    }
    if result.len() == lines.len() {
        return docstring.to_string();
    }
    let mut content = result
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty());
    if let (Some(only), None) = (content.next(), content.next()) {
        return only.to_string();
    }
    let mut collapsed: Vec<&str> = Vec::new();
    for line in result {
        if line.trim().is_empty() && collapsed.last().is_some_and(|last| last.trim().is_empty()) {
            continue;
        }
        collapsed.push(line);
    }
    collapsed.join("\n")
}
