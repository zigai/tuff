//! Applies the native spacing planner's constraints at Python newline ownership sites.
//!
//! The dry print is measurement only: final output is always printed from formatting IR.
use std::cell::Cell;
use std::collections::BTreeMap;

use ruff_formatter::{FormatError, FormatResult, Printed};
use ruff_python_ast::ModModule;
use ruff_source_file::{LineRanges, UniversalNewlines};
use ruff_text_size::{TextRange, TextSize};
use tuff_lint::builtin::spacing::{self, GapDecision, GapKind, RenderedMetrics, SpacingPolicy};

use crate::PyFormatter;
use crate::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct BlankLinePlan {
    decisions: Vec<GapDecision>,
}

impl BlankLinePlan {
    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
    }

    pub(crate) fn build(
        module: &ModModule,
        source: &str,
        options: &SpacingPolicy,
        rendered: &Printed,
    ) -> FormatResult<Self> {
        let metrics = SpacingMetrics::new(source, rendered);
        let decisions = spacing::gaps(
            module,
            source,
            &options.rules,
            &options.options,
            Some(&metrics),
        )
        .map_err(|_| conflict())?;
        // Never silently fall back to source density. Missing markers indicate a formatter
        // bug, not permission to make width-sensitive rules weaker.
        metrics.validate()?;
        Ok(Self { decisions })
    }

    pub(crate) fn between(
        &self,
        kind: GapKind,
        previous: TextSize,
        next: TextSize,
    ) -> Option<usize> {
        self.decisions
            .iter()
            .filter(|d| {
                d.anchor.kind == kind
                    && d.anchor.previous == Some(previous)
                    && d.anchor.next == Some(next)
            })
            .map(|d| d.blank_lines)
            .max()
    }

    pub(crate) fn owns_case_separator_at(&self, offset: TextSize) -> bool {
        self.decisions
            .iter()
            .any(|d| d.anchor.kind == GapKind::BetweenCases && d.range.start() == offset)
    }

    pub(crate) fn leading(&self, first: TextSize) -> Option<usize> {
        self.decisions
            .iter()
            .find(|d| d.anchor.kind == GapKind::SuiteLeading && d.anchor.next == Some(first))
            .map(|d| d.blank_lines)
    }

    pub(crate) fn trailing(&self, last: TextSize) -> Option<usize> {
        self.decisions
            .iter()
            .find(|d| d.anchor.kind == GapKind::SuiteTrailing && d.anchor.previous == Some(last))
            .map(|d| d.blank_lines)
    }

    // A comment hook may only override a gap it actually owns. In particular, a
    // suite boundary must not erase whitespace *inside* a comment group.
    pub(crate) fn at(&self, offset: TextSize) -> Option<usize> {
        self.decisions
            .iter()
            .filter(|d| d.range.contains_inclusive(offset))
            .map(|d| d.blank_lines)
            .max()
    }
}

pub(crate) fn conflict() -> FormatError {
    FormatError::syntax_error(
        "blank-line-before-unrelated-block and block-header-cuddle-strict are mutually exclusive",
    )
}

// Consecutive hard line breaks deliberately coalesce in the printer;
// `empty_line` is the blank-line primitive.
pub(crate) fn separator(blank_lines: usize, f: &mut PyFormatter) -> FormatResult<()> {
    if blank_lines == 0 {
        hard_line_break().fmt(f)
    } else {
        for _ in 0..blank_lines {
            empty_line().fmt(f)?;
        }
        Ok(())
    }
}

pub(crate) fn before_comment(offset: TextSize, f: &PyFormatter) -> Option<usize> {
    f.context()
        .custom_layout()
        .blank_lines
        .at(f.context().source().line_start(offset))
}

pub(crate) fn after_comment(offset: TextSize, f: &PyFormatter) -> Option<usize> {
    f.context()
        .custom_layout()
        .blank_lines
        .at(f.context().source().full_line_end(offset))
}

// Unowned outer-suite separator lines remain intact. The planner owns the ranges
// removed from source-based whitespace preservation.
pub(crate) fn lines_before(offset: TextSize, f: &PyFormatter) -> u32 {
    let source = f.context().source();
    let mut lines = ruff_python_trivia::lines_before(offset, source);
    let mut cursor = source.line_start(offset);
    let plan = &f.context().custom_layout().blank_lines;
    if plan.is_empty() {
        return lines;
    }
    while cursor > TextSize::new(0) {
        let start = source.line_start(cursor - TextSize::new(1));
        if !source[TextRange::new(start, cursor)].trim().is_empty() {
            break;
        }
        if plan.decisions.iter().any(|d| {
            d.anchor.kind == GapKind::SuiteTrailing
                && d.blank_lines == 0
                && d.range.start() <= start
                && cursor <= d.range.end()
        }) {
            lines = lines.saturating_sub(1);
        }
        cursor = start;
    }
    lines
}

