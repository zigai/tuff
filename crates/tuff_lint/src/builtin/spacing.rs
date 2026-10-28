use std::hash::Hash;

use anyhow::Result;
use ruff_cache::{CacheKey, CacheKeyHasher};
use ruff_diagnostics::{Edit, Fix};
use ruff_python_ast::{AnyNodeRef, ModModule, NodeKind};
use ruff_text_size::{TextRange, TextSize};
use serde::{Deserialize, Serialize};

use crate::{
    Context, Finding, LintRule, RuleDefinition, RuleOptions, parse_options, reject_options,
};

mod capture;
mod engine;
mod facts;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpacingRule {
    #[serde(rename = "blank-line-after-control-block")]
    AfterControlBlock,
    #[serde(rename = "blank-line-after-terminal-control-block")]
    AfterTerminalControlBlock,
    #[serde(rename = "blank-line-before-assignment")]
    BeforeAssignment,
    #[serde(rename = "blank-line-before-branch")]
    BeforeBranch,
    #[serde(rename = "blank-line-before-unrelated-block")]
    BeforeUnrelatedBlock,
    #[serde(rename = "block-header-cuddle-strict")]
    BlockHeaderCuddleStrict,
    #[serde(rename = "match-case-separation")]
    MatchCaseSeparation,
    #[serde(rename = "no-suite-leading-trailing-blank-lines")]
    NoSuiteLeadingTrailingBlankLines,
}

