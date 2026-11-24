//! `tuff throws check`: exception-flow diagnostics.

use std::fmt::Write as _;

use std::path::PathBuf;

use anyhow::Result;
use ruff_text_size::TextRange;
use rustc_hash::FxHashSet;
use serde::Serialize;

use crate::config::SourceKind;
use crate::graph::{Analysis, FunctionId, Location, StepKind, Via};
use crate::hierarchy::ExceptionId;
use crate::inspect::{ChainStep, chain, render_chain};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Code {
    #[serde(rename = "THROWS001")]
    UnhandledBoundaryException,
    #[serde(rename = "THROWS002")]
    RedundantExcept,
    #[serde(rename = "THROWS003")]
    UndeclaredException,
    #[serde(rename = "THROWS004")]
    OverrideWidensThrows,
}

impl Code {
    pub const fn code(self) -> &'static str {
        match self {
            Code::UnhandledBoundaryException => "THROWS001",
            Code::RedundantExcept => "THROWS002",
            Code::UndeclaredException => "THROWS003",
            Code::OverrideWidensThrows => "THROWS004",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Code::UnhandledBoundaryException => "unhandled-boundary-exception",
            Code::RedundantExcept => "redundant-except",
            Code::UndeclaredException => "undeclared-exception",
            Code::OverrideWidensThrows => "override-widens-throws",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub code: Code,
    pub name: &'static str,
    pub message: String,
    pub location: Location,
    /// Absolute path of the reported file.
    #[serde(skip)]
    pub path: PathBuf,
    /// The reported `raise` statement, call, handler type, or function name.
    #[serde(skip)]
    pub range: TextRange,
    pub exception: String,
    pub chain: Vec<ChainStep>,
}

#[derive(Debug, Default)]
pub struct CheckOptions {
    /// Report only on these project-relative files or directories. Empty means everything.
    pub paths: Vec<String>,
    /// Exception base classes to track instead of the configured `track`.
    pub track: Vec<String>,
}

pub fn check(analysis: &Analysis, options: &CheckOptions) -> Vec<Diagnostic> {
    let checker = Checker {
        analysis,
        track: if options.track.is_empty() {
            &analysis.config.track
        } else {
            &options.track
        },
    };
    let mut diagnostics = Vec::new();
    for function in analysis.function_ids() {
        let path = &analysis.file(function).relative_path;
        if !options.paths.is_empty() && !options.paths.iter().any(|prefix| is_within(path, prefix))
        {
            continue;
        }
        checker.boundary(function, &mut diagnostics);
        checker.redundant_except(function, &mut diagnostics);
        checker.undeclared(function, &mut diagnostics);
        checker.override_widens(function, &mut diagnostics);
    }
    diagnostics.sort_by(|a, b| {
        (
            &a.location.path,
            a.location.line,
            a.location.column,
            a.code,
            &a.exception,
        )
            .cmp(&(
                &b.location.path,
                b.location.line,
                b.location.column,
                b.code,
                &b.exception,
            ))
    });
    diagnostics.dedup();
    diagnostics
}

fn is_within(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || prefix == "."
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

struct Checker<'a> {
    analysis: &'a Analysis,
    track: &'a [String],
}

impl Checker<'_> {
    /// Without a configured `track`, checks focus on the project's own exception classes.
    fn is_tracked(&self, exception: ExceptionId) -> bool {
        let hierarchy = &self.analysis.hierarchy;
        if self.track.is_empty() {
            hierarchy.is_project_defined(exception)
        } else {
            self.track
                .iter()
                .any(|pattern| hierarchy.derives_from_name(exception, pattern))
        }
    }

    /// The location of the first step of an exception's chain: the `raise` or call inside
    /// `function` that lets it escape.
    fn origin(&self, function: FunctionId, exception: ExceptionId) -> TextRange {
        let facts = self.analysis.facts(function);
        match self
            .analysis
            .chain(function, exception)
            .first()
            .map(|step| &step.kind)
        {
            Some(StepKind::Raise { range } | StepKind::Call { range, .. }) => *range,
            _ => facts.name_range,
        }
    }

    fn is_suppressed(
        &self,
        function: FunctionId,
        ranges: &[TextRange],
        code: Code,
        exception: ExceptionId,
    ) -> bool {
        let file = self.analysis.file(function);
        let hierarchy = &self.analysis.hierarchy;
        let candidates = [
            code.code(),
            code.name(),
            hierarchy.display_name(exception),
            hierarchy.name(exception),
        ];
        ranges.iter().any(|range| {
            file.suppressions(*range)
                .any(|suppression| suppression.matches(&candidates))
        })
    }

    fn push(
        &self,
        diagnostics: &mut Vec<Diagnostic>,
        function: FunctionId,
        code: Code,
        exception: ExceptionId,
        range: TextRange,
        message: String,
    ) {
        let file = self.analysis.file(function);
        diagnostics.push(Diagnostic {
            code,
            name: code.name(),
            message,
            location: file.location(range.start()),
            path: file.path.clone(),
            range,
            exception: self.analysis.hierarchy.name(exception).to_string(),
            // A redundant handler has no exception flow to explain.
            chain: if code == Code::RedundantExcept {
                Vec::new()
            } else {
                chain(self.analysis, function, exception)
            },
        });
    }

