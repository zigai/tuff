//! `tuff throws inspect`: the exceptions a function can raise and where each comes from.

use std::fmt::Write as _;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::extract::RaiseKind;
use crate::graph::{Analysis, ContractSource, FunctionId, Location, StepKind};
use crate::hierarchy::ExceptionId;

/// One hop of a provenance chain, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainStep {
    pub function: String,
    pub location: Location,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectedException {
    pub name: String,
    pub qualified_name: String,
    /// Number of calls between the inspected function and the origin.
    pub depth: usize,
    pub chain: Vec<ChainStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectedFunction {
    pub function: String,
    pub symbol: String,
    pub location: Location,
    /// Whether every call made, directly or indirectly, was resolved.
    pub complete: bool,
    pub exceptions: Vec<InspectedException>,
}

/// Inspects every function matching `target` (`path::qualname`, `path`, or `module.qualname`).
///
/// `track` limits the output to exceptions deriving from one of the given classes; when empty,
/// the configured `track` applies, and when that is empty too, every exception is shown.
pub fn inspect(
    analysis: &Analysis,
    target: &str,
    track: &[String],
) -> Result<Vec<InspectedFunction>> {
    let functions = analysis.find(target);
    if functions.is_empty() {
        bail!("no function matches `{target}`");
    }
    let track = if track.is_empty() {
        analysis.config.track.as_slice()
    } else {
        track
    };
    Ok(functions
        .into_iter()
        .map(|function| inspect_function(analysis, function, track))
        .collect())
}

fn inspect_function(
    analysis: &Analysis,
    function: FunctionId,
    track: &[String],
) -> InspectedFunction {
    let facts = analysis.facts(function);
    let file = analysis.file(function);
    let result = analysis.result(function);
    let hierarchy = &analysis.hierarchy;
    let mut exceptions = result
        .exceptions
        .iter()
        .filter(|(exception, _)| {
            track.is_empty()
                || track
                    .iter()
                    .any(|pattern| hierarchy.derives_from_name(**exception, pattern))
        })
        .map(|(&exception, trace)| InspectedException {
            name: hierarchy.display_name(exception).to_string(),
            qualified_name: hierarchy.name(exception).to_string(),
            depth: trace.depth,
            chain: chain(analysis, function, exception),
        })
        .collect::<Vec<_>>();
    exceptions.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    InspectedFunction {
        function: format!("{}::{}", file.relative_path, facts.qualname),
        symbol: analysis.node(function).symbol.clone(),
        location: file.location(facts.name_range.start()),
        complete: !result.incomplete,
        exceptions,
    }
}

/// Renders the provenance chain of `exception` escaping `function`.
pub fn chain(analysis: &Analysis, function: FunctionId, exception: ExceptionId) -> Vec<ChainStep> {
    let name = analysis.hierarchy.display_name(exception);
    analysis
        .chain(function, exception)
        .into_iter()
        .map(|step| {
            let facts = analysis.facts(step.function);
            let file = analysis.file(step.function);
            let (offset, action) = match step.kind {
                StepKind::Raise { range } => {
                    let action = match facts
                        .raises
                        .iter()
                        .find(|raise| raise.range == range)
                        .map(|raise| &raise.kind)
                    {
                        Some(RaiseKind::Exception(exception)) => {
                            format!("raises `{}`", exception.text)
                        }
                        _ => format!("raises `{name}`"),
                    };
                    (range.start(), action)
                }
                StepKind::Call { range, .. } => {
                    let callee = facts
                        .calls
                        .iter()
                        .find(|call| call.range == range)
                        .map_or("?", |call| call.callee.as_str());
                    (range.start(), format!("calls `{callee}()`"))
                }
                StepKind::Declared(ContractSource::Decorator) => (
                    facts.name_range.start(),
                    format!("declares `{name}` with @throws"),
                ),
                StepKind::Declared(ContractSource::Docstring) => (
                    facts.name_range.start(),
                    format!("documents `{name}` in its docstring"),
                ),
            };
            ChainStep {
                function: facts.qualname.clone(),
                location: file.location(offset),
                action,
            }
        })
        .collect()
}

pub fn render_chain(output: &mut String, chain: &[ChainStep], indent: &str) {
    let function_width = chain
        .iter()
        .map(|step| step.function.len())
        .max()
        .unwrap_or_default();
    let location_width = chain
        .iter()
        .map(|step| step.location.to_string().len())
        .max()
        .unwrap_or_default();
    for (index, step) in chain.iter().enumerate() {
        let arrow = if index == 0 { " " } else { "→" };
        let _ = writeln!(
            output,
            "{indent}{arrow} {:function_width$}  {:location_width$}  {}",
            step.function,
            step.location.to_string(),
            step.action,
        );
    }
}

pub fn render_text(functions: &[InspectedFunction]) -> String {
    let mut output = String::new();
    for (index, function) in functions.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let count = function.exceptions.len();
        let _ = writeln!(
            output,
            "{} ({}) raises {count} exception{}",
            function.function,
            function.location,
            if count == 1 { "" } else { "s" }
        );
        for exception in &function.exceptions {
            output.push('\n');
            if exception.qualified_name.starts_with("builtins.") {
                let _ = writeln!(output, "  {}", exception.name);
            } else {
                let _ = writeln!(
                    output,
                    "  {} ({})",
                    exception.name, exception.qualified_name
                );
            }
            render_chain(&mut output, &exception.chain, "    ");
        }
        if !function.complete {
            let _ = writeln!(
                output,
                "\n  note: other exceptions may escape through unresolved calls or calls beyond the depth limit"
            );
        }
    }
    output
}

pub fn render_json(functions: &[InspectedFunction]) -> Result<String> {
    Ok(serde_json::to_string_pretty(functions)?)
}