impl SpacingRule {
    pub const ALL: [Self; 8] = [
        Self::AfterControlBlock,
        Self::AfterTerminalControlBlock,
        Self::BeforeAssignment,
        Self::BeforeBranch,
        Self::BeforeUnrelatedBlock,
        Self::BlockHeaderCuddleStrict,
        Self::MatchCaseSeparation,
        Self::NoSuiteLeadingTrailingBlankLines,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::AfterControlBlock => "blank-line-after-control-block",
            Self::AfterTerminalControlBlock => "blank-line-after-terminal-control-block",
            Self::BeforeAssignment => "blank-line-before-assignment",
            Self::BeforeBranch => "blank-line-before-branch",
            Self::BeforeUnrelatedBlock => "blank-line-before-unrelated-block",
            Self::BlockHeaderCuddleStrict => "block-header-cuddle-strict",
            Self::MatchCaseSeparation => "match-case-separation",
            Self::NoSuiteLeadingTrailingBlankLines => "no-suite-leading-trailing-blank-lines",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AfterControlOptions {
    pub related_use_lookahead: usize,
    pub allow_compact_guard_ladders: bool,
    pub allow_pytest_raises_clusters: bool,
    pub allow_with_immediate_inspection: bool,
}
impl Default for AfterControlOptions {
    fn default() -> Self {
        Self {
            related_use_lookahead: 2,
            allow_compact_guard_ladders: true,
            allow_pytest_raises_clusters: true,
            allow_with_immediate_inspection: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AfterTerminalOptions {
    pub allow_compact_guard_ladders: bool,
}
impl Default for AfterTerminalOptions {
    fn default() -> Self {
        Self {
            allow_compact_guard_ladders: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BeforeAssignmentOptions {
    pub short_control_flow_max_statements: usize,
    pub related_use_lookahead: usize,
    pub allow_local_helper_capture: bool,
    pub allow_post_guard_continuation: bool,
}
impl Default for BeforeAssignmentOptions {
    fn default() -> Self {
        Self {
            short_control_flow_max_statements: 3,
            related_use_lookahead: 2,
            allow_local_helper_capture: true,
            allow_post_guard_continuation: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BeforeBranchOptions {
    pub max_suite_non_empty_lines: usize,
    pub compact_tail_max_statements: usize,
    pub allow_related_return_tails: bool,
    pub allow_guard_ladder_final_branch: bool,
}
impl Default for BeforeBranchOptions {
    fn default() -> Self {
        Self {
            max_suite_non_empty_lines: 2,
            compact_tail_max_statements: 2,
            allow_related_return_tails: true,
            allow_guard_ladder_final_branch: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeaderOptions {
    pub body_usage_lookahead: usize,
    pub setup_run_lookback: usize,
    pub allow_setup_before_compact_guard_ladder: bool,
}
impl Default for HeaderOptions {
    fn default() -> Self {
        Self {
            body_usage_lookahead: 4,
            setup_run_lookback: 3,
            allow_setup_before_compact_guard_ladder: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MatchCaseOptions {
    pub max_case_non_empty_lines: usize,
}
impl Default for MatchCaseOptions {
    fn default() -> Self {
        Self {
            max_case_non_empty_lines: 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpacingOptions {
    pub after_control_block: AfterControlOptions,
    pub after_terminal_control_block: AfterTerminalOptions,
    pub before_assignment: BeforeAssignmentOptions,
    pub before_branch: BeforeBranchOptions,
    pub before_unrelated_block: HeaderOptions,
    pub match_case_separation: MatchCaseOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpacingPolicy {
    pub rules: Vec<SpacingRule>,
    pub options: SpacingOptions,
}

impl SpacingPolicy {
    pub const fn is_disabled(&self) -> bool {
        self.rules.is_empty()
    }
}

impl CacheKey for SpacingRule {
    fn cache_key(&self, state: &mut CacheKeyHasher) {
        self.hash(state);
    }
}
impl CacheKey for SpacingOptions {
    fn cache_key(&self, state: &mut CacheKeyHasher) {
        self.hash(state);
    }
}
impl CacheKey for SpacingPolicy {
    fn cache_key(&self, state: &mut CacheKeyHasher) {
        self.hash(state);
    }
}

impl SpacingOptions {
    pub fn apply_rule(&mut self, rule: SpacingRule, input: &RuleOptions) -> Result<()> {
        match rule {
            SpacingRule::AfterControlBlock => self.after_control_block = parse_options(input)?,
            SpacingRule::AfterTerminalControlBlock => {
                self.after_terminal_control_block = parse_options(input)?
            }
            SpacingRule::BeforeAssignment => self.before_assignment = parse_options(input)?,
            SpacingRule::BeforeBranch => self.before_branch = parse_options(input)?,
            SpacingRule::BeforeUnrelatedBlock => {
                self.before_unrelated_block = parse_options(input)?
            }
            SpacingRule::MatchCaseSeparation => self.match_case_separation = parse_options(input)?,
            SpacingRule::BlockHeaderCuddleStrict
            | SpacingRule::NoSuiteLeadingTrailingBlankLines => reject_options(input)?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapKind {
    BetweenStatements,
    BetweenCases,
    SuiteLeading,
    SuiteTrailing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapAnchor {
    pub kind: GapKind,
    pub suite: TextSize,
    pub previous: Option<TextSize>,
    pub next: Option<TextSize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapDecision {
    pub anchor: GapAnchor,
    // Complete blank physical lines only; does not include the preceding code newline.
    pub range: TextRange,
    pub replacement: String,
    pub blank_lines: usize,
    pub rule: SpacingRule,
    pub reason: &'static str,
    pub diagnostic_range: TextRange,
    pub fixable: bool,
}

pub trait RenderedMetrics {
    // The range identifies an original AST statement or a first-to-last body span.
    // Return None to retain its source nonempty-line count.
    fn non_empty_lines(&self, original_range: TextRange) -> Option<usize>;
    fn same_line(&self, left: TextSize, right: TextSize) -> Option<bool>;
    fn is_own_line(&self, original_offset: TextSize) -> Option<bool>;
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("blank-line-before-unrelated-block and block-header-cuddle-strict are mutually exclusive")]
pub struct SpacingConflict;

pub fn validate(enabled: &[SpacingRule]) -> std::result::Result<(), SpacingConflict> {
    if enabled.contains(&SpacingRule::BeforeUnrelatedBlock)
        && enabled.contains(&SpacingRule::BlockHeaderCuddleStrict)
    {
        Err(SpacingConflict)
    } else {
        Ok(())
    }
}

// Returns applicable constraints, including already-satisfied constraints. Comments are
// never part of an edit. Unconstrained/exempt gaps are deliberately absent from the plan.
pub fn gaps(
    module: &ModModule,
    source: &str,
    enabled: &[SpacingRule],
    options: &SpacingOptions,
    metrics: Option<&dyn RenderedMetrics>,
) -> std::result::Result<Vec<GapDecision>, SpacingConflict> {
    validate(enabled)?;
    Ok(engine::plan(module, source, enabled, options, metrics))
}

#[derive(Debug)]
struct Adapter {
    rule: SpacingRule,
    options: SpacingOptions,
}
impl LintRule for Adapter {
    fn check(
        &self,
        node: AnyNodeRef<'_>,
        context: &Context<'_, '_, '_>,
        findings: &mut Vec<Finding>,
    ) {
        if !matches!(node, AnyNodeRef::ModModule(_)) {
            return;
        }
        let Ok(decisions) = gaps(
            context.module,
            context.source,
            &[self.rule],
            &self.options,
            context.rendered_metrics,
        ) else {
            return;
        };
        for decision in decisions {
            if let Some(finding) = finding(&decision, context.source) {
                findings.push(finding);
            }
        }
    }
}

pub fn finding(decision: &GapDecision, source: &str) -> Option<Finding> {
    if source[decision.range] == decision.replacement {
        return None;
    }
    let mut finding = Finding::new(
        decision.rule.name(),
        decision.diagnostic_range,
        decision.reason,
    );
    if decision.fixable {
        let edit = if decision.replacement.is_empty() {
            Edit::range_deletion(decision.range)
        } else {
            Edit::range_replacement(decision.replacement.clone(), decision.range)
        };
        finding = finding.with_fix(Fix::safe_edit(edit), "Adjust blank lines");
    }
    Some(finding)
}

fn factory(rule: SpacingRule, input: &RuleOptions) -> Result<Box<dyn LintRule>> {
    let mut options = SpacingOptions::default();
    options.apply_rule(rule, input)?;
    Ok(Box::new(Adapter { rule, options }))
}

pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! { name: "blank-line-after-control-block", code: "TUF101", explanation: "Separate multiline control-flow blocks from subsequent code.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::AfterControlBlock, o) },
    crate::native_rule! { name: "blank-line-after-terminal-control-block", code: "TUF102", explanation: "Separate early-exit control-flow blocks from subsequent code.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::AfterTerminalControlBlock, o) },
    crate::native_rule! { name: "blank-line-before-assignment", code: "TUF103", explanation: "Separate assignments that do not continue local flow.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::BeforeAssignment, o) },
    crate::native_rule! { name: "blank-line-before-branch", code: "TUF104", explanation: "Separate branch statements in larger suites.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::BeforeBranch, o) },
    crate::native_rule! { name: "blank-line-before-unrelated-block", code: "TUF105", explanation: "Separate unrelated block headers, allowing related setup.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::BeforeUnrelatedBlock, o) },
    crate::native_rule! { name: "block-header-cuddle-strict", code: "TUF106", explanation: "Allow only immediately preceding assignments used by block headers.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::BlockHeaderCuddleStrict, o) },
    crate::native_rule! { name: "match-case-separation", code: "TUF107", explanation: "Separate match cases after larger case bodies.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::MatchCaseSeparation, o) },
    crate::native_rule! { name: "no-suite-leading-trailing-blank-lines", code: "TUF108", explanation: "Remove unnecessary blank lines at suite boundaries.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: |o| factory(SpacingRule::NoSuiteLeadingTrailingBlankLines, o) },
];
