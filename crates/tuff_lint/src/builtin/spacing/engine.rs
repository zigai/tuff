use ruff_python_ast::{self as ast, Expr, ModModule, Stmt};
use ruff_source_file::{LineRanges, UniversalNewlines, find_newline};
use ruff_text_size::{Ranged, TextRange, TextSize};

use super::capture::{ScopeKind, captures};
use super::facts::{
    Names, Source, assigned, assigned_nodes, bound, contains_expr, control, definition,
    direct_branch, docstring, early_consumed, expr_names, expression, header, headers, last_name,
    names, next_consumes, pass_handler, pass_try, pytest, refs, related_use, shared_receivers,
    simple, targets,
};
use super::{GapAnchor, GapDecision, GapKind, RenderedMetrics, SpacingOptions, SpacingRule};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Parent {
    Module,
    Function,
    Class,
    Control,
    Cleanup,
}
impl Parent {
    fn docstring(self) -> bool {
        matches!(self, Self::Module | Self::Function | Self::Class)
    }
}

pub(super) fn plan(
    module: &ModModule,
    source: &str,
    enabled: &[SpacingRule],
    options: &SpacingOptions,
    metrics: Option<&dyn RenderedMetrics>,
) -> Vec<GapDecision> {
    let mut planner = Planner {
        src: Source::new(module, source, metrics),
        enabled,
        options,
        decisions: Vec::new(),
    };
    planner.suite(
        &module.body,
        Parent::Module,
        TextSize::new(0),
        ScopeKind::Module,
    );
    if enabled.contains(&SpacingRule::NoSuiteLeadingTrailingBlankLines) {
        planner.trailing();
    }
    planner
        .decisions
        .sort_by_key(|d| (d.range.start(), d.range.end()));
    planner.decisions
}
struct Planner<'a> {
    src: Source<'a>,
    enabled: &'a [SpacingRule],
    options: &'a SpacingOptions,
    decisions: Vec<GapDecision>,
}
impl Planner<'_> {
    fn enabled(&self, rule: SpacingRule) -> bool {
        self.enabled.contains(&rule)
    }
    fn suite(&mut self, body: &[Stmt], parent: Parent, anchor: TextSize, scope: ScopeKind) {
        if body.is_empty() {
            return;
        }
        if parent != Parent::Module && self.src.layout_same_line(anchor, body[0].start()) {
            return;
        }
        if self.enabled(SpacingRule::NoSuiteLeadingTrailingBlankLines) && parent != Parent::Module {
            self.leading(body, parent, anchor);
        }
        let suite_lines = self.src.body_count(body);
        let suite_statements = self.src.statement_count(body);
        for index in 1..body.len() {
            let previous = &body[index - 1];
            let statement = &body[index];
            if self
                .src
                .layout_same_line(previous.end(), self.src.first_offset(statement))
            {
                continue;
            }
            if self.enabled(SpacingRule::AfterControlBlock) && self.after_control(body, index - 1) {
                self.between(
                    previous,
                    statement,
                    anchor,
                    SpacingRule::AfterControlBlock,
                    1,
                    "Add a blank line after this multiline control-flow block.",
                    false,
                );
            }
            if self.enabled(SpacingRule::AfterTerminalControlBlock)
                && self.after_terminal(body, index - 1)
            {
                self.between(
                    previous,
                    statement,
                    anchor,
                    SpacingRule::AfterTerminalControlBlock,
                    1,
                    "Add a blank line after this early-exit control-flow block.",
                    false,
                );
            }
            if self.enabled(SpacingRule::BeforeAssignment)
                && !(matches!(parent, Parent::Control | Parent::Cleanup)
                    && suite_statements
                        <= self
                            .options
                            .before_assignment
                            .short_control_flow_max_statements)
                && self.src.assignment(statement)
            {
                self.assignment(body, index, parent, anchor, scope);
            }
            if self.enabled(SpacingRule::BeforeBranch)
                && suite_lines > self.options.before_branch.max_suite_non_empty_lines
                && self.src.branch(statement)
            {
                self.branch_gap(body, index, parent, anchor);
            }
            for rule in [
                SpacingRule::BeforeUnrelatedBlock,
                SpacingRule::BlockHeaderCuddleStrict,
            ] {
                if self.enabled(rule)
                    && header(statement)
                    && !(parent.docstring() && index == 1 && docstring(&body[0]))
                    && !self.allowed_header(
                        body,
                        index,
                        rule == SpacingRule::BlockHeaderCuddleStrict,
                    )
                {
                    self.between(previous, statement, anchor, rule, 1, if rule == SpacingRule::BeforeUnrelatedBlock { "Add a blank line before this block; the preceding statements do not prepare values used by it." } else { "Add a blank line before this block; only an immediately preceding assignment used by the block may remain attached." }, true);
                }
            }
        }
        for statement in body {
            self.descend(statement, scope);
        }
    }
    fn descend(&mut self, statement: &Stmt, scope: ScopeKind) {
        match statement {
            Stmt::FunctionDef(s) => {
                self.suite(&s.body, Parent::Function, s.start(), ScopeKind::Function)
            }
            Stmt::ClassDef(s) => self.suite(&s.body, Parent::Class, s.start(), ScopeKind::Class),
            Stmt::If(s) => {
                self.suite(&s.body, Parent::Control, s.start(), scope);
                for clause in &s.elif_else_clauses {
                    self.suite(&clause.body, Parent::Control, clause.start(), scope);
                }
            }
            Stmt::For(s) => {
                self.suite(&s.body, Parent::Control, s.start(), scope);
                if let Some(first) = s.orelse.first() {
                    let anchor = self
                        .clause_anchor(s.body.last().map_or(s.start(), Ranged::end), first.start());
                    self.suite(&s.orelse, Parent::Control, anchor, scope);
                }
            }
            Stmt::While(s) => {
                self.suite(&s.body, Parent::Control, s.start(), scope);
                if let Some(first) = s.orelse.first() {
                    let anchor = self
                        .clause_anchor(s.body.last().map_or(s.start(), Ranged::end), first.start());
                    self.suite(&s.orelse, Parent::Control, anchor, scope);
                }
            }
            Stmt::With(s) => self.suite(&s.body, Parent::Control, s.start(), scope),
            Stmt::Try(s) => {
                self.suite(&s.body, Parent::Control, s.start(), scope);
                let mut end = s.body.last().map_or(s.start(), Ranged::end);
                for handler in &s.handlers {
                    let ast::ExceptHandler::ExceptHandler(h) = handler;
                    self.suite(&h.body, Parent::Cleanup, h.start(), scope);
                    end = h.end();
                }
                if let Some(first) = s.orelse.first() {
                    let anchor = self.clause_anchor(end, first.start());
                    self.suite(&s.orelse, Parent::Control, anchor, scope);
                    end = s.orelse.last().map_or(end, Ranged::end);
                }
                if let Some(first) = s.finalbody.first() {
                    let anchor = self.clause_anchor(end, first.start());
                    self.suite(&s.finalbody, Parent::Cleanup, anchor, scope);
                }
            }
            Stmt::Match(s) => {
                if self.enabled(SpacingRule::MatchCaseSeparation) {
                    for pair in s.cases.windows(2) {
                        let a = &pair[0];
                        let b = &pair[1];
                        if self.src.body_count(&a.body)
                            > self.options.match_case_separation.max_case_non_empty_lines
                        {
                            self.gap(
                                self.src.text.full_line_end(a.end()),
                                b.start(),
                                GapAnchor {
                                    kind: GapKind::BetweenCases,
                                    suite: s.start(),
                                    previous: Some(a.start()),
                                    next: Some(b.start()),
                                },
                                SpacingRule::MatchCaseSeparation,
                                1,
                                "Missing separator between match cases after a large case body.",
                                TextRange::at(b.start(), TextSize::new(4)),
                            );
                        }
                    }
                }
                for case in &s.cases {
                    self.suite(&case.body, Parent::Control, case.start(), scope);
                }
            }
            _ => {}
        }
    }
    fn clause_anchor(&self, previous_end: TextSize, first_start: TextSize) -> TextSize {
        self.src.find_clause(previous_end, first_start)
    }
    fn anchor_range(&self, statement: &Stmt, keyword: bool) -> TextRange {
        let length = if keyword {
            match statement {
                Stmt::If(_) => Some(2),
                Stmt::For(s) => Some(if s.is_async { 9 } else { 3 }),
                Stmt::While(_) => Some(5),
                Stmt::With(s) => Some(if s.is_async { 10 } else { 4 }),
                Stmt::Match(_) => Some(5),
                Stmt::Return(_) => Some(6),
                Stmt::Raise(_) => Some(5),
                Stmt::Break(_) => Some(5),
                Stmt::Continue(_) => Some(8),
                _ => None,
            }
        } else {
            None
        };
        length.map_or_else(
            || TextRange::new(statement.start(), self.src.text.line_end(statement.start())),
            |n| TextRange::at(statement.start(), TextSize::new(n)),
        )
    }
    fn between(
        &mut self,
        a: &Stmt,
        b: &Stmt,
        suite: TextSize,
        rule: SpacingRule,
        blanks: usize,
        reason: &'static str,
        keyword: bool,
    ) {
        let diagnostic = self.anchor_range(b, keyword);
        if self.src.same_line(a.end(), self.src.first_offset(b)) {
            self.decisions.push(GapDecision {
                anchor: GapAnchor {
                    kind: GapKind::BetweenStatements,
                    suite,
                    previous: Some(a.start()),
                    next: Some(b.start()),
                },
                range: TextRange::empty(b.start()),
                replacement: self.newline().repeat(blanks),
                blank_lines: blanks,
                rule,
                reason,
                diagnostic_range: diagnostic,
                fixable: false,
            });
            return;
        }
        self.gap(
            self.src.gap_start(a),
            self.src.first_offset(b),
            GapAnchor {
                kind: GapKind::BetweenStatements,
                suite,
                previous: Some(a.start()),
                next: Some(b.start()),
            },
            rule,
            blanks,
            reason,
            diagnostic,
        );
    }
    fn gap(
        &mut self,
        start: TextSize,
        next_start: TextSize,
        anchor: GapAnchor,
        rule: SpacingRule,
        blanks: usize,
        reason: &'static str,
        diagnostic_range: TextRange,
    ) {
        let end = self.src.text.line_start(next_start);
        if start > end {
            return;
        }
        let range = TextRange::new(start, end);
        let current = &self.src.text[range];
        // A comment counts as a separator. Never delete, relocate or duplicate it.
        if !current.trim().is_empty() {
            return;
        }
        let existing = current.universal_newlines().count();
        let blank_lines = if blanks > 0 { existing.max(blanks) } else { 0 };
        let replacement = if existing == blank_lines {
            current.to_string()
        } else {
            self.newline().repeat(blank_lines)
        };
        self.decisions.push(GapDecision {
            fixable: true,
            anchor,
            range,
            replacement,
            blank_lines,
            rule,
            reason,
            diagnostic_range,
        });
    }
    fn newline(&self) -> &'static str {
        find_newline(self.src.text).map_or("\n", |(_, ending)| ending.as_str())
    }
    fn leading(&mut self, body: &[Stmt], parent: Parent, suite: TextSize) {
        let first = &body[0];
        let first_start = self.src.text.line_start(self.src.first_offset(first));
        let start = self.src.header_end(suite, first_start);
        if start > first_start {
            return;
        }
        let leading = &self.src.text[TextRange::new(start, first_start)];
        let mut length = 0;
        for line in leading.universal_newlines().map(|line| line.as_full_str()) {
            if !line.trim().is_empty() {
                break;
            }
            length += line.len();
        }
        let has_comment = !leading.trim().is_empty();
        let allowed = usize::from(
            definition(first)
                && parent != Parent::Module
                && parent != Parent::Class
                && (parent == Parent::Function || !has_comment),
        );
        let range = TextRange::new(
            start,
            start + TextSize::try_from(length).unwrap_or_default(),
        );
        let current = &self.src.text[range];
        let count = current.universal_newlines().count();
        // Nested definitions may have one leading blank, but do not require one.
        let blank_lines = count.min(allowed);
        let replacement = if blank_lines == count {
            current.to_string()
        } else {
            self.newline().repeat(blank_lines)
        };
        self.decisions.push(GapDecision {
            fixable: true,
            anchor: GapAnchor {
                kind: GapKind::SuiteLeading,
                suite,
                previous: None,
                next: Some(first.start()),
            },
            range,
            replacement,
            blank_lines,
            rule: SpacingRule::NoSuiteLeadingTrailingBlankLines,
            reason: "Remove blank lines at the beginning of this code block.",
            diagnostic_range: first.range(),
        });
    }
    fn trailing(&mut self) {
        for footer in &self.src.footers {
            let end = footer.range.end();
            let mut start = end;
            for line in self.src.text[footer.range].universal_newlines().rev() {
                if !line.trim().is_empty() {
                    break;
                }
                start = footer.range.start() + line.start();
            }
            self.decisions.push(GapDecision {
                fixable: true,
                anchor: GapAnchor {
                    kind: GapKind::SuiteTrailing,
                    suite: footer.suite,
                    previous: footer.previous,
                    next: None,
                },
                range: TextRange::new(start, end),
                replacement: String::new(),
                blank_lines: 0,
                rule: SpacingRule::NoSuiteLeadingTrailingBlankLines,
                reason: "Remove blank lines at the end of this code block.",
                diagnostic_range: TextRange::new(start, end),
            });
        }
    }
    fn after_control(&self, body: &[Stmt], index: usize) -> bool {
        let current = &body[index];
        let next = &body[index + 1];
        let settings = &self.options.after_control_block;
        if !control(current) || self.src.inline(current) {
            return false;
        }
        if header(next) {
            let allowed = if pytest(current) && pytest(next) {
                settings.allow_pytest_raises_clusters
            } else if self.src.guard(current) && self.src.guard(next) {
                settings.allow_compact_guard_ladders
            } else {
                true
            };
            if allowed {
                return false;
            }
        }
        if self.src.guard(current) && self.src.assignment(next) {
            return false;
        }
        if self.same_subject(current, next) {
            return false;
        }
        if settings.allow_compact_guard_ladders
            && self.src.guard(current)
            && (self.src.guard(next) || self.src.branch(next))
        {
            return false;
        }
        if !self.src.guard(current)
            && !matches!(current, Stmt::With(_))
            && !self.src.branch(next)
            && !self.src.assignment(next)
            && simple(next)
        {
            let assigned: Names = self
                .src
                .groups(current)
                .into_iter()
                .flatten()
                .filter(|s| self.src.assignment(s))
                .flat_map(assigned)
                .collect();
            if self
                .src
                .leading(&body[index + 1..], 1)
                .iter()
                .any(|next| !refs(next, false).is_disjoint(&assigned))
            {
                return false;
            }
        }
        if settings.allow_with_immediate_inspection && !pytest(current) {
            if let Stmt::With(s) = current {
                let statements = self.src.primary(current);
                if !statements.is_empty() && statements.iter().all(simple) {
                    let mut assigned_names = bound(current);
                    for statement in statements {
                        assigned_names.extend(self.src.assigned(statement));
                    }
                    if self
                        .src
                        .leading(&body[index + 1..], 1)
                        .iter()
                        .any(|next| !assigned_names.is_disjoint(&refs(next, false)))
                    {
                        return false;
                    }
                }
                let _ = s;
            }
        }
        if !self.src.guard(current)
            && self.src.assignment(next)
            && related_use(&self.src, body, index + 1, settings.related_use_lookahead)
        {
            return false;
        }
        if matches!(current, Stmt::If(_) | Stmt::Match(_) | Stmt::Try(_))
            && self
                .src
                .primary(current)
                .last()
                .is_some_and(|last| matches!(last, Stmt::Continue(_)) && self.src.branch(last))
        {
            return false;
        }
        if pass_try(current) && simple(next) {
            return false;
        }
        true
    }
    fn same_subject(&self, current: &Stmt, next: &Stmt) -> bool {
        match (current, next) {
            (Stmt::If(a), Stmt::If(b))
                if a.elif_else_clauses.is_empty()
                    && b.elif_else_clauses.is_empty()
                    && a.body.len() == 1
                    && b.body.len() == 1
                    && !self.src.inline(current)
                    && !self.src.inline(next) =>
            {
                if let (Expr::Compare(a), Expr::Compare(b)) = (a.test.as_ref(), b.test.as_ref())
                    && let (Some((left_a, _, _)), Some((left_b, _, _))) =
                        (a.as_single(), b.as_single())
                {
                    self.src.equal(left_a, left_b)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
    fn after_terminal(&self, body: &[Stmt], index: usize) -> bool {
        let current = &body[index];
        let next = &body[index + 1];
        if !control(current)
            || self.src.inline(current)
            || header(next)
            || pass_handler(current)
            || !self
                .src
                .groups(current)
                .iter()
                .any(|g| g.last().is_some_and(|last| self.src.branch(last)))
        {
            return false;
        }
        if self
            .options
            .after_terminal_control_block
            .allow_compact_guard_ladders
            && self.src.guard(current)
            && (self.src.guard(next)
                || (self.src.branch(next)
                    && index > 0
                    && !self.src.separator(&body[index - 1], current)
                    && self.src.guard(&body[index - 1])))
        {
            return false;
        }
        true
    }
    fn cleanup(&self, body: &[Stmt], start: usize, parent: Parent) -> bool {
        parent == Parent::Cleanup
            && body.last().is_some_and(|s| self.src.branch(s))
            && body[start..].iter().all(simple)
    }
    fn receiver_setup(&self, body: &[Stmt], index: usize) -> bool {
        shared_receivers(
            &self.src,
            self.src.setup(&body[index - 1]),
            targets(&body[index]),
        )
    }
    fn simple_return_tail(&self, body: &[Stmt], index: usize, parent: Parent) -> bool {
        parent == Parent::Function
            && index + 2 == body.len()
            && matches!(&body[index + 1], Stmt::Return(_))
            && self.src.single(&body[index + 1])
            && direct_branch(&body[index + 1], &assigned(&body[index]))
            && simple(&body[index - 1])
            && self.src.single(&body[index - 1])
            && (index == 1 || self.src.separator(&body[index - 2], &body[index - 1]))
            && self.src.count(body[index - 1].range()) == 1
            && self.src.count(body[index].range()) == 1
    }
    fn assignment(
        &mut self,
        body: &[Stmt],
        index: usize,
        parent: Parent,
        anchor: TextSize,
        scope: ScopeKind,
    ) {
        let settings = &self.options.before_assignment;
        let previous = &body[index - 1];
        let statement = &body[index];
        let compact =
            self.receiver_setup(body, index) || self.simple_return_tail(body, index, parent);
        if compact {
            self.between(previous, statement, anchor, SpacingRule::BeforeAssignment, 0, "Unnecessary blank line before assignment statement that continues a compact local flow.", false);
            return;
        }
        let related = related_use(&self.src, body, index, settings.related_use_lookahead);
        if self.src.assignment(previous)
            || (parent.docstring() && index == 1 && docstring(&body[0]))
            || self.cleanup(body, index, parent)
            || next_consumes(&self.src, body, index, settings.related_use_lookahead)
            || (settings.allow_local_helper_capture
                && body
                    .get(index + 1)
                    .is_some_and(|next| definition(next) && captures(statement, next, scope)))
            || (settings.allow_post_guard_continuation
                && self.src.guard(previous)
                && (related
                    || body.get(index + 1).is_some_and(|next| {
                        self.src.single(next) && direct_branch(next, &assigned(statement))
                    })))
            || (related && !self.src.guard(previous))
        {
            return;
        }
        self.between(previous, statement, anchor, SpacingRule::BeforeAssignment, 1, "Missing blank line before assignment statement that follows a non-assignment statement.", false);
    }
    fn annotated_return(&self, body: &[Stmt], index: usize) -> bool {
        matches!((&body[index - 1], &body[index]), (Stmt::AnnAssign(a), Stmt::Return(r)) if a.value.is_some() && self.src.single(&body[index - 1]) && matches!((a.target.as_ref(), r.value.as_deref()), (Expr::Name(a), Some(Expr::Name(r))) if a.id == r.id))
    }
    fn branch_gap(&mut self, body: &[Stmt], index: usize, parent: Parent, anchor: TextSize) {
        let settings = &self.options.before_branch;
        let previous = &body[index - 1];
        let statement = &body[index];
        if settings.allow_related_return_tails && self.annotated_return(body, index) {
            self.between(previous, statement, anchor, SpacingRule::BeforeBranch, 0, "Unnecessary blank line before returning an immediately preceding annotated binding.", true);
            return;
        }
        if control(previous)
            && (settings.allow_guard_ladder_final_branch || !self.src.ladder_tail(body, index))
        {
            return;
        }
        if (parent.docstring() && index == 1 && docstring(&body[0]))
            || self.cleanup(body, index - 1, parent)
        {
            return;
        }
        let run = &body[self.src.run_start(body, index)..index];
        if matches!(statement, Stmt::Continue(_) | Stmt::Break(_))
            && !self.src.separator(previous, statement)
            && (1..=4).contains(&self.src.statement_count(run))
            && run.iter().all(simple)
        {
            return;
        }
        if settings.allow_related_return_tails {
            if index > 1
                && control(&body[index - 2])
                && direct_branch(statement, &self.src.assigned(previous))
            {
                return;
            }
            if index + 1 == body.len()
                && !run.is_empty()
                && self.src.statement_count(run) <= settings.compact_tail_max_statements
                && run.iter().all(simple)
            {
                let assigned: Names = run.iter().flat_map(|s| self.src.assigned(s)).collect();
                if !refs(statement, false).is_disjoint(&assigned) {
                    return;
                }
            }
        }
        self.between(
            previous,
            statement,
            anchor,
            SpacingRule::BeforeBranch,
            1,
            "Add a blank line before this branch statement in a larger code block.",
            true,
        );
    }
    fn related_name(&self, block: &Stmt, name: &str, strict: bool) -> bool {
        if headers(block)
            .into_iter()
            .any(|expr| expr_names(expr).contains(name))
        {
            return true;
        }
        if strict || bound(block).contains(name) {
            return false;
        }
        let limit = self.options.before_unrelated_block.body_usage_lookahead;
        if limit == 0 {
            return false;
        }
        if matches!(block, Stmt::If(_)) {
            if early_consumed(&self.src, block, limit).contains(name) {
                return true;
            }
        } else if self
            .src
            .leading(self.src.primary(block), 1)
            .iter()
            .any(|stmt| names(stmt.into()).contains(name))
        {
            return true;
        }
        self.src
            .leading(self.src.primary(block), limit)
            .iter()
            .any(|stmt| self.src.assigned(stmt).contains(name) || refs(stmt, false).contains(name))
    }
    fn related_target(&self, block: &Stmt, target: &Expr, strict: bool) -> bool {
        if headers(block)
            .into_iter()
            .any(|expr| contains_expr(&self.src, expr.into(), target))
        {
            return true;
        }
        if strict || matches!(target, Expr::Name(n) if bound(block).contains(n.id.as_str())) {
            return false;
        }
        self.src
            .leading(
                self.src.primary(block),
                self.options.before_unrelated_block.body_usage_lookahead,
            )
            .iter()
            .any(|stmt| contains_expr(&self.src, stmt.into(), target))
    }
    fn related_assignment(&self, assignment: &Stmt, block: &Stmt, strict: bool) -> bool {
        self.src.assignment(assignment)
            && (assigned_nodes(assignment)
                .iter()
                .any(|n| self.related_name(block, n.id.as_str(), strict))
                || targets(assignment)
                    .iter()
                    .any(|t| self.target_related_recursive(block, t, strict)))
    }
    fn target_related_recursive(&self, block: &Stmt, target: &Expr, strict: bool) -> bool {
        match target {
            Expr::Tuple(s) => s
                .elts
                .iter()
                .any(|e| self.target_related_recursive(block, e, strict)),
            Expr::List(s) => s
                .elts
                .iter()
                .any(|e| self.target_related_recursive(block, e, strict)),
            Expr::Starred(s) => self.target_related_recursive(block, &s.value, strict),
            Expr::Name(_) | Expr::Subscript(_) | Expr::Attribute(_) => {
                self.related_target(block, target, strict)
            }
            _ => false,
        }
    }
    fn continuation(&self, statement: &Stmt, name: &str) -> bool {
        if !self.src.single(statement) {
            return false;
        }
        expression(statement).is_some_and(|expr| { let expr = if let Expr::Call(c) = expr { c.func.as_ref() } else { expr }; matches!(expr, Expr::Attribute(a) if matches!(a.value.as_ref(), Expr::Name(n) if n.id.as_str() == name)) })
    }
    fn allowed_header(&self, body: &[Stmt], index: usize, strict: bool) -> bool {
        let previous = &body[index - 1];
        let block = &body[index];
        if self.related_assignment(previous, block, strict) {
            return true;
        }
        if strict {
            return false;
        }
        if self.src.single(previous) {
            if let Some(Expr::Call(call)) = expression(previous) {
                if let Expr::Attribute(attribute) = call.func.as_ref() {
                    let related = refs(previous, false);
                    if expr_names(&attribute.value)
                        .intersection(&related)
                        .any(|name| self.related_name(block, name, false))
                    {
                        return true;
                    }
                }
            }
        }
        if index >= 2
            && self.src.assignment(&body[index - 2])
            && last_name(&body[index - 2]).is_some_and(|name| {
                self.continuation(previous, name) && self.related_name(block, name, false)
            })
        {
            return true;
        }
        if self.src.guard(block) && self.src.guard(previous) && self.src.ladder(body, index - 1) {
            return true;
        }
        if shared_receivers(&self.src, self.src.setup(previous), headers(block)) {
            return true;
        }
        let settings = &self.options.before_unrelated_block;
        if settings.setup_run_lookback == 0 {
            return false;
        }
        let start = self
            .src
            .run_start(body, index)
            .max(index.saturating_sub(settings.setup_run_lookback));
        let run = &body[start..index];
        let assignments: Vec<_> = run
            .iter()
            .enumerate()
            .filter(|(_, stmt)| self.src.assignment(stmt))
            .collect();
        if let [(position, assignment)] = assignments.as_slice() {
            let Some(name) = last_name(assignment) else {
                return false;
            };
            let trailing = &run[*position + 1..];
            if *position == 0
                && trailing.is_empty()
                && settings.allow_setup_before_compact_guard_ladder
                && self.src.ladder(body, index)
            {
                return true;
            }
            return !trailing.is_empty()
                && trailing.iter().all(|s| self.continuation(s, name))
                && self.related_name(block, name, false);
        }
        false
    }
}
