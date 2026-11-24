use std::path::Path;

use ruff_db::diagnostic::{
    Annotation, Diagnostic, DiagnosticId, LintName, SecondaryCode, Severity, Span,
};
use ruff_diagnostics::{Edit, Fix};
use ruff_python_ast::{ModModule, PythonVersion};
use ruff_python_parser::Parsed;
use ruff_python_trivia::CommentRanges;
use ruff_source_file::LineRanges;
use ruff_text_size::{Ranged, TextRange};
use rustc_hash::FxHashMap;
use tuff_lint::builtin::spacing::{self, SpacingRule};
use tuff_lint::{Context, NameResolver};
use tuff_python_rules::CheckedFile;

use crate::checkers::ast::{Checker, LintContext};
use crate::registry::{Rule, RuleSet};
use crate::rules::tuff;
use crate::settings::LinterSettings;
use crate::settings::rule_table::RuleTable;

pub(crate) struct RuleReplacement {
    pub rule: Rule,
    pub name: &'static str,
    pub replaced: &'static [Rule],
    pub versions: tuff_lint::VersionRange,
}

pub(crate) fn replace_overlaps(
    rules: &mut RuleTable,
    settings: &LinterSettings,
    version: PythonVersion,
) {
    for replacement in crate::codes::TUFF_REPLACEMENTS {
        if rules.enabled(replacement.rule)
            && replacement.versions.contains(version)
            && settings
                .tuff
                .registry
                .active(replacement.name, version)
                .is_ok_and(|active| active)
        {
            for rule in replacement.replaced {
                rules.disable(*rule);
            }
        }
    }
}

pub(crate) fn replace_spacing_overlaps(context: &mut LintContext<'_>, source: &str) {
    let decisions = context.take_spacing_decisions();
    if decisions.is_empty() {
        return;
    }
    context.as_mut_vec().retain(|diagnostic| {
        let Some(name) = diagnostic.id().as_lint() else {
            return true;
        };
        if !matches!(
            Rule::from_name(name.as_str()),
            Ok(Rule::MissingBlankLineBeforeMethod
                | Rule::MissingBlankLinesBeforeTopLevelDefinition
                | Rule::TooManyBlankLines
                | Rule::MissingBlankLinesAfterTopLevelDefinition
                | Rule::MissingBlankLineBeforeNestedDefinition
                | Rule::TooManyNewlinesAtEndOfFile)
        ) {
            return true;
        }
        !diagnostic.range().is_some_and(|range| {
            decisions.iter().any(|decision| {
                decision.range.contains_inclusive(range.start())
                    || source.line_start(range.start())
                        == source.line_start(decision.diagnostic_range.start())
            })
        })
    });
}

pub(crate) fn check(
    checker: &Checker<'_>,
    parsed: &Parsed<ModModule>,
    path: &Path,
    context: &LintContext<'_>,
) {
    check_python_rules(checker, parsed, path, context);
    if !context.any_rule_enabled(tuff::NATIVE_RULES) {
        return;
    }
    let module = parsed.syntax();
    let settings = checker.settings();
    let spacing_rules: Vec<_> = SpacingRule::ALL
        .iter()
        .copied()
        .filter(|rule| {
            tuff::rule_for_name(rule.name()).is_some_and(|rule| context.is_rule_enabled(rule))
        })
        .collect();
    let spacing_enabled = !spacing_rules.is_empty();
    let printed = if spacing_enabled {
        match tuff_python_formatter::measure_module_layout(
            parsed,
            checker.source(),
            settings
                .tuff_format
                .clone()
                .with_source_type(checker.source_type)
                .with_target_version(checker.target_version()),
        ) {
            Ok(printed) => Some(printed),
            Err(error) => {
                report_error(
                    context,
                    DiagnosticId::Panic,
                    format!("Cannot measure blank-line layout: {error}"),
                    module.range(),
                );
                return;
            }
        }
    } else {
        None
    };
    let metrics = printed
        .as_ref()
        .map(|printed| tuff_python_formatter::SpacingMetrics::new(checker.source(), printed));
    let native = Context {
        module,
        source: checker.source(),
        path,
        root: &settings.tuff.root,
        python_version: checker.target_version(),
        resolver: checker.semantic(),
        ancestors: &[],
        rendered_metrics: metrics
            .as_ref()
            .map(|metrics| metrics as &dyn tuff_lint::builtin::spacing::RenderedMetrics),
    };
    let mut findings =
        settings
            .tuff
            .registry
            .check(&native, checker.indexer().comment_ranges(), &|definition| {
                !SpacingRule::ALL
                    .iter()
                    .any(|rule| rule.name() == definition.name)
                    && tuff::rule_for_name(definition.name)
                        .is_some_and(|rule| context.is_rule_enabled(rule))
            });
    if spacing_enabled {
        match spacing::gaps(
            module,
            checker.source(),
            &spacing_rules,
            &settings.tuff_format.custom().blank_lines.options,
            native.rendered_metrics,
        ) {
            Ok(decisions) => {
                findings.extend(
                    decisions
                        .iter()
                        .filter_map(|decision| spacing::finding(decision, checker.source())),
                );
                context.set_spacing_decisions(decisions);
            }
            Err(error) => {
                report_error(
                    context,
                    DiagnosticId::Panic,
                    error.to_string(),
                    module.range(),
                );
                return;
            }
        }
    }
    if let Some(metrics) = metrics
        && let Err(error) = metrics.validate()
    {
        report_error(
            context,
            DiagnosticId::Panic,
            error.to_string(),
            module.range(),
        );
        return;
    }
    for finding in findings {
        tuff::report(context, finding);
    }
}

