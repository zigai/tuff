use rustc_hash::FxHashMap;

use ruff_formatter::SourceCode;
use ruff_python_ast::token::Tokens;
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, Stmt};
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextRange};
use unicode_width::UnicodeWidthChar;

use crate::comments::Comments;
use crate::custom::diagnostics::{CustomLayoutDiagnostic, CustomLayoutError};
use crate::custom::options::{
    AlignmentMode, AssignmentAlignmentScope, ClassFieldAlignmentScope, DictAlignmentMode,
    FunctionParamAlignmentScope, TuffCustomOptions,
};
use crate::verbatim::{ends_suppression, starts_suppression};

#[derive(Clone, Debug, Default)]
pub struct TuffLayoutPlan {
    pub alignment: AlignmentPlan,
    pub collections: CollectionPlan,
    pub blank_lines: BlankLinePlan,
    pub diagnostics: Vec<CustomLayoutDiagnostic>,
}

impl TuffLayoutPlan {
    pub fn is_empty(&self) -> bool {
        self.alignment.is_empty()
            && self.collections.is_empty()
            && self.blank_lines.is_empty()
            && self.diagnostics.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct AlignmentPlan {
    pub class_fields: FxHashMap<TextRange, AlignmentDecision>,
    pub class_field_defaults: FxHashMap<TextRange, AlignmentDecision>,
    pub function_params: FxHashMap<TextRange, AlignmentDecision>,
    pub function_param_defaults: FxHashMap<TextRange, AlignmentDecision>,
    pub assignments: FxHashMap<TextRange, AlignmentDecision>,
    pub dict_values: FxHashMap<TextRange, AlignmentDecision>,
    pub call_keyword_args: FxHashMap<TextRange, AlignmentDecision>,
    pub import_aliases: FxHashMap<TextRange, AlignmentDecision>,
    pub collection_row_items: FxHashMap<TextRange, AlignmentDecision>,
    pub repeated_call_args: FxHashMap<TextRange, AlignmentDecision>,
    pub with_items: FxHashMap<TextRange, AlignmentDecision>,
    pub trailing_comments: FxHashMap<TextRange, AlignmentDecision>,
}

impl AlignmentPlan {
    pub fn is_empty(&self) -> bool {
        self.class_fields.is_empty()
            && self.class_field_defaults.is_empty()
            && self.function_params.is_empty()
            && self.function_param_defaults.is_empty()
            && self.assignments.is_empty()
            && self.dict_values.is_empty()
            && self.call_keyword_args.is_empty()
            && self.import_aliases.is_empty()
            && self.collection_row_items.is_empty()
            && self.repeated_call_args.is_empty()
            && self.with_items.is_empty()
            && self.trailing_comments.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlignmentDecision {
    pub group_id: AlignmentGroupId,
    pub padding_after_separator: u16,
    pub target_column: DisplayColumn,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AlignmentGroupId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayColumn(pub u32);

#[derive(Clone, Debug, Default)]
pub struct CollectionPlan;

impl CollectionPlan {
    #[expect(
        clippy::unused_self,
        reason = "plan shape will grow with collection decisions"
    )]
    pub const fn is_empty(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlankLinePlan;

impl BlankLinePlan {
    #[expect(
        clippy::unused_self,
        reason = "plan shape will grow with blank-line decisions"
    )]
    pub const fn is_empty(&self) -> bool {
        true
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "layout analysis may report hard errors"
)]
pub(crate) fn build_layout_plan(
    root: AnyNodeRef<'_>,
    tokens: &Tokens,
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    options: &TuffCustomOptions,
    line_width: usize,
) -> Result<TuffLayoutPlan, CustomLayoutError> {
    let _ = tokens;

    if options.is_default_disabled() {
        return Ok(TuffLayoutPlan::default());
    }

    let mut plan = TuffLayoutPlan::default();
    let AnyNodeRef::ModModule(module) = root else {
        return Ok(plan);
    };
    let index = LineIndex::from_source_text(source.as_str());

    if matches!(options.alignment.class_fields, AlignmentMode::Enabled) {
        analyze_class_fields(
            &module.body,
            comments,
            source,
            &index,
            options,
            line_width,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.function_params, AlignmentMode::Enabled) {
        analyze_function_params(
            &module.body,
            comments,
            source,
            &index,
            options,
            line_width,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.assignments, AlignmentMode::Enabled) {
        analyze_assignments(
            &module.body,
            AssignmentContext::Module,
            comments,
            source,
            options,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.dict_values, AlignmentMode::Enabled)
        || !matches!(options.alignment.dict_alignment, DictAlignmentMode::None)
    {
        analyze_dict_values(
            &module.body,
            comments,
            source,
            &index,
            options,
            line_width,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.call_keyword_args, AlignmentMode::Enabled) {
        analyze_call_keyword_args(
            &module.body,
            comments,
            source,
            &index,
            options,
            line_width,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.import_aliases, AlignmentMode::Enabled) {
        analyze_import_aliases(&module.body, comments, source, options, &mut plan.alignment);
    }

    if matches!(options.alignment.collection_rows, AlignmentMode::Enabled) {
        analyze_collection_rows(
            &module.body,
            comments,
            source,
            &index,
            line_width,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.repeated_call_args, AlignmentMode::Enabled) {
        analyze_repeated_call_args(
            &module.body,
            comments,
            source,
            &index,
            options,
            line_width,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.with_items, AlignmentMode::Enabled) {
        analyze_with_items(
            &module.body,
            comments,
            source,
            &index,
            options,
            &mut plan.alignment,
        );
    }

    if matches!(options.alignment.trailing_comments, AlignmentMode::Enabled) {
        analyze_trailing_comments(&module.body, comments, source, options, &mut plan.alignment);
    }

    Ok(plan)
}

pub(crate) fn restrict_to_range(plan: &mut TuffLayoutPlan, range: TextRange) {
    let class_field_group_ranges = alignment_group_ranges(&plan.alignment.class_fields);
    restrict_alignment_map_to_group_ranges(
        &mut plan.alignment.class_fields,
        range,
        &class_field_group_ranges,
    );
    restrict_alignment_map_to_group_ranges(
        &mut plan.alignment.class_field_defaults,
        range,
        &class_field_group_ranges,
    );

    let function_param_group_ranges = alignment_group_ranges(&plan.alignment.function_params);
    restrict_alignment_map_to_group_ranges(
        &mut plan.alignment.function_params,
        range,
        &function_param_group_ranges,
    );
    restrict_alignment_map_to_group_ranges(
        &mut plan.alignment.function_param_defaults,
        range,
        &function_param_group_ranges,
    );

    restrict_alignment_map_to_range(&mut plan.alignment.assignments, range);
    restrict_alignment_map_to_range(&mut plan.alignment.dict_values, range);
    restrict_alignment_map_to_range(&mut plan.alignment.call_keyword_args, range);
    restrict_alignment_map_to_range(&mut plan.alignment.import_aliases, range);
    restrict_alignment_map_to_range(&mut plan.alignment.collection_row_items, range);
    restrict_alignment_map_to_range(&mut plan.alignment.repeated_call_args, range);
    restrict_alignment_map_to_range(&mut plan.alignment.with_items, range);
    restrict_alignment_map_to_range(&mut plan.alignment.trailing_comments, range);
}

fn restrict_alignment_map_to_range(
    map: &mut FxHashMap<TextRange, AlignmentDecision>,
    range: TextRange,
) {
    let group_ranges = alignment_group_ranges(map);
    restrict_alignment_map_to_group_ranges(map, range, &group_ranges);
}

fn alignment_group_ranges(
    map: &FxHashMap<TextRange, AlignmentDecision>,
) -> FxHashMap<AlignmentGroupId, Vec<TextRange>> {
    let mut group_ranges: FxHashMap<AlignmentGroupId, Vec<TextRange>> = FxHashMap::default();
    #[expect(
        clippy::iter_over_hash_type,
        reason = "group members are only checked for range containment, so their order is irrelevant"
    )]
    for (target_range, decision) in map {
        group_ranges
            .entry(decision.group_id)
            .or_default()
            .push(*target_range);
    }
    group_ranges
}

fn restrict_alignment_map_to_group_ranges(
    map: &mut FxHashMap<TextRange, AlignmentDecision>,
    range: TextRange,
    group_ranges: &FxHashMap<AlignmentGroupId, Vec<TextRange>>,
) {
    map.retain(|_, decision| {
        group_ranges
            .get(&decision.group_id)
            .is_some_and(|ranges| ranges.iter().all(|member| range.contains_range(*member)))
    });
    map.shrink_to_fit();
}

fn analyze_class_fields(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    for statement in body {
        match statement {
            Stmt::ClassDef(class_def) => {
                if class_field_scope_matches(&options.alignment.class_field_scope, class_def) {
                    analyze_class_body(
                        &class_def.body,
                        comments,
                        source,
                        index,
                        options,
                        line_width,
                        plan,
                    );
                }
                analyze_class_fields(
                    &class_def.body,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
            }
            Stmt::FunctionDef(function_def) => analyze_class_fields(
                &function_def.body,
                comments,
                source,
                index,
                options,
                line_width,
                plan,
            ),
            Stmt::For(for_stmt) => {
                analyze_class_fields(
                    &for_stmt.body,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
                analyze_class_fields(
                    &for_stmt.orelse,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
            }
            Stmt::While(while_stmt) => {
                analyze_class_fields(
                    &while_stmt.body,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
                analyze_class_fields(
                    &while_stmt.orelse,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
            }
            Stmt::If(if_stmt) => {
                analyze_class_fields(
                    &if_stmt.body,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
                for clause in &if_stmt.elif_else_clauses {
                    analyze_class_fields(
                        &clause.body,
                        comments,
                        source,
                        index,
                        options,
                        line_width,
                        plan,
                    );
                }
            }
            Stmt::With(with_stmt) => analyze_class_fields(
                &with_stmt.body,
                comments,
                source,
                index,
                options,
                line_width,
                plan,
            ),
            Stmt::Try(try_stmt) => {
                analyze_class_fields(
                    &try_stmt.body,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
                analyze_class_fields(
                    &try_stmt.orelse,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
                analyze_class_fields(
                    &try_stmt.finalbody,
                    comments,
                    source,
                    index,
                    options,
                    line_width,
                    plan,
                );
                for handler in &try_stmt.handlers {
                    if let Some(handler) = handler.as_except_handler() {
                        analyze_class_fields(
                            &handler.body,
                            comments,
                            source,
                            index,
                            options,
                            line_width,
                            plan,
                        );
                    }
                }
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    analyze_class_fields(
                        &case.body, comments, source, index, options, line_width, plan,
                    );
                }
            }
            _ => {}
        }
    }
}

fn analyze_class_body(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    let mut group = Vec::new();
    let mut previous_range: Option<TextRange> = None;
    let mut in_suppression = false;

    for statement in body {
        let leading_comments = comments.leading(statement);

        if !in_suppression && starts_suppression(leading_comments, source.as_str()) {
            flush_class_field_group(&mut group, options, line_width, plan);
            in_suppression = true;
        }

        if in_suppression {
            if ends_suppression(leading_comments, source.as_str()) {
                in_suppression = false;
                flush_class_field_group(&mut group, options, line_width, plan);
            } else {
                previous_range = Some(statement.range());
                continue;
            }
        }

        if previous_range.is_some_and(|previous| {
            options.alignment.break_on_blank_line
                && has_blank_line_between(previous, statement.range(), source.as_str())
        }) || (options.alignment.break_on_leading_comment && !leading_comments.is_empty())
        {
            flush_class_field_group(&mut group, options, line_width, plan);
        }

        match statement {
            Stmt::AnnAssign(ann_assign) => {
                if let Some(candidate) = class_field_candidate(ann_assign, index, source.as_str()) {
                    group.push(candidate);
                    if options.alignment.break_on_trailing_comment
                        && comments.has_trailing(ann_assign)
                    {
                        flush_class_field_group(&mut group, options, line_width, plan);
                    }
                } else {
                    flush_class_field_group(&mut group, options, line_width, plan);
                }
            }
            _ => flush_class_field_group(&mut group, options, line_width, plan),
        }

        previous_range = Some(statement.range());
    }

    flush_class_field_group(&mut group, options, line_width, plan);
}

fn class_field_candidate(
    ann_assign: &ast::StmtAnnAssign,
    index: &LineIndex,
    source: &str,
) -> Option<ClassFieldCandidate> {
    let target = ann_assign.target.as_name_expr()?;

    if index.line_index(ann_assign.annotation.start())
        != index.line_index(ann_assign.annotation.end())
        || source[TextRange::new(target.end(), ann_assign.annotation.start())].contains('\n')
        || ann_assign
            .value
            .as_deref()
            .is_some_and(may_expand_collection_default)
    {
        return None;
    }

    Some(ClassFieldCandidate {
        range: ann_assign.range(),
        name_width: display_width(target.id.as_str()),
        annotation_width: display_width_source(&source[ann_assign.annotation.range()]),
        line_width: display_width_for_line(ann_assign.range(), source),
        has_default: ann_assign.value.is_some(),
    })
}

fn flush_class_field_group(
    group: &mut Vec<ClassFieldCandidate>,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.name_width)
            .max()
            .unwrap_or_default();
        let default_target_column = group
            .iter()
            .filter(|member| member.has_default)
            .map(|member| member.annotation_width)
            .max()
            .unwrap_or_default();

        if !group.iter().all(|member| {
            let annotation_padding = target_column.saturating_sub(member.name_width);
            let default_padding = if options.alignment.align_defaults && member.has_default {
                default_target_column.saturating_sub(member.annotation_width)
            } else {
                0
            };
            member.line_width + annotation_padding as usize + default_padding as usize <= line_width
        }) {
            group.clear();
            return;
        }

        let group_id = AlignmentGroupId(u32::try_from(plan.class_fields.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.name_width);
            plan.class_fields.insert(
                member.range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );

            if options.alignment.align_defaults && member.has_default {
                let padding = default_target_column.saturating_sub(member.annotation_width);
                plan.class_field_defaults.insert(
                    member.range,
                    AlignmentDecision {
                        group_id,
                        padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                        target_column: DisplayColumn(default_target_column),
                    },
                );
            }
        }
    }

    group.clear();
}

fn class_field_scope_matches(
    scope: &ClassFieldAlignmentScope,
    class_def: &ast::StmtClassDef,
) -> bool {
    match scope {
        ClassFieldAlignmentScope::All => true,
        ClassFieldAlignmentScope::DataclassesAndPydantic => is_schema_class(class_def, false),
        ClassFieldAlignmentScope::DataclassesPydanticAndTypedDict => {
            is_schema_class(class_def, true)
        }
    }
}

fn is_schema_class(class_def: &ast::StmtClassDef, include_typeddict: bool) -> bool {
    class_def
        .decorator_list
        .iter()
        .any(|decorator| is_dataclass_expr(&decorator.expression))
        || class_def.arguments.as_deref().is_some_and(|arguments| {
            arguments.args.iter().any(|expr| {
                is_pydantic_base_expr(expr) || (include_typeddict && is_typeddict_base_expr(expr))
            })
        })
}

fn analyze_function_params(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    FunctionParamAnalysis {
        comments,
        source,
        index,
        options,
        line_width,
        plan,
    }
    .analyze_scope(body, FunctionContext::Module);
}

struct FunctionParamAnalysis<'a, 'src> {
    comments: &'a Comments<'src>,
    source: SourceCode<'src>,
    index: &'a LineIndex,
    options: &'a TuffCustomOptions,
    line_width: usize,
    plan: &'a mut AlignmentPlan,
}

impl FunctionParamAnalysis<'_, '_> {
    fn analyze_scope(&mut self, body: &[Stmt], context: FunctionContext) {
        for statement in body {
            match statement {
                Stmt::FunctionDef(function_def) => {
                    if function_param_scope_matches(
                        &self.options.alignment.function_param_scope,
                        context,
                        function_def,
                    ) {
                        analyze_parameters(
                            &function_def.parameters,
                            self.comments,
                            self.source,
                            self.index,
                            self.options,
                            self.line_width,
                            self.plan,
                        );
                    }
                    self.analyze_scope(&function_def.body, FunctionContext::NestedFunction);
                }
                Stmt::ClassDef(class_def) => {
                    self.analyze_scope(&class_def.body, FunctionContext::ClassBody);
                }
                _ => walk_child_suites(statement, context, &mut |body, context| {
                    self.analyze_scope(body, context);
                }),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FunctionContext {
    Module,
    ClassBody,
    NestedFunction,
}

fn function_param_scope_matches(
    scope: &FunctionParamAlignmentScope,
    context: FunctionContext,
    function_def: &ast::StmtFunctionDef,
) -> bool {
    match scope {
        FunctionParamAlignmentScope::All => true,
        FunctionParamAlignmentScope::Functions => context != FunctionContext::ClassBody,
        FunctionParamAlignmentScope::Methods => context == FunctionContext::ClassBody,
        FunctionParamAlignmentScope::ClassInit => {
            context == FunctionContext::ClassBody && function_def.name.as_str() == "__init__"
        }
    }
}

fn analyze_parameters(
    parameters: &ast::Parameters,
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    analyze_parameter_with_defaults(
        parameters.posonlyargs.iter(),
        comments,
        source.as_str(),
        index,
        options,
        line_width,
        plan,
    );
    analyze_parameter_with_defaults(
        parameters.args.iter(),
        comments,
        source.as_str(),
        index,
        options,
        line_width,
        plan,
    );

    let vararg = parameters.vararg.as_deref().and_then(|parameter| {
        parameter_candidate(
            parameter,
            None,
            parameter.range(),
            comments,
            source.as_str(),
            index,
        )
    });
    let mut vararg_group = vararg.into_iter().collect();
    flush_parameter_group(&mut vararg_group, options, line_width, plan);

    analyze_parameter_with_defaults(
        parameters.kwonlyargs.iter(),
        comments,
        source.as_str(),
        index,
        options,
        line_width,
        plan,
    );

    let kwarg = parameters.kwarg.as_deref().and_then(|parameter| {
        parameter_candidate(
            parameter,
            None,
            parameter.range(),
            comments,
            source.as_str(),
            index,
        )
    });
    let mut kwarg_group = kwarg.into_iter().collect();
    flush_parameter_group(&mut kwarg_group, options, line_width, plan);
}

fn analyze_parameter_with_defaults<'a>(
    parameters: impl Iterator<Item = &'a ast::ParameterWithDefault>,
    comments: &Comments<'_>,
    source: &str,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    let mut group = Vec::new();
    let mut previous_range: Option<TextRange> = None;

    for parameter_with_default in parameters {
        let parameter = &parameter_with_default.parameter;
        let leading_comments = comments.leading(parameter);

        if previous_range.is_some_and(|previous| {
            options.alignment.break_on_blank_line
                && has_blank_line_between(previous, parameter_with_default.range(), source)
        }) || (options.alignment.break_on_leading_comment && !leading_comments.is_empty())
        {
            flush_parameter_group(&mut group, options, line_width, plan);
        }

        if let Some(candidate) = parameter_candidate(
            parameter,
            parameter_with_default.default.as_deref(),
            parameter_with_default.range(),
            comments,
            source,
            index,
        ) {
            group.push(candidate);
            if options.alignment.break_on_trailing_comment
                && comments.has_trailing(parameter_with_default)
            {
                flush_parameter_group(&mut group, options, line_width, plan);
            }
        } else {
            flush_parameter_group(&mut group, options, line_width, plan);
        }

        previous_range = Some(parameter_with_default.range());
    }

    flush_parameter_group(&mut group, options, line_width, plan);
}

fn parameter_candidate(
    parameter: &ast::Parameter,
    default: Option<&ast::Expr>,
    containing_range: TextRange,
    comments: &Comments<'_>,
    source: &str,
    index: &LineIndex,
) -> Option<ParameterCandidate> {
    let annotation = parameter.annotation.as_deref()?;

    if comments.has_leading(annotation)
        || index.line_index(containing_range.start()) != index.line_index(containing_range.end())
        || index.line_index(annotation.start()) != index.line_index(annotation.end())
        || source[TextRange::new(parameter.name.end(), annotation.start())].contains('\n')
        || default.is_some_and(|default| {
            index.line_index(default.start()) != index.line_index(default.end())
                || may_expand_collection_default(default)
        })
    {
        return None;
    }

    Some(ParameterCandidate {
        parameter_range: parameter.range(),
        containing_range,
        name_width: display_width(parameter.name.as_str()),
        annotation_width: display_width_source(&source[annotation.range()]),
        line_width: display_width_for_line(containing_range, source),
        has_default: default.is_some(),
    })
}

fn flush_parameter_group(
    group: &mut Vec<ParameterCandidate>,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.name_width)
            .max()
            .unwrap_or_default();
        let default_target_column = group
            .iter()
            .filter(|member| member.has_default)
            .map(|member| member.annotation_width)
            .max()
            .unwrap_or_default();

        if !group.iter().all(|member| {
            let annotation_padding = target_column.saturating_sub(member.name_width);
            let default_padding = if options.alignment.align_defaults && member.has_default {
                default_target_column.saturating_sub(member.annotation_width)
            } else {
                0
            };
            member.line_width + annotation_padding as usize + default_padding as usize <= line_width
        }) {
            group.clear();
            return;
        }

        let group_id =
            AlignmentGroupId(u32::try_from(plan.function_params.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.name_width);
            plan.function_params.insert(
                member.parameter_range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }

        if options.alignment.align_defaults {
            for member in group.iter().filter(|member| member.has_default) {
                let padding = default_target_column.saturating_sub(member.annotation_width);
                plan.function_param_defaults.insert(
                    member.containing_range,
                    AlignmentDecision {
                        group_id,
                        padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                        target_column: DisplayColumn(default_target_column),
                    },
                );
            }
        }
    }

    group.clear();
}

fn analyze_assignments(
    body: &[Stmt],
    context: AssignmentContext,
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    options: &TuffCustomOptions,
    plan: &mut AlignmentPlan,
) {
    if assignment_scope_matches(&options.alignment.assignment_scope, context) {
        analyze_assignment_suite(body, comments, source, options, plan);
    }

    for statement in body {
        match statement {
            Stmt::ClassDef(class_def) => {
                let context = if is_enum_class(class_def) {
                    AssignmentContext::EnumClassBody
                } else {
                    AssignmentContext::ClassBody
                };
                analyze_assignments(&class_def.body, context, comments, source, options, plan);
            }
            Stmt::FunctionDef(function_def) => {
                analyze_assignments(
                    &function_def.body,
                    AssignmentContext::FunctionBody,
                    comments,
                    source,
                    options,
                    plan,
                );
            }
            _ => walk_child_suites(statement, context, &mut |body, context| {
                analyze_assignments(body, context, comments, source, options, plan);
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AssignmentContext {
    Module,
    ClassBody,
    EnumClassBody,
    FunctionBody,
}

fn assignment_scope_matches(scope: &AssignmentAlignmentScope, context: AssignmentContext) -> bool {
    match scope {
        AssignmentAlignmentScope::All => true,
        AssignmentAlignmentScope::Module => context == AssignmentContext::Module,
        AssignmentAlignmentScope::Class => {
            matches!(
                context,
                AssignmentContext::ClassBody | AssignmentContext::EnumClassBody
            )
        }
        AssignmentAlignmentScope::ModuleAndClass => context != AssignmentContext::FunctionBody,
        AssignmentAlignmentScope::EnumClass => context == AssignmentContext::EnumClassBody,
    }
}

fn analyze_assignment_suite(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    options: &TuffCustomOptions,
    plan: &mut AlignmentPlan,
) {
    let mut group = Vec::new();
    let mut previous_range: Option<TextRange> = None;
    let mut in_suppression = false;

    for statement in body {
        let leading_comments = comments.leading(statement);

        if !in_suppression && starts_suppression(leading_comments, source.as_str()) {
            flush_assignment_group(&mut group, options, plan);
            in_suppression = true;
        }

        if in_suppression {
            if ends_suppression(leading_comments, source.as_str()) {
                in_suppression = false;
                flush_assignment_group(&mut group, options, plan);
            } else {
                previous_range = Some(statement.range());
                continue;
            }
        }

        if previous_range.is_some_and(|previous| {
            options.alignment.break_on_blank_line
                && has_blank_line_between(previous, statement.range(), source.as_str())
        }) || (options.alignment.break_on_leading_comment && !leading_comments.is_empty())
        {
            flush_assignment_group(&mut group, options, plan);
        }

        match statement {
            Stmt::Assign(assign) => {
                if let Some(candidate) = assignment_candidate(assign, comments, source.as_str()) {
                    group.push(candidate);
                    if options.alignment.break_on_trailing_comment && comments.has_trailing(assign)
                    {
                        flush_assignment_group(&mut group, options, plan);
                    }
                } else {
                    flush_assignment_group(&mut group, options, plan);
                }
            }
            _ => flush_assignment_group(&mut group, options, plan),
        }

        previous_range = Some(statement.range());
    }

    flush_assignment_group(&mut group, options, plan);
}

fn assignment_candidate(
    assign: &ast::StmtAssign,
    comments: &Comments<'_>,
    source: &str,
) -> Option<AssignmentCandidate> {
    let [target] = assign.targets.as_slice() else {
        return None;
    };
    let target = target.as_name_expr()?;

    if comments.has_leading(target)
        || comments.has_trailing(target)
        || comments.has_leading(assign.value.as_ref())
        || !source[TextRange::new(target.end(), assign.value.start())]
            .lines()
            .next()
            .is_some_and(|line| line.contains('='))
    {
        return None;
    }

    Some(AssignmentCandidate {
        range: assign.range(),
        target_width: display_width(target.id.as_str()),
    })
}

fn flush_assignment_group(
    group: &mut Vec<AssignmentCandidate>,
    options: &TuffCustomOptions,
    plan: &mut AlignmentPlan,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.target_width)
            .max()
            .unwrap_or_default();
        let group_id = AlignmentGroupId(u32::try_from(plan.assignments.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.target_width);
            plan.assignments.insert(
                member.range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }
    }

    group.clear();
}

fn analyze_dict_values(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    for statement in body {
        walk_statement_expressions(statement, &mut |expr| {
            if let Expr::Dict(dict) = expr {
                analyze_dict_items(dict, comments, source, index, options, line_width, plan);
            }
        });
    }
}

fn analyze_dict_items(
    dict: &ast::ExprDict,
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    if !source.as_str()[dict.range()].contains('\n') {
        return;
    }

    let mut group = Vec::new();

    for item in &dict.items {
        if let Some(candidate) = dict_item_candidate(item, comments, source.as_str(), index) {
            group.push(candidate);
        } else {
            flush_separator_group(&mut group, options, line_width, &mut plan.dict_values);
        }
    }

    flush_separator_group(&mut group, options, line_width, &mut plan.dict_values);
}

fn dict_item_candidate(
    item: &ast::DictItem,
    comments: &Comments<'_>,
    source: &str,
    index: &LineIndex,
) -> Option<SeparatorCandidate> {
    let key = item.key.as_ref()?;

    if comments.has_leading(key)
        || comments.has_trailing(key)
        || comments.has_leading(&item.value)
        || !is_simple_separator_value(&item.value)
        || index.line_index(key.start()) != index.line_index(key.end())
        || index.line_index(item.value.start()) != index.line_index(item.value.end())
        || !source[TextRange::new(key.end(), item.value.start())].contains(':')
    {
        return None;
    }

    Some(SeparatorCandidate {
        range: item.range(),
        left_width: display_width_source(&source[key.range()]),
    })
}

fn analyze_call_keyword_args(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    for statement in body {
        walk_statement_expressions(statement, &mut |expr| {
            if let Expr::Call(call) = expr {
                analyze_call_keywords(call, comments, source, index, options, line_width, plan);
            }
        });
    }
}

fn analyze_call_keywords(
    call: &ast::ExprCall,
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    if !source.as_str()[call.arguments.range()].contains('\n') {
        return;
    }

    let mut group = Vec::new();
    let mut previous_keyword_line = None;

    for arg_or_keyword in call.arguments.iter_source_order() {
        match arg_or_keyword {
            ast::ArgOrKeyword::Keyword(keyword) => {
                if let Some(candidate) =
                    keyword_candidate(keyword, comments, source.as_str(), index)
                {
                    let keyword_line = line_start_offset(candidate.range.start(), source.as_str());
                    if previous_keyword_line == Some(keyword_line) {
                        flush_separator_group(
                            &mut group,
                            options,
                            line_width,
                            &mut plan.call_keyword_args,
                        );
                    } else {
                        group.push(candidate);
                    }
                    previous_keyword_line = Some(keyword_line);
                } else {
                    flush_separator_group(
                        &mut group,
                        options,
                        line_width,
                        &mut plan.call_keyword_args,
                    );
                }
            }
            ast::ArgOrKeyword::Arg(_) => {
                flush_separator_group(&mut group, options, line_width, &mut plan.call_keyword_args);
                previous_keyword_line = None;
            }
        }
    }

    flush_separator_group(&mut group, options, line_width, &mut plan.call_keyword_args);
}

fn keyword_candidate(
    keyword: &ast::Keyword,
    comments: &Comments<'_>,
    source: &str,
    index: &LineIndex,
) -> Option<SeparatorCandidate> {
    let arg = keyword.arg.as_ref()?;

    if comments.has_leading(arg)
        || comments.has_trailing(arg)
        || comments.has_leading(&keyword.value)
        || !is_simple_separator_value(&keyword.value)
        || index.line_index(arg.start()) != index.line_index(arg.end())
        || index.line_index(keyword.value.start()) != index.line_index(keyword.value.end())
        || !source[TextRange::new(arg.end(), keyword.value.start())].contains('=')
    {
        return None;
    }

    Some(SeparatorCandidate {
        range: keyword.range(),
        left_width: display_width(arg.id.as_str()),
    })
}

fn flush_separator_group(
    group: &mut Vec<SeparatorCandidate>,
    options: &TuffCustomOptions,
    _line_width: usize,
    map: &mut FxHashMap<TextRange, AlignmentDecision>,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.left_width)
            .max()
            .unwrap_or_default();

        let group_id = AlignmentGroupId(u32::try_from(map.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.left_width);
            map.insert(
                member.range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }
    }

    group.clear();
}

fn analyze_import_aliases(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    options: &TuffCustomOptions,
    plan: &mut AlignmentPlan,
) {
    let mut group = Vec::new();
    let mut previous_range: Option<TextRange> = None;

    for statement in body {
        let leading_comments = comments.leading(statement);

        if previous_range.is_some_and(|previous| {
            options.alignment.break_on_blank_line
                && has_blank_line_between(previous, statement.range(), source.as_str())
        }) || (options.alignment.break_on_leading_comment && !leading_comments.is_empty())
        {
            flush_alias_group(&mut group, options, &mut plan.import_aliases);
        }

        match statement {
            Stmt::Import(import) => {
                if let [alias] = import.names.as_slice()
                    && let Some(candidate) = alias_candidate(alias, comments, source.as_str())
                {
                    group.push(candidate);
                } else {
                    flush_alias_group(&mut group, options, &mut plan.import_aliases);
                }
            }
            Stmt::ImportFrom(import_from) => {
                if let [alias] = import_from.names.as_slice()
                    && let Some(candidate) = alias_candidate(alias, comments, source.as_str())
                {
                    group.push(candidate);
                } else {
                    flush_alias_group(&mut group, options, &mut plan.import_aliases);
                }
            }
            _ => flush_alias_group(&mut group, options, &mut plan.import_aliases),
        }

        previous_range = Some(statement.range());
    }

    flush_alias_group(&mut group, options, &mut plan.import_aliases);
}

fn analyze_collection_rows(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    for statement in body {
        walk_statement_expressions(statement, &mut |expr| {
            let Some(rows) = collection_rows(expr) else {
                return;
            };
            analyze_collection_row_group(rows, comments, source, index, line_width, plan);
        });
    }
}

fn collection_rows(expr: &Expr) -> Option<Vec<&[Expr]>> {
    let elements = match expr {
        Expr::List(list) => list.elts.as_slice(),
        Expr::Tuple(tuple) => tuple.elts.as_slice(),
        _ => return None,
    };

    let rows = elements
        .iter()
        .map(|element| match element {
            Expr::List(list) => Some(list.elts.as_slice()),
            Expr::Tuple(tuple) => Some(tuple.elts.as_slice()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    let arity = rows.first()?.len();
    if arity < 2 || rows.len() < 2 || rows.iter().any(|row| row.len() != arity) {
        return None;
    }

    Some(rows)
}

fn analyze_collection_row_group(
    rows: Vec<&[Expr]>,
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    let columns = rows[0].len();
    let mut target_columns = vec![0u32; columns.saturating_sub(1)];
    let mut candidates = Vec::new();

    for row in rows {
        let mut row_candidates = Vec::new();
        for (column, element) in row.iter().enumerate() {
            if comments.has_leading(element)
                || comments.has_trailing(element)
                || !is_simple_separator_value(element)
                || index.line_index(element.start()) != index.line_index(element.end())
            {
                return;
            }
            let width = display_width_source(&source.as_str()[element.range()]);
            if column < columns.saturating_sub(1) {
                target_columns[column] = target_columns[column].max(width);
            }
            row_candidates.push((element.range(), column, width));
        }
        candidates.push(row_candidates);
    }

    let group_id =
        AlignmentGroupId(u32::try_from(plan.collection_row_items.len()).unwrap_or(u32::MAX));

    for row in candidates {
        for (range, column, _width) in &row {
            if *column == 0 {
                continue;
            }
            let target_column = target_columns[*column - 1];
            let padding = target_column.saturating_sub(
                row.iter()
                    .find(|(_, candidate_column, _)| *candidate_column == *column - 1)
                    .map_or(0, |(_, _, previous_width)| *previous_width),
            );
            if display_width_for_line(*range, source.as_str()) + padding as usize > line_width {
                return;
            }
            plan.collection_row_items.insert(
                *range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }
    }
}

fn analyze_repeated_call_args(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    let mut group = Vec::new();
    let mut previous_range = None;
    let mut previous_callee: Option<&str> = None;

    for statement in body {
        let leading_comments = comments.leading(statement);

        if previous_range.is_some_and(|previous| {
            options.alignment.break_on_blank_line
                && has_blank_line_between(previous, statement.range(), source.as_str())
        }) || (options.alignment.break_on_leading_comment && !leading_comments.is_empty())
        {
            flush_repeated_call_group(&mut group, options, line_width, plan);
            previous_callee = None;
        }

        let candidate = repeated_call_candidate(statement, comments, source.as_str(), index);
        if let Some(candidate) = candidate {
            if previous_callee != Some(candidate.callee) {
                flush_repeated_call_group(&mut group, options, line_width, plan);
            }
            previous_callee = Some(candidate.callee);
            group.push(candidate);
        } else {
            flush_repeated_call_group(&mut group, options, line_width, plan);
            previous_callee = None;
        }

        previous_range = Some(statement.range());
    }

    flush_repeated_call_group(&mut group, options, line_width, plan);
}

fn repeated_call_candidate<'a>(
    statement: &'a Stmt,
    comments: &Comments<'_>,
    source: &'a str,
    index: &LineIndex,
) -> Option<RepeatedCallCandidate<'a>> {
    let Stmt::Expr(expr_stmt) = statement else {
        return None;
    };
    let Expr::Call(call) = expr_stmt.value.as_ref() else {
        return None;
    };
    if !call.arguments.keywords.is_empty() || call.arguments.args.len() < 2 {
        return None;
    }

    let mut args = Vec::new();
    for arg in &call.arguments.args {
        if comments.has_leading(arg)
            || comments.has_trailing(arg)
            || !is_simple_separator_value(arg)
            || index.line_index(arg.start()) != index.line_index(arg.end())
        {
            return None;
        }
        args.push(ArgumentAlignmentCandidate {
            range: arg.range(),
            width: display_width_source(&source[arg.range()]),
        });
    }

    Some(RepeatedCallCandidate {
        callee: &source[call.func.range()],
        args,
    })
}

fn flush_repeated_call_group(
    group: &mut Vec<RepeatedCallCandidate<'_>>,
    options: &TuffCustomOptions,
    line_width: usize,
    plan: &mut AlignmentPlan,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let columns = group[0].args.len().saturating_sub(1);
        if columns > 0
            && group
                .iter()
                .all(|call| call.args.len().saturating_sub(1) == columns)
        {
            let mut target_columns = vec![0u32; columns];
            for call in group.iter() {
                for (index, arg) in call.args.iter().enumerate().take(columns) {
                    target_columns[index] = target_columns[index].max(arg.width);
                }
            }

            let group_id =
                AlignmentGroupId(u32::try_from(plan.repeated_call_args.len()).unwrap_or(u32::MAX));
            for call in group.iter() {
                for (index, arg) in call.args.iter().enumerate().skip(1) {
                    let previous = call.args[index - 1];
                    let padding = target_columns[index - 1].saturating_sub(previous.width);
                    if padding as usize > line_width {
                        group.clear();
                        return;
                    }
                    plan.repeated_call_args.insert(
                        arg.range,
                        AlignmentDecision {
                            group_id,
                            padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                            target_column: DisplayColumn(target_columns[index - 1]),
                        },
                    );
                }
            }
        }
    }

    group.clear();
}

fn analyze_with_items(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    index: &LineIndex,
    options: &TuffCustomOptions,
    plan: &mut AlignmentPlan,
) {
    for statement in body {
        if let Stmt::With(with_stmt) = statement {
            let mut group = Vec::new();
            for item in &with_stmt.items {
                if let Some(candidate) = with_item_candidate(item, comments, source.as_str(), index)
                {
                    group.push(candidate);
                }
            }
            flush_with_item_group(&mut group, options, &mut plan.with_items);
            analyze_with_items(&with_stmt.body, comments, source, index, options, plan);
        } else {
            walk_child_suites(statement, (), &mut |body, ()| {
                analyze_with_items(body, comments, source, index, options, plan);
            });
        }
    }
}

fn with_item_candidate(
    item: &ast::WithItem,
    comments: &Comments<'_>,
    source: &str,
    index: &LineIndex,
) -> Option<WithItemCandidate> {
    item.optional_vars.as_ref()?;
    let context_expr = &item.context_expr;

    if comments.has_leading(context_expr)
        || comments.has_trailing(context_expr)
        || index.line_index(context_expr.start()) != index.line_index(context_expr.end())
        || !source[TextRange::new(context_expr.end(), item.end())].contains("as")
    {
        return None;
    }

    Some(WithItemCandidate {
        range: item.range(),
        context_width: display_width_source(&source[context_expr.range()]),
    })
}

fn flush_with_item_group(
    group: &mut Vec<WithItemCandidate>,
    options: &TuffCustomOptions,
    map: &mut FxHashMap<TextRange, AlignmentDecision>,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.context_width)
            .max()
            .unwrap_or_default();
        let group_id = AlignmentGroupId(u32::try_from(map.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.context_width);
            map.insert(
                member.range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }
    }

    group.clear();
}

fn analyze_trailing_comments(
    body: &[Stmt],
    comments: &Comments<'_>,
    source: SourceCode<'_>,
    options: &TuffCustomOptions,
    plan: &mut AlignmentPlan,
) {
    let mut group = Vec::new();
    let mut previous_range = None;

    for statement in body {
        if previous_range.is_some_and(|previous| {
            options.alignment.break_on_blank_line
                && has_blank_line_between(previous, statement.range(), source.as_str())
        }) || (options.alignment.break_on_leading_comment
            && !comments.leading(statement).is_empty())
        {
            flush_trailing_comment_group(&mut group, options, &mut plan.trailing_comments);
        }

        let trailing_comments = trailing_comment_candidates_for_statement(statement, comments);
        let mut inline_comments = trailing_comments
            .iter()
            .copied()
            .filter(|comment| comment.line_position().is_end_of_line());

        if let (Some(comment), None) = (inline_comments.next(), inline_comments.next()) {
            let line_start = line_start_offset(statement.start(), source.as_str());
            let code = source.as_str()[line_start..comment.start().to_usize()].trim_end();
            let code_width = display_width_source(code);
            group.push(TrailingCommentCandidate {
                range: comment.range(),
                code_width,
            });
        } else {
            flush_trailing_comment_group(&mut group, options, &mut plan.trailing_comments);
        }

        walk_child_suites(statement, (), &mut |body, ()| {
            analyze_trailing_comments(body, comments, source, options, plan);
        });
        previous_range = Some(statement.range());
    }

    flush_trailing_comment_group(&mut group, options, &mut plan.trailing_comments);
}

fn trailing_comment_candidates_for_statement<'a>(
    statement: &'a Stmt,
    comments: &'a Comments<'_>,
) -> Vec<&'a crate::comments::SourceComment> {
    let mut trailing = comments.trailing(statement).iter().collect::<Vec<_>>();

    match statement {
        Stmt::Assign(assign) => {
            trailing.extend(comments.trailing(assign.value.as_ref()));
        }
        Stmt::AnnAssign(assign) => {
            if let Some(value) = assign.value.as_deref() {
                trailing.extend(comments.trailing(value));
            }
        }
        _ => {}
    }

    trailing
}

fn flush_trailing_comment_group(
    group: &mut Vec<TrailingCommentCandidate>,
    options: &TuffCustomOptions,
    map: &mut FxHashMap<TextRange, AlignmentDecision>,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.code_width)
            .max()
            .unwrap_or_default();
        let group_id = AlignmentGroupId(u32::try_from(map.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.code_width);
            map.insert(
                member.range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }
    }

    group.clear();
}

fn alias_candidate(
    alias: &ast::Alias,
    comments: &Comments<'_>,
    source: &str,
) -> Option<AliasCandidate> {
    let asname = alias.asname.as_ref()?;

    if comments.has_trailing(&alias.name)
        || comments.has_leading(asname)
        || !source[TextRange::new(alias.name.end(), asname.start())].contains("as")
    {
        return None;
    }

    Some(AliasCandidate {
        range: alias.range(),
        name_width: display_width_source(&source[alias.name.range()]),
    })
}

fn flush_alias_group(
    group: &mut Vec<AliasCandidate>,
    options: &TuffCustomOptions,
    map: &mut FxHashMap<TextRange, AlignmentDecision>,
) {
    if group.len() >= usize::from(options.alignment.min_group_size) {
        let target_column = group
            .iter()
            .map(|member| member.name_width)
            .max()
            .unwrap_or_default();
        let group_id = AlignmentGroupId(u32::try_from(map.len()).unwrap_or(u32::MAX));

        for member in group.iter() {
            let padding = target_column.saturating_sub(member.name_width);
            map.insert(
                member.range,
                AlignmentDecision {
                    group_id,
                    padding_after_separator: u16::try_from(padding).unwrap_or(u16::MAX),
                    target_column: DisplayColumn(target_column),
                },
            );
        }
    }

    group.clear();
}

fn walk_child_suites<C>(statement: &Stmt, context: C, visit: &mut impl FnMut(&[Stmt], C))
where
    C: Copy,
{
    match statement {
        Stmt::For(for_stmt) => {
            visit(&for_stmt.body, context);
            visit(&for_stmt.orelse, context);
        }
        Stmt::While(while_stmt) => {
            visit(&while_stmt.body, context);
            visit(&while_stmt.orelse, context);
        }
        Stmt::If(if_stmt) => {
            visit(&if_stmt.body, context);
            for clause in &if_stmt.elif_else_clauses {
                visit(&clause.body, context);
            }
        }
        Stmt::With(with_stmt) => visit(&with_stmt.body, context),
        Stmt::Try(try_stmt) => {
            visit(&try_stmt.body, context);
            visit(&try_stmt.orelse, context);
            visit(&try_stmt.finalbody, context);
            for handler in &try_stmt.handlers {
                if let Some(handler) = handler.as_except_handler() {
                    visit(&handler.body, context);
                }
            }
        }
        Stmt::Match(match_stmt) => {
            for case in &match_stmt.cases {
                visit(&case.body, context);
            }
        }
        _ => {}
    }
}

fn walk_statement_expressions(statement: &Stmt, visit: &mut impl FnMut(&Expr)) {
    match statement {
        Stmt::FunctionDef(function_def) => {
            for decorator in &function_def.decorator_list {
                walk_expression(&decorator.expression, visit);
            }
            if let Some(returns) = function_def.returns.as_deref() {
                walk_expression(returns, visit);
            }
            walk_statement_expressions_in_body(&function_def.body, visit);
        }
        Stmt::ClassDef(class_def) => {
            for decorator in &class_def.decorator_list {
                walk_expression(&decorator.expression, visit);
            }
            if let Some(arguments) = class_def.arguments.as_deref() {
                for arg in &arguments.args {
                    walk_expression(arg, visit);
                }
                for keyword in &arguments.keywords {
                    walk_expression(&keyword.value, visit);
                }
            }
            walk_statement_expressions_in_body(&class_def.body, visit);
        }
        Stmt::Return(return_stmt) => {
            if let Some(value) = return_stmt.value.as_deref() {
                walk_expression(value, visit);
            }
        }
        Stmt::Raise(raise_stmt) => {
            if let Some(exc) = raise_stmt.exc.as_deref() {
                walk_expression(exc, visit);
            }
            if let Some(cause) = raise_stmt.cause.as_deref() {
                walk_expression(cause, visit);
            }
        }
        Stmt::Delete(delete_stmt) => {
            for target in &delete_stmt.targets {
                walk_expression(target, visit);
            }
        }
        Stmt::Assign(assign) => {
            for target in &assign.targets {
                walk_expression(target, visit);
            }
            walk_expression(&assign.value, visit);
        }
        Stmt::AugAssign(assign) => {
            walk_expression(&assign.target, visit);
            walk_expression(&assign.value, visit);
        }
        Stmt::AnnAssign(assign) => {
            walk_expression(&assign.target, visit);
            walk_expression(&assign.annotation, visit);
            if let Some(value) = assign.value.as_deref() {
                walk_expression(value, visit);
            }
        }
        Stmt::For(for_stmt) => {
            walk_expression(&for_stmt.target, visit);
            walk_expression(&for_stmt.iter, visit);
            walk_statement_expressions_in_body(&for_stmt.body, visit);
            walk_statement_expressions_in_body(&for_stmt.orelse, visit);
        }
        Stmt::While(while_stmt) => {
            walk_expression(&while_stmt.test, visit);
            walk_statement_expressions_in_body(&while_stmt.body, visit);
            walk_statement_expressions_in_body(&while_stmt.orelse, visit);
        }
        Stmt::If(if_stmt) => {
            walk_expression(&if_stmt.test, visit);
            walk_statement_expressions_in_body(&if_stmt.body, visit);
            for clause in &if_stmt.elif_else_clauses {
                if let Some(test) = clause.test.as_ref() {
                    walk_expression(test, visit);
                }
                walk_statement_expressions_in_body(&clause.body, visit);
            }
        }
        Stmt::With(with_stmt) => {
            for item in &with_stmt.items {
                walk_expression(&item.context_expr, visit);
                if let Some(optional_vars) = item.optional_vars.as_deref() {
                    walk_expression(optional_vars, visit);
                }
            }
            walk_statement_expressions_in_body(&with_stmt.body, visit);
        }
        Stmt::Match(match_stmt) => {
            walk_expression(&match_stmt.subject, visit);
            for case in &match_stmt.cases {
                if let Some(guard) = case.guard.as_deref() {
                    walk_expression(guard, visit);
                }
                walk_statement_expressions_in_body(&case.body, visit);
            }
        }
        Stmt::Try(try_stmt) => {
            walk_statement_expressions_in_body(&try_stmt.body, visit);
            walk_statement_expressions_in_body(&try_stmt.orelse, visit);
            walk_statement_expressions_in_body(&try_stmt.finalbody, visit);
            for handler in &try_stmt.handlers {
                if let Some(handler) = handler.as_except_handler() {
                    if let Some(type_) = handler.type_.as_deref() {
                        walk_expression(type_, visit);
                    }
                    walk_statement_expressions_in_body(&handler.body, visit);
                }
            }
        }
        Stmt::Assert(assert_stmt) => {
            walk_expression(&assert_stmt.test, visit);
            if let Some(msg) = assert_stmt.msg.as_deref() {
                walk_expression(msg, visit);
            }
        }
        Stmt::Expr(expr_stmt) => walk_expression(&expr_stmt.value, visit),
        Stmt::TypeAlias(type_alias) => walk_expression(&type_alias.value, visit),
        Stmt::Import(_)
        | Stmt::ImportFrom(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::IpyEscapeCommand(_) => {}
    }
}

fn walk_statement_expressions_in_body(body: &[Stmt], visit: &mut impl FnMut(&Expr)) {
    for statement in body {
        walk_statement_expressions(statement, visit);
    }
}

fn walk_expression(expr: &Expr, visit: &mut impl FnMut(&Expr)) {
    visit(expr);

    match expr {
        Expr::BoolOp(expr) => {
            for value in &expr.values {
                walk_expression(value, visit);
            }
        }
        Expr::Named(named) => {
            walk_expression(&named.target, visit);
            walk_expression(&named.value, visit);
        }
        Expr::BinOp(expr) => {
            walk_expression(&expr.left, visit);
            walk_expression(&expr.right, visit);
        }
        Expr::UnaryOp(expr) => walk_expression(&expr.operand, visit),
        Expr::Lambda(expr) => walk_expression(&expr.body, visit),
        Expr::If(expr) => {
            walk_expression(&expr.test, visit);
            walk_expression(&expr.body, visit);
            walk_expression(&expr.orelse, visit);
        }
        Expr::Dict(expr) => {
            for item in &expr.items {
                if let Some(key) = item.key.as_ref() {
                    walk_expression(key, visit);
                }
                walk_expression(&item.value, visit);
            }
        }
        Expr::Set(expr) => {
            for elt in &expr.elts {
                walk_expression(elt, visit);
            }
        }
        Expr::ListComp(expr) => {
            walk_expression(&expr.elt, visit);
            for generator in &expr.generators {
                walk_comprehension(generator, visit);
            }
        }
        Expr::SetComp(expr) => {
            walk_expression(&expr.elt, visit);
            for generator in &expr.generators {
                walk_comprehension(generator, visit);
            }
        }
        Expr::DictComp(expr) => {
            if let Some(key) = expr.key.as_deref() {
                walk_expression(key, visit);
            }
            walk_expression(&expr.value, visit);
            for generator in &expr.generators {
                walk_comprehension(generator, visit);
            }
        }
        Expr::Generator(expr) => {
            walk_expression(&expr.elt, visit);
            for generator in &expr.generators {
                walk_comprehension(generator, visit);
            }
        }
        Expr::Await(expr) => walk_expression(&expr.value, visit),
        Expr::Yield(expr) => {
            if let Some(value) = expr.value.as_deref() {
                walk_expression(value, visit);
            }
        }
        Expr::YieldFrom(expr) => walk_expression(&expr.value, visit),
        Expr::Compare(expr) => {
            walk_expression(&expr.left, visit);
            for comparator in &expr.comparators {
                walk_expression(comparator, visit);
            }
        }
        Expr::Call(expr) => {
            walk_expression(&expr.func, visit);
            for arg in &expr.arguments.args {
                walk_expression(arg, visit);
            }
            for keyword in &expr.arguments.keywords {
                walk_expression(&keyword.value, visit);
            }
        }
        Expr::FString(_)
        | Expr::TString(_)
        | Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => {}
        Expr::Attribute(expr) => walk_expression(&expr.value, visit),
        Expr::Subscript(expr) => {
            walk_expression(&expr.value, visit);
            walk_expression(&expr.slice, visit);
        }
        Expr::Starred(expr) => walk_expression(&expr.value, visit),
        Expr::Name(_) => {}
        Expr::List(expr) => {
            for elt in &expr.elts {
                walk_expression(elt, visit);
            }
        }
        Expr::Tuple(expr) => {
            for elt in &expr.elts {
                walk_expression(elt, visit);
            }
        }
        Expr::Slice(expr) => {
            if let Some(lower) = expr.lower.as_deref() {
                walk_expression(lower, visit);
            }
            if let Some(upper) = expr.upper.as_deref() {
                walk_expression(upper, visit);
            }
            if let Some(step) = expr.step.as_deref() {
                walk_expression(step, visit);
            }
        }
        Expr::IpyEscapeCommand(_) => {}
    }
}

fn walk_comprehension(comprehension: &ast::Comprehension, visit: &mut impl FnMut(&Expr)) {
    walk_expression(&comprehension.target, visit);
    walk_expression(&comprehension.iter, visit);
    for if_expr in &comprehension.ifs {
        walk_expression(if_expr, visit);
    }
}

fn is_dataclass_expr(expr: &ast::Expr) -> bool {
    let expr = expr.as_call_expr().map_or(expr, |call| call.func.as_ref());
    matches!(expr_tail_name(expr), Some("dataclass"))
}

fn is_pydantic_base_expr(expr: &ast::Expr) -> bool {
    matches!(expr_tail_name(expr), Some("BaseModel" | "BaseSettings"))
}

fn is_typeddict_base_expr(expr: &ast::Expr) -> bool {
    matches!(expr_tail_name(expr), Some("TypedDict"))
}

fn is_enum_class(class_def: &ast::StmtClassDef) -> bool {
    class_def.arguments.as_deref().is_some_and(|arguments| {
        arguments.args.iter().any(|expr| {
            matches!(
                expr_tail_name(expr),
                Some("Enum" | "IntEnum" | "StrEnum" | "Flag" | "IntFlag")
            )
        })
    })
}

fn expr_tail_name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Name(name) => Some(name.id.as_str()),
        ast::Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        ast::Expr::Subscript(subscript) => expr_tail_name(&subscript.value),
        _ => None,
    }
}

fn has_blank_line_between(previous: TextRange, current: TextRange, source: &str) -> bool {
    source[TextRange::new(previous.end(), current.start())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        > 1
}

fn display_width(identifier: &str) -> u32 {
    u32::try_from(
        identifier
            .chars()
            .map(|character| UnicodeWidthChar::width(character).unwrap_or_default())
            .sum::<usize>(),
    )
    .unwrap_or(u32::MAX)
}

fn display_width_source(source: &str) -> u32 {
    u32::try_from(
        source
            .chars()
            .filter(|character| *character != '\n')
            .map(|character| UnicodeWidthChar::width(character).unwrap_or_default())
            .sum::<usize>(),
    )
    .unwrap_or(u32::MAX)
}

fn display_width_for_line(range: TextRange, source: &str) -> usize {
    let start = range.start().to_usize();
    let line_start = line_start_offset(range.start(), source);

    let indent_width = source[line_start..start]
        .chars()
        .map(|character| {
            if character == '\t' {
                8
            } else {
                UnicodeWidthChar::width(character).unwrap_or_default()
            }
        })
        .sum::<usize>();

    indent_width + normalized_display_width(&source[range])
}

fn line_start_offset(position: ruff_text_size::TextSize, source: &str) -> usize {
    let start = position.to_usize();
    source[..start]
        .rfind('\n')
        .map_or(0, |index| index.saturating_add(1))
}

fn normalized_display_width(source: &str) -> usize {
    let mut width = 0;
    let mut in_whitespace = false;

    for character in source.chars() {
        if character.is_whitespace() {
            if !in_whitespace {
                width += 1;
                in_whitespace = true;
            }
        } else {
            width += UnicodeWidthChar::width(character).unwrap_or_default();
            in_whitespace = false;
        }
    }

    width
}

fn may_expand_collection_default(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Dict(expr) => !expr.items.is_empty(),
        ast::Expr::Set(expr) => !expr.elts.is_empty(),
        ast::Expr::List(expr) => !expr.elts.is_empty(),
        ast::Expr::Tuple(expr) => !expr.elts.is_empty(),
        _ => false,
    }
}

fn is_simple_separator_value(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Name(_)
        | ast::Expr::Attribute(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::BytesLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::BooleanLiteral(_)
        | ast::Expr::NoneLiteral(_)
        | ast::Expr::EllipsisLiteral(_) => true,
        ast::Expr::UnaryOp(unary) => is_simple_separator_value(&unary.operand),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug)]
struct ClassFieldCandidate {
    range: TextRange,
    name_width: u32,
    annotation_width: u32,
    line_width: usize,
    has_default: bool,
}

#[derive(Clone, Copy, Debug)]
struct ParameterCandidate {
    parameter_range: TextRange,
    containing_range: TextRange,
    name_width: u32,
    annotation_width: u32,
    line_width: usize,
    has_default: bool,
}

#[derive(Clone, Copy, Debug)]
struct AssignmentCandidate {
    range: TextRange,
    target_width: u32,
}

#[derive(Clone, Copy, Debug)]
struct SeparatorCandidate {
    range: TextRange,
    left_width: u32,
}

#[derive(Clone, Copy, Debug)]
struct AliasCandidate {
    range: TextRange,
    name_width: u32,
}

#[derive(Clone, Copy, Debug)]
struct ArgumentAlignmentCandidate {
    range: TextRange,
    width: u32,
}

#[derive(Clone, Debug)]
struct RepeatedCallCandidate<'a> {
    callee: &'a str,
    args: Vec<ArgumentAlignmentCandidate>,
}

#[derive(Clone, Copy, Debug)]
struct WithItemCandidate {
    range: TextRange,
    context_width: u32,
}

#[derive(Clone, Copy, Debug)]
struct TrailingCommentCandidate {
    range: TextRange,
    code_width: u32,
}
