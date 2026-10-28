use std::collections::{HashMap, HashSet};

use ruff_python_ast::comparable::ComparableExpr;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_node};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, Stmt};
use ruff_source_file::{LineRanges, UniversalNewlines};
use ruff_text_size::{Ranged, TextRange, TextSize};

use super::RenderedMetrics;

pub(super) type Names = HashSet<String>;

pub(super) struct Footer {
    pub range: TextRange,
    pub suite: TextSize,
    pub previous: Option<TextSize>,
}

pub(super) struct Source<'a> {
    pub text: &'a str,
    pub metrics: Option<&'a dyn RenderedMetrics>,
    small_per_statement: HashMap<TextSize, usize>,
    ends: HashMap<TextSize, TextSize>,
    first_leading: HashSet<TextSize>,
    pub footers: Vec<Footer>,
}
impl<'a> Source<'a> {
    pub(super) fn new(
        module: &ast::ModModule,
        text: &'a str,
        metrics: Option<&'a dyn RenderedMetrics>,
    ) -> Self {
        struct Counter<'a> {
            text: &'a str,
            metrics: Option<&'a dyn RenderedMetrics>,
            counts: HashMap<TextSize, usize>,
        }
        impl<'a> SourceOrderVisitor<'a> for Counter<'_> {
            fn visit_body(&mut self, body: &'a [Stmt]) {
                let mut start = 0;
                while start < body.len() {
                    let mut end = start + 1;
                    if simple(&body[start]) {
                        while end < body.len()
                            && simple(&body[end])
                            && self
                                .metrics
                                .and_then(|metrics| {
                                    metrics.same_line(body[end - 1].end(), body[end].start())
                                })
                                .unwrap_or_else(|| {
                                    self.text.line_start(body[end - 1].end())
                                        == self.text.line_start(body[end].start())
                                })
                        {
                            end += 1;
                        }
                        for statement in &body[start..end] {
                            self.counts.insert(statement.start(), end - start);
                        }
                    }
                    start = end;
                }
                ruff_python_ast::visitor::source_order::walk_body(self, body);
            }
        }
        let mut counter = Counter {
            text,
            metrics,
            counts: HashMap::new(),
        };
        walk_node(&mut counter, AnyNodeRef::ModModule(module));
        let mut result = Self {
            text,
            metrics,
            small_per_statement: counter.counts,
            ends: HashMap::new(),
            first_leading: HashSet::new(),
            footers: Vec::new(),
        };
        result.index_suite(
            &module.body,
            TextSize::new(0),
            TextSize::try_from(text.len()).unwrap_or_default(),
            true,
        );
        result
    }
    pub(super) fn gap_start(&self, statement: &Stmt) -> TextSize {
        self.ends
            .get(&statement.start())
            .copied()
            .unwrap_or_else(|| self.text.full_line_end(statement.end()))
    }
    fn index_suite(
        &mut self,
        body: &[Stmt],
        anchor: TextSize,
        limit: TextSize,
        module: bool,
    ) -> TextSize {
        let Some(first) = body.first() else {
            return anchor;
        };
        let inline = !module && self.same_line(anchor, first.start());
        if !module && !inline {
            let start = self.header_end(anchor, self.first_offset(first));
            if start < self.text.line_start(self.first_offset(first)) {
                self.first_leading.insert(first.start());
            }
        }
        for (index, statement) in body.iter().enumerate() {
            let next = body
                .get(index + 1)
                .map_or(limit, |s| self.text.line_start(self.first_offset(s)));
            self.index_stmt(statement, next);
        }
        let Some(last) = body.last() else {
            return anchor;
        };
        let start = self.gap_start(last).min(limit);
        if inline {
            return start;
        }
        let indent = &self.text[TextRange::new(self.text.line_start(first.start()), first.start())];
        let mut end = start;
        let mut offset = start;
        for line in self.text[TextRange::new(start, limit)]
            .universal_newlines()
            .map(|line| line.as_full_str())
        {
            if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                break;
            }
            offset += TextSize::try_from(line.len()).unwrap_or_default();
            if module || line.starts_with(indent) {
                end = offset;
            }
        }
        self.footers.push(Footer {
            range: TextRange::new(start, end),
            suite: anchor,
            previous: Some(last.start()),
        });
        end
    }
    fn index_stmt(&mut self, statement: &Stmt, limit: TextSize) {
        let end = match statement {
            Stmt::FunctionDef(s) => self.index_suite(&s.body, s.start(), limit, false),
            Stmt::ClassDef(s) => self.index_suite(&s.body, s.start(), limit, false),
            Stmt::With(s) => self.index_suite(&s.body, s.start(), limit, false),
            Stmt::If(s) => {
                let first_limit = s
                    .elif_else_clauses
                    .first()
                    .map_or(limit, |c| self.text.line_start(c.start()));
                let mut end = self.index_suite(&s.body, s.start(), first_limit, false);
                for (index, clause) in s.elif_else_clauses.iter().enumerate() {
                    let next = s
                        .elif_else_clauses
                        .get(index + 1)
                        .map_or(limit, |c| self.text.line_start(c.start()));
                    end = self.index_suite(&clause.body, clause.start(), next, false);
                }
                end
            }
            Stmt::For(s) => self.index_loop(&s.body, &s.orelse, s.start(), limit),
            Stmt::While(s) => self.index_loop(&s.body, &s.orelse, s.start(), limit),
            Stmt::Try(s) => {
                let mut suites: Vec<(TextSize, &[Stmt])> = vec![(s.start(), &s.body)];
                for handler in &s.handlers {
                    let ast::ExceptHandler::ExceptHandler(h) = handler;
                    suites.push((h.start(), &h.body));
                }
                let mut previous = s
                    .handlers
                    .last()
                    .map_or_else(|| s.body.last().map_or(s.start(), Ranged::end), Ranged::end);
                if let Some(first) = s.orelse.first() {
                    let anchor = self.find_clause(previous, first.start());
                    suites.push((anchor, &s.orelse));
                    previous = s.orelse.last().map_or(previous, Ranged::end);
                }
                if let Some(first) = s.finalbody.first() {
                    suites.push((self.find_clause(previous, first.start()), &s.finalbody));
                }
                let mut end = self.text.full_line_end(s.end());
                for (index, (anchor, body)) in suites.iter().enumerate() {
                    let next = suites
                        .get(index + 1)
                        .map_or(limit, |(anchor, _)| self.text.line_start(*anchor));
                    end = self.index_suite(body, *anchor, next, false);
                }
                end
            }
            Stmt::Match(s) => {
                let mut end = self.text.full_line_end(s.end());
                for (index, case) in s.cases.iter().enumerate() {
                    let next = s
                        .cases
                        .get(index + 1)
                        .map_or(limit, |c| self.text.line_start(c.start()));
                    end = self.index_suite(&case.body, case.start(), next, false);
                }
                end
            }
            _ => return,
        };
        self.ends.insert(statement.start(), end);
    }
    fn index_loop(
        &mut self,
        body: &[Stmt],
        alternative: &[Stmt],
        anchor: TextSize,
        limit: TextSize,
    ) -> TextSize {
        if let Some(first) = alternative.first() {
            let clause = self.find_clause(body.last().map_or(anchor, Ranged::end), first.start());
            self.index_suite(body, anchor, self.text.line_start(clause), false);
            self.index_suite(alternative, clause, limit, false)
        } else {
            self.index_suite(body, anchor, limit, false)
        }
    }
    pub(super) fn find_clause(&self, previous: TextSize, first: TextSize) -> TextSize {
        let start = self
            .text
            .full_line_end(previous)
            .min(self.text.line_start(first));
        let end = self.text.line_start(first);
        let mut offset = start;
        for line in self.text[TextRange::new(start, end)]
            .universal_newlines()
            .map(|line| line.as_full_str())
        {
            if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                return offset
                    + TextSize::try_from(line.len() - line.trim_start().len()).unwrap_or_default();
            }
            offset += TextSize::try_from(line.len()).unwrap_or_default();
        }
        previous
    }
    pub(super) fn header_end(&self, anchor: TextSize, first: TextSize) -> TextSize {
        let mut last = self.text.full_line_end(anchor);
        let end = self.text.line_start(first);
        let mut offset = last;
        for line in self.text[TextRange::new(last.min(end), end)]
            .universal_newlines()
            .map(|line| line.as_full_str())
        {
            offset += TextSize::try_from(line.len()).unwrap_or_default();
            if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                last = offset;
            }
        }
        last.min(end)
    }
    pub(super) fn equal(&self, a: &Expr, b: &Expr) -> bool {
        equal(a, b) && self.text[a.range()] == self.text[b.range()]
    }
    pub(super) fn single(&self, stmt: &Stmt) -> bool {
        self.small_per_statement
            .get(&stmt.start())
            .copied()
            .unwrap_or(0)
            == 1
    }
    pub(super) fn same_line(&self, a: TextSize, b: TextSize) -> bool {
        self.text.line_start(a) == self.text.line_start(b)
    }
    pub(super) fn layout_same_line(&self, a: TextSize, b: TextSize) -> bool {
        self.metrics
            .and_then(|metrics| metrics.same_line(a, b))
            .unwrap_or_else(|| self.same_line(a, b))
    }
    pub(super) fn count(&self, range: TextRange) -> usize {
        self.metrics
            .and_then(|m| m.non_empty_lines(range))
            .unwrap_or_else(|| {
                self.text[TextRange::new(
                    self.text.line_start(range.start()),
                    self.text.line_end(range.end()),
                )]
                .universal_newlines()
                .filter(|line| !line.trim().is_empty())
                .count()
            })
    }
    pub(super) fn body_range(&self, body: &[Stmt]) -> Option<TextRange> {
        Some(TextRange::new(body.first()?.start(), body.last()?.end()))
    }
    pub(super) fn body_count(&self, body: &[Stmt]) -> usize {
        self.body_range(body).map_or(0, |r| self.count(r))
    }
    pub(super) fn first_offset(&self, stmt: &Stmt) -> TextSize {
        let decorator = match stmt {
            Stmt::FunctionDef(s) => s.decorator_list.first(),
            Stmt::ClassDef(s) => s.decorator_list.first(),
            _ => None,
        };
        decorator.map_or(stmt.start(), Ranged::start)
    }
    pub(super) fn separator(&self, a: &Stmt, b: &Stmt) -> bool {
        self.gap_start(a) < self.text.line_start(self.first_offset(b))
    }
    pub(super) fn inline(&self, stmt: &Stmt) -> bool {
        primary(stmt)
            .first()
            .is_some_and(|first| self.layout_same_line(stmt.start(), first.start()))
    }
    pub(super) fn assignment(&self, stmt: &Stmt) -> bool {
        matches!(
            stmt,
            Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::AugAssign(_)
        ) && self.single(stmt)
    }
    pub(super) fn branch(&self, stmt: &Stmt) -> bool {
        branch(stmt) && self.single(stmt)
    }
    pub(super) fn guard(&self, stmt: &Stmt) -> bool {
        let Stmt::If(s) = stmt else {
            return false;
        };
        let count = if self.inline(stmt) {
            s.body.len()
        } else {
            self.statement_count(&s.body)
        };
        s.elif_else_clauses.is_empty()
            && (1..=2).contains(&count)
            && s.body.iter().all(simple)
            && s.body.last().is_some_and(|last| {
                if self.inline(stmt) {
                    branch(last)
                } else {
                    self.branch(last)
                }
            })
    }
    pub(super) fn statement_count(&self, body: &[Stmt]) -> usize {
        body.iter()
            .enumerate()
            .filter(|(i, stmt)| {
                *i == 0 || !simple(stmt) || !self.layout_same_line(body[*i - 1].end(), stmt.start())
            })
            .count()
    }
    pub(super) fn leading<'b>(&self, body: &'b [Stmt], limit: usize) -> &'b [Stmt] {
        let mut count = 0;
        let mut end = 0;
        for (index, statement) in body.iter().enumerate() {
            if index == 0 || !self.layout_same_line(body[index - 1].end(), statement.start()) {
                count += 1;
            }
            if count > limit {
                break;
            }
            end = index + 1;
        }
        &body[..end]
    }
    pub(super) fn assigned(&self, stmt: &Stmt) -> Names {
        if self.assignment(stmt) {
            assigned(stmt)
        } else {
            Names::new()
        }
    }
    pub(super) fn setup<'b>(&self, stmt: &'b Stmt) -> Vec<&'b Expr> {
        if self.single(stmt) {
            setup_expressions(stmt)
        } else {
            Vec::new()
        }
    }
    pub(super) fn groups<'b>(&self, stmt: &'b Stmt) -> Vec<&'b [Stmt]> {
        groups(stmt)
            .into_iter()
            .filter(|body| {
                body.first()
                    .is_none_or(|first| self.own_line(first.start()))
            })
            .collect()
    }
    pub(super) fn primary<'b>(&self, stmt: &'b Stmt) -> &'b [Stmt] {
        let body = primary(stmt);
        if body
            .first()
            .is_some_and(|first| !self.own_line(first.start()))
        {
            &[]
        } else {
            body
        }
    }

    fn own_line(&self, offset: TextSize) -> bool {
        self.metrics
            .and_then(|metrics| metrics.is_own_line(offset))
            .unwrap_or_else(|| {
                self.text[TextRange::new(self.text.line_start(offset), offset)]
                    .trim()
                    .is_empty()
            })
    }
    pub(super) fn run_start(&self, body: &[Stmt], index: usize) -> usize {
        if index == 0 {
            return 0;
        }
        let mut start = index - 1;
        while start > 0 && !self.separator(&body[start - 1], &body[start]) {
            start -= 1;
        }
        if (start > 0 && self.separator(&body[start - 1], &body[start]))
            || (start == 0 && self.first_leading.contains(&body[0].start()))
        {
            start += 1;
        }
        start.min(index)
    }
    pub(super) fn ladder(&self, body: &[Stmt], start: usize) -> bool {
        let mut count = 0;
        for index in start..body.len() {
            if !self.guard(&body[index]) {
                return false;
            }
            count += 1;
            let Some(next) = body.get(index + 1) else {
                return count >= 2;
            };
            if self.separator(&body[index], next) || self.branch(next) {
                return count >= 2;
            }
        }
        false
    }
    pub(super) fn ladder_tail(&self, body: &[Stmt], index: usize) -> bool {
        let mut run = &body[self.run_start(body, index)..index];
        if run.first().is_some_and(|s| self.assignment(s)) {
            run = &run[1..];
        }
        !run.is_empty() && run.iter().all(|s| self.guard(s))
    }
}