fn check_python_rules(
    checker: &Checker<'_>,
    parsed: &Parsed<ModModule>,
    path: &Path,
    context: &LintContext<'_>,
) {
    let settings = checker.settings();
    let Some(rules) = &settings.tuff.python_rules else {
        return;
    };
    // CPython cannot parse the cell-joined source of a notebook.
    if !rules.has_active_rules() || checker.source_type.is_ipynb() {
        return;
    }
    let semantic = checker.semantic();
    let report = rules.check(&CheckedFile {
        path,
        source: checker.source(),
        module: parsed.syntax(),
        comments: checker.indexer().comment_ranges(),
        qualified_name: &|expression| semantic.qualified_name(expression),
        is_ignored: &|name| settings.tuff_per_file_ignores.ignores(path, name),
    });
    for failure in report.failures {
        let message = match failure.rule {
            Some(rule) => format!("Python rule `{rule}` failed: {}", failure.message),
            None => format!("Python rules failed: {}", failure.message),
        };
        let mut diagnostic = Diagnostic::new(DiagnosticId::Panic, Severity::Error, message);
        let mut annotation = Annotation::primary(
            Span::from(context.source_file().clone()).with_range(TextRange::default()),
        );
        annotation.hide_snippet(true);
        diagnostic.annotate(annotation);
        context.push_diagnostic(diagnostic);
    }
    for finding in report.findings {
        let mut diagnostic = Diagnostic::new(
            DiagnosticId::Lint(LintName::of(finding.rule)),
            Severity::Error,
            finding.message,
        );
        let mut annotation = Annotation::primary(
            Span::from(context.source_file().clone()).with_range(finding.range),
        );
        if finding.range == TextRange::default() {
            annotation.hide_snippet(true);
        }
        diagnostic.annotate(annotation);
        diagnostic.set_secondary_code(SecondaryCode::new(finding.rule.to_owned()));
        let mut edits = finding.edits.into_iter().map(|edit| {
            if edit.replacement.is_empty() {
                Edit::range_deletion(edit.range)
            } else {
                Edit::range_replacement(edit.replacement, edit.range)
            }
        });
        if let Some(first) = edits.next() {
            diagnostic.set_fix(Fix::safe_edits(first, edits));
        }
        context.push_diagnostic(diagnostic);
    }
}

pub(crate) fn report_unknown_rule(context: &LintContext<'_>, name: &str, range: TextRange) {
    report_error(
        context,
        DiagnosticId::UnknownRule,
        format!("Native rule reported an unregistered rule name `{name}`"),
        range,
    );
}

fn report_error(context: &LintContext<'_>, id: DiagnosticId, message: String, range: TextRange) {
    let mut diagnostic = Diagnostic::new(id, Severity::Error, message);
    diagnostic.annotate(Annotation::primary(
        Span::from(context.source_file().clone()).with_range(range),
    ));
    context.push_diagnostic(diagnostic);
}

pub(crate) struct Suppressions {
    source: tuff_lint::suppression::Suppressions,
    rules: FxHashMap<String, RuleSet>,
}

pub(crate) fn suppressions(
    module: &ModModule,
    source: &str,
    comments: &CommentRanges,
    context: &LintContext<'_>,
) -> Option<Suppressions> {
    let source = match tuff_lint::suppression::Suppressions::from_module(module, source, comments) {
        Ok(source) => source,
        Err(error) => {
            report_error(
                context,
                DiagnosticId::Panic,
                error.to_string(),
                module.range(),
            );
            return None;
        }
    };
    if source.directives.is_empty() {
        return None;
    }
    let mut rules = FxHashMap::default();
    for directive in &source.directives {
        for name in directive.names.iter().flatten() {
            if rules.contains_key(name) {
                continue;
            }
            if let Some(selected) = resolve_name(name) {
                rules.insert(name.clone(), selected);
            } else if !is_python_rule(context.settings(), name) {
                report_error(
                    context,
                    DiagnosticId::UnknownRule,
                    format!("Unknown rule `{name}` in Tuff ignore comment"),
                    directive.comment,
                );
            }
        }
    }
    Some(Suppressions { source, rules })
}

fn is_python_rule(settings: &LinterSettings, name: &str) -> bool {
    settings
        .tuff
        .python_rules
        .as_ref()
        .is_some_and(|rules| rules.rule_name(name).is_some())
}

fn resolve_name(name: &str) -> Option<RuleSet> {
    Rule::from_name(name)
        .ok()
        .map(|rule| [rule].into_iter().collect())
}

pub(crate) fn apply_suppressions(context: &mut LintContext<'_>, suppressions: &Suppressions) {
    let python_rules = context.settings().tuff.python_rules.clone();
    context.as_mut_vec().retain(|diagnostic| {
        let (Some(name), Some(range)) = (diagnostic.id().as_lint(), diagnostic.range()) else {
            return true;
        };
        if let Ok(rule) = Rule::from_name(name.as_str()) {
            return !suppressions.source.suppresses(range, |name| {
                suppressions
                    .rules
                    .get(name)
                    .is_some_and(|rules| rules.contains(rule))
            });
        }
        let is_python_rule = python_rules
            .as_ref()
            .is_some_and(|rules| rules.rule_name(name.as_str()).is_some());
        !(is_python_rule
            && suppressions
                .source
                .suppresses(range, |ignored| ignored == name.as_str()))
    });
}