/// Marker positions can repeat (for example a statement and its first child).
/// Retain both edges rather than relying on printer marker order.
pub struct SpacingMetrics<'a> {
    source: &'a str,
    rendered: &'a str,
    positions: BTreeMap<TextSize, (TextSize, TextSize)>,
    missing: Cell<bool>,
}

impl<'a> SpacingMetrics<'a> {
    pub fn new(source: &'a str, printed: &'a Printed) -> Self {
        let mut positions = BTreeMap::new();
        for marker in printed.sourcemap() {
            positions
                .entry(marker.source)
                .and_modify(|edges: &mut (TextSize, TextSize)| {
                    edges.0 = edges.0.min(marker.dest);
                    edges.1 = edges.1.max(marker.dest);
                })
                .or_insert((marker.dest, marker.dest));
        }
        Self {
            source,
            rendered: printed.as_code(),
            positions,
            missing: Cell::new(false),
        }
    }

    pub fn validate(&self) -> FormatResult<()> {
        if self.missing.get() {
            Err(FormatError::syntax_error(
                "could not map a blank-line density span to the rendered layout",
            ))
        } else {
            Ok(())
        }
    }

    fn map(&self, offset: TextSize, end: bool) -> Option<TextSize> {
        if let Some(&(first, last)) = self.positions.get(&offset) {
            return Some(if end { last } else { first });
        }
        // Suppressed statements aren't visited individually. Their verbatim logical
        // lines *are* marked. Only interpolate an unchanged prefix (with newline
        // normalization); an enclosing end marker can include a continued semicolon.
        let (&source_start, &(_, dest_start)) = self.positions.range(..offset).next_back()?;
        let (_, &(dest_end, _)) = self.positions.range(offset..).next()?;
        if dest_start > dest_end {
            return None;
        }
        let original = &self.source[TextRange::new(source_start, offset)];
        let output = &self.rendered[TextRange::new(dest_start, dest_end)];
        let normalize = |s: &str| s.replace("\r\n", "\n").replace('\r', "\n");
        let prefix = normalize(original);
        if !normalize(output).starts_with(&prefix) {
            return None;
        }
        let prefix = prefix.len();
        let mut normalized = 0;
        let mut bytes = 0;
        while normalized < prefix {
            let byte = *output.as_bytes().get(bytes)?;
            bytes += 1;
            if byte == b'\r' && output.as_bytes().get(bytes) == Some(&b'\n') {
                bytes += 1;
            }
            normalized += 1;
        }
        Some(dest_start + TextSize::try_from(bytes).ok()?)
    }
}

impl RenderedMetrics for SpacingMetrics<'_> {
    fn is_own_line(&self, offset: TextSize) -> Option<bool> {
        if let Some(offset) = self.map(offset, false) {
            Some(
                self.rendered[TextRange::new(self.rendered.line_start(offset), offset)]
                    .trim()
                    .is_empty(),
            )
        } else {
            self.missing.set(true);
            Some(false)
        }
    }
    fn same_line(&self, left: TextSize, right: TextSize) -> Option<bool> {
        if let Some((left, right)) = self.map(left, true).zip(self.map(right, false)) {
            Some(self.rendered.line_start(left) == self.rendered.line_start(right))
        } else {
            self.missing.set(true);
            Some(false)
        }
    }
    fn non_empty_lines(&self, range: TextRange) -> Option<usize> {
        let mapped = self
            .map(range.start(), false)
            .zip(self.map(range.end(), true));
        if let Some((start, end)) = mapped.filter(|(start, end)| start <= end) {
            let range =
                TextRange::new(self.rendered.line_start(start), self.rendered.line_end(end));
            Some(
                self.rendered[range]
                    .universal_newlines()
                    .filter(|line| !line.trim().is_empty())
                    .count(),
            )
        } else {
            self.missing.set(true);
            // The build fails after planning. Always return Some so the native
            // planner never substitutes source-based density.
            Some(0)
        }
    }
}