pub(super) fn simple(stmt: &Stmt) -> bool {
    !matches!(
        stmt,
        Stmt::FunctionDef(_)
            | Stmt::ClassDef(_)
            | Stmt::For(_)
            | Stmt::While(_)
            | Stmt::If(_)
            | Stmt::With(_)
            | Stmt::Match(_)
            | Stmt::Try(_)
    )
}
pub(super) fn branch(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Return(_) | Stmt::Raise(_) | Stmt::Break(_) | Stmt::Continue(_)
    )
}
pub(super) fn control(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::For(_) | Stmt::While(_) | Stmt::If(_) | Stmt::With(_) | Stmt::Match(_) | Stmt::Try(_)
    )
}
pub(super) fn header(stmt: &Stmt) -> bool {
    control(stmt) && !matches!(stmt, Stmt::Try(_))
}
pub(super) fn definition(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_))
}
pub(super) fn docstring(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(s) if matches!(s.value.as_ref(), Expr::StringLiteral(_)))
}
pub(super) fn primary(stmt: &Stmt) -> &[Stmt] {
    match stmt {
        Stmt::For(s) => &s.body,
        Stmt::While(s) => &s.body,
        Stmt::If(s) => &s.body,
        Stmt::With(s) => &s.body,
        Stmt::Try(s) => &s.body,
        Stmt::Match(s) => s.cases.first().map_or(&[], |c| c.body.as_slice()),
        _ => &[],
    }
}
pub(super) fn groups(stmt: &Stmt) -> Vec<&[Stmt]> {
    match stmt {
        Stmt::If(s) => std::iter::once(s.body.as_slice())
            .chain(s.elif_else_clauses.iter().map(|c| c.body.as_slice()))
            .collect(),
        Stmt::For(s) => vec![&s.body, &s.orelse],
        Stmt::While(s) => vec![&s.body, &s.orelse],
        Stmt::With(s) => vec![&s.body],
        Stmt::Try(s) => std::iter::once(s.body.as_slice())
            .chain(s.handlers.iter().map(|h| {
                let ast::ExceptHandler::ExceptHandler(h) = h;
                h.body.as_slice()
            }))
            .chain([s.orelse.as_slice(), s.finalbody.as_slice()])
            .collect(),
        Stmt::Match(s) => s.cases.iter().map(|c| c.body.as_slice()).collect(),
        _ => Vec::new(),
    }
}
pub(super) fn headers(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::If(s) => vec![&s.test],
        Stmt::While(s) => vec![&s.test],
        Stmt::For(s) => vec![&s.iter],
        Stmt::With(s) => s.items.iter().map(|i| &i.context_expr).collect(),
        Stmt::Match(s) => vec![&s.subject],
        _ => Vec::new(),
    }
}