    fn escaping(&self, function: FunctionId) -> impl Iterator<Item = ExceptionId> + '_ {
        self.analysis
            .result(function)
            .exceptions
            .iter()
            .filter(|(exception, trace)| {
                !matches!(trace.via, Via::Declared(_)) && self.is_tracked(**exception)
            })
            .map(|(exception, _)| *exception)
    }

    fn boundary(&self, function: FunctionId, diagnostics: &mut Vec<Diagnostic>) {
        let node = self.analysis.node(function);
        if !node.is_boundary {
            return;
        }
        let facts = self.analysis.facts(function);
        for exception in self.escaping(function) {
            let origin = self.origin(function, exception);
            let code = Code::UnhandledBoundaryException;
            if self.is_suppressed(function, &[origin, facts.name_range], code, exception) {
                continue;
            }
            let message = format!(
                "`{}` escapes boundary `{}` unhandled",
                self.analysis.hierarchy.display_name(exception),
                facts.qualname
            );
            self.push(diagnostics, function, code, exception, origin, message);
        }
    }

    fn redundant_except(&self, function: FunctionId, diagnostics: &mut Vec<Diagnostic>) {
        if !self.analysis.config.uses(SourceKind::Raise) {
            return;
        }
        let hierarchy = &self.analysis.hierarchy;
        let result = self.analysis.result(function);
        for (try_index, outcome) in result.tries.iter().enumerate() {
            if outcome.incomplete {
                continue;
            }
            for types in self
                .analysis
                .handler_types(function, try_index)
                .into_iter()
                .flatten()
            {
                for &(handled, range) in types {
                    // Code outside the project cannot raise the project's own exceptions (except
                    // through callbacks, which mark the try incomplete), so only those are
                    // checked.
                    if !hierarchy.is_project_defined(handled) || !self.is_tracked(handled) {
                        continue;
                    }
                    let reachable = outcome.reaching.iter().any(|&raised| {
                        hierarchy.is_subclass(raised, handled)
                            || hierarchy.is_subclass(handled, raised)
                    });
                    let code = Code::RedundantExcept;
                    if reachable || self.is_suppressed(function, &[range], code, handled) {
                        continue;
                    }
                    let message = format!(
                        "`{}` is never raised in the `try` block",
                        hierarchy.display_name(handled)
                    );
                    self.push(diagnostics, function, code, handled, range, message);
                }
            }
        }
    }

    fn undeclared(&self, function: FunctionId, diagnostics: &mut Vec<Diagnostic>) {
        let Some(contract) = self.analysis.contract(function) else {
            return;
        };
        if self.analysis.node(function).is_suppressing {
            return;
        }
        let hierarchy = &self.analysis.hierarchy;
        let facts = self.analysis.facts(function);
        for exception in self.escaping(function) {
            if contract
                .iter()
                .any(|&declared| hierarchy.is_subclass(exception, declared))
            {
                continue;
            }
            let origin = self.origin(function, exception);
            let code = Code::UndeclaredException;
            if self.is_suppressed(function, &[origin, facts.name_range], code, exception) {
                continue;
            }
            let message = format!(
                "`{}` raises `{}`, which its contract does not declare",
                facts.qualname,
                hierarchy.display_name(exception)
            );
            self.push(diagnostics, function, code, exception, origin, message);
        }
    }

    fn override_widens(&self, function: FunctionId, diagnostics: &mut Vec<Diagnostic>) {
        let Some(parent) = self.analysis.overridden(function) else {
            return;
        };
        let Some(parent_contract) = self.analysis.contract(parent) else {
            return;
        };
        let hierarchy = &self.analysis.hierarchy;
        let facts = self.analysis.facts(function);
        let parent_name = &self.analysis.facts(parent).qualname;
        let mut reported = FxHashSet::default();
        for &exception in self.analysis.result(function).exceptions.keys() {
            if !self.is_tracked(exception)
                || parent_contract
                    .iter()
                    .any(|&declared| hierarchy.is_subclass(exception, declared))
                || !reported.insert(exception)
            {
                continue;
            }
            let code = Code::OverrideWidensThrows;
            if self.is_suppressed(function, &[facts.name_range], code, exception) {
                continue;
            }
            let message = format!(
                "`{}` can raise `{}`, which the overridden `{parent_name}` does not declare",
                facts.qualname,
                hierarchy.display_name(exception)
            );
            self.push(
                diagnostics,
                function,
                code,
                exception,
                facts.name_range,
                message,
            );
        }
    }
}

pub fn render_text(diagnostics: &[Diagnostic]) -> String {
    let mut output = String::new();
    for diagnostic in diagnostics {
        let _ = writeln!(
            output,
            "{}: {} [{}] {}",
            diagnostic.location,
            diagnostic.code.code(),
            diagnostic.name,
            diagnostic.message
        );
        render_chain(&mut output, &diagnostic.chain, "    ");
    }
    let count = diagnostics.len();
    if count == 0 {
        output.push_str("No exception-flow problems found.\n");
    } else {
        let _ = writeln!(
            output,
            "Found {count} exception-flow problem{}.",
            if count == 1 { "" } else { "s" }
        );
    }
    output
}

pub fn render_json(diagnostics: &[Diagnostic]) -> Result<String> {
    Ok(serde_json::to_string_pretty(diagnostics)?)
}