pub(super) fn names(node: AnyNodeRef<'_>) -> Names {
    struct Collector(Names);
    impl<'a> SourceOrderVisitor<'a> for Collector {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            match node {
                AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
                    return TraversalSignal::Skip;
                }
                AnyNodeRef::ExprName(n) => {
                    self.0.insert(n.id.to_string());
                }
                AnyNodeRef::ExprAttribute(n) => {
                    self.0.insert(n.attr.to_string());
                }
                AnyNodeRef::Keyword(n) => {
                    if let Some(arg) = &n.arg {
                        self.0.insert(arg.to_string());
                    }
                }
                AnyNodeRef::ExprBooleanLiteral(n) => {
                    self.0.insert(if n.value { "True" } else { "False" }.into());
                }
                AnyNodeRef::ExprNoneLiteral(_) => {
                    self.0.insert("None".into());
                }
                _ => {}
            }
            TraversalSignal::Traverse
        }
        fn visit_identifier(&mut self, identifier: &'a ast::Identifier) {
            self.0.insert(identifier.to_string());
        }
    }
    let mut collector = Collector(Names::new());
    walk_node(&mut collector, node);
    collector.0
}
pub(super) fn expr_names(expr: &Expr) -> Names {
    names(expr.into())
}
pub(super) fn targets(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::Assign(s) => s.targets.iter().collect(),
        Stmt::AnnAssign(s) => vec![&s.target],
        Stmt::AugAssign(s) => vec![&s.target],
        _ => Vec::new(),
    }
}
pub(super) fn target_names<'a>(expr: &'a Expr, out: &mut Vec<&'a ast::ExprName>) {
    match expr {
        Expr::Name(n) => out.push(n),
        Expr::Tuple(n) => {
            for e in &n.elts {
                target_names(e, out);
            }
        }
        Expr::List(n) => {
            for e in &n.elts {
                target_names(e, out);
            }
        }
        Expr::Starred(n) => target_names(&n.value, out),
        _ => {}
    }
}
pub(super) fn assigned_nodes(stmt: &Stmt) -> Vec<&ast::ExprName> {
    let mut out = Vec::new();
    for t in targets(stmt) {
        target_names(t, &mut out);
    }
    out
}
pub(super) fn assigned(stmt: &Stmt) -> Names {
    assigned_nodes(stmt)
        .into_iter()
        .map(|n| n.id.to_string())
        .collect()
}
pub(super) fn last_name(stmt: &Stmt) -> Option<&str> {
    assigned_nodes(stmt).last().map(|n| n.id.as_str())
}
pub(super) fn values(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::Assign(s) => vec![&s.value],
        Stmt::AnnAssign(s) => s.value.iter().map(AsRef::as_ref).collect(),
        Stmt::AugAssign(s) => vec![&s.target, &s.value],
        _ => Vec::new(),
    }
}
pub(super) fn target_refs(expr: &Expr) -> Names {
    match expr {
        Expr::Name(_) => Names::new(),
        Expr::Tuple(s) => s.elts.iter().flat_map(target_refs).collect(),
        Expr::List(s) => s.elts.iter().flat_map(target_refs).collect(),
        Expr::Starred(s) => target_refs(&s.value),
        _ => expr_names(expr),
    }
}
pub(super) fn refs(stmt: &Stmt, consumed: bool) -> Names {
    match stmt {
        Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::AugAssign(_) => {
            let mut out: Names = values(stmt).into_iter().flat_map(expr_names).collect();
            if !consumed {
                for t in targets(stmt) {
                    out.extend(target_refs(t));
                }
            }
            out
        }
        Stmt::Assert(s) => std::iter::once(s.test.as_ref())
            .chain(s.msg.iter().map(AsRef::as_ref))
            .flat_map(expr_names)
            .collect(),
        Stmt::Expr(s) => expr_names(&s.value),
        Stmt::Return(s) => s.value.iter().flat_map(|v| expr_names(v)).collect(),
        Stmt::Raise(s) => s.exc.iter().flat_map(|v| expr_names(v)).collect(),
        _ => headers(stmt).into_iter().flat_map(expr_names).collect(),
    }
}
pub(super) fn direct_branch(stmt: &Stmt, assigned: &Names) -> bool {
    let expr = match stmt {
        Stmt::Return(s) => s.value.as_deref(),
        Stmt::Raise(s) => s.exc.as_deref(),
        _ => None,
    };
    matches!(expr, Some(Expr::Name(n)) if assigned.contains(n.id.as_str()))
}
pub(super) fn related_use(src: &Source<'_>, body: &[Stmt], index: usize, lookahead: usize) -> bool {
    let assigned = assigned(&body[index]);
    !assigned.is_empty()
        && src.leading(&body[index + 1..], lookahead).iter().any(|s| {
            !refs(s, false).is_disjoint(&assigned)
                && !(src.branch(s) && direct_branch(s, &assigned))
        })
}
pub(super) fn early_consumed(src: &Source<'_>, stmt: &Stmt, limit: usize) -> Names {
    src.groups(stmt)
        .into_iter()
        .flat_map(|g| src.leading(g, limit))
        .flat_map(|s| refs(s, true))
        .collect()
}
pub(super) fn next_consumes(src: &Source<'_>, body: &[Stmt], index: usize, limit: usize) -> bool {
    body.get(index + 1).is_some_and(|next| {
        control(next) && !early_consumed(src, next, limit).is_disjoint(&assigned(&body[index]))
    })
}
pub(super) fn expression(stmt: &Stmt) -> Option<&Expr> {
    match stmt {
        Stmt::Expr(s) => Some(&s.value),
        _ => None,
    }
}
pub(super) fn setup_expressions(stmt: &Stmt) -> Vec<&Expr> {
    let mut out = values(stmt);
    out.extend(targets(stmt));
    out.extend(expression(stmt));
    out
}
pub(super) fn receivers(expr: &Expr) -> Vec<&Expr> {
    struct Collector<'a>(Vec<&'a Expr>);
    impl<'a> SourceOrderVisitor<'a> for Collector<'a> {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Attribute(s) = expr {
                self.0.push(&s.value);
            } else {
                ruff_python_ast::visitor::source_order::walk_expr(self, expr);
            }
        }
    }
    let mut collector = Collector(Vec::new());
    collector.visit_expr(expr);
    collector.0
}
pub(super) fn equal(a: &Expr, b: &Expr) -> bool {
    ComparableExpr::from(a) == ComparableExpr::from(b)
}
pub(super) fn shared_receivers<'a, 'b>(
    src: &Source<'_>,
    a: impl IntoIterator<Item = &'a Expr>,
    b: impl IntoIterator<Item = &'b Expr>,
) -> bool {
    let a: Vec<_> = a.into_iter().collect();
    let b: Vec<_> = b.into_iter().collect();
    a.iter().flat_map(|e| receivers(e)).any(|ar| {
        b.iter()
            .flat_map(|e| receivers(e))
            .any(|br| src.equal(ar, br))
    })
}
pub(super) fn contains_expr(src: &Source<'_>, node: AnyNodeRef<'_>, target: &Expr) -> bool {
    struct Collector<'a, 's> {
        src: &'a Source<'s>,
        target: &'a Expr,
        found: bool,
    }
    impl<'a> SourceOrderVisitor<'a> for Collector<'_, '_> {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            let same_source = self.src.text[node.range()] == self.src.text[self.target.range()];
            self.found |= same_source
                && match (node, self.target) {
                    (AnyNodeRef::ExprName(a), Expr::Name(b)) => a.id == b.id,
                    (AnyNodeRef::ExprAttribute(a), Expr::Attribute(b)) => {
                        a.attr.as_str() == b.attr.as_str() && equal(&a.value, &b.value)
                    }
                    (AnyNodeRef::ExprSubscript(a), Expr::Subscript(b)) => {
                        equal(&a.value, &b.value) && equal(&a.slice, &b.slice)
                    }
                    _ => false,
                };
            TraversalSignal::Traverse
        }
        fn visit_identifier(&mut self, identifier: &'a ast::Identifier) {
            if matches!(self.target, Expr::Name(name) if name.id.as_str() == identifier.as_str()) {
                self.found = true;
            }
        }
    }
    let mut collector = Collector {
        src,
        target,
        found: false,
    };
    walk_node(&mut collector, node);
    collector.found
}
pub(super) fn bound(stmt: &Stmt) -> Names {
    let mut out = Vec::new();
    match stmt {
        Stmt::For(s) => target_names(&s.target, &mut out),
        Stmt::With(s) => {
            for i in &s.items {
                if let Some(t) = &i.optional_vars {
                    target_names(t, &mut out);
                }
            }
        }
        _ => {}
    }
    let mut result: Names = out.into_iter().map(|n| n.id.to_string()).collect();
    if let Stmt::Match(s) = stmt {
        if let Some(c) = s.cases.first() {
            struct Bindings<'a>(&'a mut Names);
            impl<'a> SourceOrderVisitor<'a> for Bindings<'_> {
                fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                    match node {
                        AnyNodeRef::PatternMatchAs(p) => {
                            if let Some(n) = &p.name {
                                self.0.insert(n.to_string());
                            }
                        }
                        AnyNodeRef::PatternMatchStar(p) => {
                            if let Some(n) = &p.name {
                                self.0.insert(n.to_string());
                            }
                        }
                        AnyNodeRef::PatternMatchMapping(p) => {
                            if let Some(n) = &p.rest {
                                self.0.insert(n.to_string());
                            }
                        }
                        _ => {}
                    }
                    TraversalSignal::Traverse
                }
            }
            Bindings(&mut result).visit_pattern(&c.pattern);
        }
    }
    result
}
pub(super) fn pytest(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::With(s) if s.items.first().is_some_and(|i| matches!(&i.context_expr, Expr::Call(c) if matches!(c.func.as_ref(), Expr::Attribute(a) if a.attr.as_str() == "raises" && matches!(a.value.as_ref(), Expr::Name(n) if n.id.as_str() == "pytest")))))
}
pub(super) fn pass_handler(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Try(s) if s.handlers.iter().any(|h| { let ast::ExceptHandler::ExceptHandler(h) = h; matches!(h.body.as_slice(), [Stmt::Pass(_)]) }))
}
pub(super) fn pass_try(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Try(s) if !s.handlers.is_empty() && s.orelse.is_empty() && s.finalbody.is_empty() && s.handlers.iter().all(|h| { let ast::ExceptHandler::ExceptHandler(h) = h; matches!(h.body.as_slice(), [Stmt::Pass(_)]) }))
}
