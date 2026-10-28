use std::num::NonZeroU8;

use ruff_formatter::format_element::TextWidth;
use ruff_formatter::format_element::document::Document;
use ruff_formatter::prelude::{space, text, token};
use ruff_formatter::{Format, FormatElement, FormatOptions, FormatResult, Formatted};
use ruff_python_ast as ast;
use ruff_python_ast::{AnyNodeRef, Expr, Stmt};
use ruff_text_size::Ranged;

use crate::PyFormatter;
use crate::comments::SourceComment;
use crate::custom::options::{CollectionLayout, DictAlignmentMode, FeatureMode};
pub(crate) use crate::custom::options::{OneLineSuiteClause, OneLineSuiteStatement};

#[derive(Clone, Copy)]
pub(crate) enum AnnotationSubject<'a> {
    ClassField(&'a ast::StmtAnnAssign),
    FunctionParameter(&'a ast::Parameter),
}

#[derive(Clone, Copy)]
pub(crate) enum AssignmentSubject<'a> {
    Assignment(&'a ast::StmtAssign),
    AnnotatedAssignment(&'a ast::StmtAnnAssign),
}

#[derive(Clone, Copy)]
pub(crate) enum CollectionSubject {
    List,
    Dict,
    Tuple,
    Set,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CollectionDecision {
    UseRuffDefault,
    PreferCompact,
    CompactExpanded,
    Fill,
    ForceExpanded,
    PreserveInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OneLineSuiteDecision {
    UseBlock,
    PreferOneLine,
}

pub(crate) fn after_annotation_colon(
    f: &mut PyFormatter,
    subject: AnnotationSubject<'_>,
) -> FormatResult<()> {
    let decision = match subject {
        AnnotationSubject::ClassField(stmt) => f
            .context()
            .custom_layout()
            .alignment
            .class_fields
            .get(&stmt.range()),
        AnnotationSubject::FunctionParameter(parameter) => f
            .context()
            .custom_layout()
            .alignment
            .function_params
            .get(&parameter.range()),
    };

    if let Some(decision) = decision
        && decision.padding_after_separator > 0
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator));
        text(&padding).fmt(f)?;
    }

    Ok(())
}

pub(crate) fn before_assignment_operator(
    f: &mut PyFormatter,
    subject: AssignmentSubject<'_>,
) -> FormatResult<()> {
    let decision = match subject {
        AssignmentSubject::Assignment(stmt) => f
            .context()
            .custom_layout()
            .alignment
            .assignments
            .get(&stmt.range()),
        AssignmentSubject::AnnotatedAssignment(stmt) => f
            .context()
            .custom_layout()
            .alignment
            .class_field_defaults
            .get(&stmt.range()),
    };

    if let Some(decision) = decision
        && decision.padding_after_separator > 0
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator));
        text(&padding).fmt(f)?;
    }

    Ok(())
}

pub(crate) fn before_default_equals(
    f: &mut PyFormatter,
    parameter: &ast::ParameterWithDefault,
) -> FormatResult<()> {
    let decision = f
        .context()
        .custom_layout()
        .alignment
        .function_param_defaults
        .get(&parameter.range());

    if let Some(decision) = decision
        && decision.padding_after_separator > 0
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator));
        text(&padding).fmt(f)?;
    }

    Ok(())
}

pub(crate) fn dict_key_value_separator(
    f: &mut PyFormatter,
    item: &ast::DictItem,
    aligned: bool,
) -> FormatResult<()> {
    if !aligned {
        token(":").fmt(f)?;
        return space().fmt(f);
    }
    let decision = f
        .context()
        .custom_layout()
        .alignment
        .dict_values
        .get(&item.range());

    if matches!(
        f.context().custom_options().alignment.dict_alignment,
        DictAlignmentMode::Colon
    ) && let Some(decision) = decision
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator));
        text(&padding).fmt(f)?;
    }

    token(":").fmt(f)?;

    if matches!(
        f.context().custom_options().alignment.dict_alignment,
        DictAlignmentMode::Value
    ) || matches!(
        f.context().custom_options().alignment.dict_values,
        crate::custom::options::AlignmentMode::Enabled
    ) {
        separator_space(f, SeparatorSubject(item))
    } else {
        space().fmt(f)
    }
}

pub(crate) fn keyword_separator(f: &mut PyFormatter, keyword: &ast::Keyword) -> FormatResult<()> {
    if let Some(decision) = f
        .context()
        .custom_layout()
        .alignment
        .call_keyword_args
        .get(&keyword.range())
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator) + 1);
        text(&padding).fmt(f)?;
        token("=").fmt(f)?;
        space().fmt(f)
    } else {
        token("=").fmt(f)
    }
}

pub(crate) fn before_alias_as(f: &mut PyFormatter, alias: &ast::Alias) -> FormatResult<()> {
    if let Some(decision) = f
        .context()
        .custom_layout()
        .alignment
        .import_aliases
        .get(&alias.range())
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator) + 1);
        text(&padding).fmt(f)
    } else {
        space().fmt(f)
    }
}

pub(crate) fn before_with_item_as(f: &mut PyFormatter, item: &ast::WithItem) -> FormatResult<()> {
    if let Some(decision) = f
        .context()
        .custom_layout()
        .alignment
        .with_items
        .get(&item.range())
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator) + 1);
        text(&padding).fmt(f)
    } else {
        space().fmt(f)
    }
}

pub(crate) fn before_collection_element(f: &mut PyFormatter, expr: &Expr) -> FormatResult<()> {
    before_aligned_expression(f, expr, ExpressionAlignmentSubject::CollectionRow)
}

pub(crate) fn before_call_argument(f: &mut PyFormatter, expr: &Expr) -> FormatResult<()> {
    before_aligned_expression(f, expr, ExpressionAlignmentSubject::RepeatedCall)
}

fn before_aligned_expression(
    f: &mut PyFormatter,
    expr: &Expr,
    subject: ExpressionAlignmentSubject,
) -> FormatResult<()> {
    let decision = match subject {
        ExpressionAlignmentSubject::CollectionRow => f
            .context()
            .custom_layout()
            .alignment
            .collection_row_items
            .get(&expr.range()),
        ExpressionAlignmentSubject::RepeatedCall => f
            .context()
            .custom_layout()
            .alignment
            .repeated_call_args
            .get(&expr.range()),
    };

    if let Some(decision) = decision
        && decision.padding_after_separator > 0
    {
        let padding = " ".repeat(usize::from(decision.padding_after_separator));
        text(&padding).fmt(f)?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ExpressionAlignmentSubject {
    CollectionRow,
    RepeatedCall,
}

pub(crate) fn before_trailing_comment(
    f: &mut PyFormatter,
    comment: &SourceComment,
) -> FormatResult<()> {
    if let Some(decision) = f
        .context()
        .custom_layout()
        .alignment
        .trailing_comments
        .get(&comment.range())
    {
        if decision.padding_after_separator > 0 {
            let padding = " ".repeat(usize::from(decision.padding_after_separator));
            text(&padding).fmt(f)?;
        }
    }

    Ok(())
}

fn separator_space(f: &mut PyFormatter, subject: SeparatorSubject<'_>) -> FormatResult<()> {
    let decision = f
        .context()
        .custom_layout()
        .alignment
        .dict_values
        .get(&subject.0.range());

    if let Some(decision) = decision {
        let padding = " ".repeat(usize::from(decision.padding_after_separator) + 1);
        text(&padding).fmt(f)
    } else {
        space().fmt(f)
    }
}

#[derive(Clone, Copy)]
struct SeparatorSubject<'a>(&'a ast::DictItem);

pub(crate) fn collection_layout(
    f: &PyFormatter,
    subject: CollectionSubject,
    item_count: usize,
    has_comments: bool,
    has_magic_trailing_comma: bool,
) -> CollectionDecision {
    let policy = crate::custom::collections::policy(f, subject);
    if !crate::custom::collections::can_pack(f) || has_comments {
        return CollectionDecision::UseRuffDefault;
    }

    match policy.layout {
        CollectionLayout::RuffDefault => CollectionDecision::UseRuffDefault,
        CollectionLayout::PreferCompact
            if has_magic_trailing_comma && !policy.ignore_magic_trailing_comma =>
        {
            CollectionDecision::CompactExpanded
        }
        CollectionLayout::PreferCompact => CollectionDecision::PreferCompact,
        CollectionLayout::Fill => CollectionDecision::Fill,
        CollectionLayout::ForceExpanded => CollectionDecision::ForceExpanded,
        CollectionLayout::ExpandIfMoreThan { threshold } => {
            if item_count > usize::from(threshold) {
                CollectionDecision::ForceExpanded
            } else {
                CollectionDecision::UseRuffDefault
            }
        }
        CollectionLayout::PreserveInput => CollectionDecision::PreserveInput,
    }
}

pub(crate) fn one_line_suite_layout(
    f: &PyFormatter,
    clause: OneLineSuiteClause,
    body: &[Stmt],
    has_trailing_colon_comment: bool,
) -> OneLineSuiteDecision {
    let options = &f.context().custom_options().one_line_suites;

    if f.context().docstring().is_some()
        || !matches!(options.mode, FeatureMode::Enabled)
        || has_trailing_colon_comment
        || body.len() != 1
        || !options.clauses.contains(&clause)
    {
        return OneLineSuiteDecision::UseBlock;
    }

    let statement = &body[0];
    let statement_comments = f
        .context()
        .comments()
        .leading_dangling_trailing(AnyNodeRef::from(statement));

    if !statement_comments.leading.is_empty()
        || !statement_comments.dangling.is_empty()
        || !statement_comments.trailing.is_empty()
        || (matches!(
            clause,
            OneLineSuiteClause::Function | OneLineSuiteClause::Class
        ) && f.context().comments().contains_comments(statement.into()))
    {
        return OneLineSuiteDecision::UseBlock;
    }

    // A leading string expression in a definition is a docstring, not an ordinary
    // expression statement. Keep its opt-in independent of the expression allowlist.
    if matches!(
        clause,
        OneLineSuiteClause::Function | OneLineSuiteClause::Class
    ) && let Stmt::Expr(expression) = statement
        && expression.value.is_string_literal_expr()
    {
        return if matches!(options.docstrings, FeatureMode::Enabled) {
            OneLineSuiteDecision::PreferOneLine
        } else {
            OneLineSuiteDecision::UseBlock
        };
    }

    if statement_is_allowed(statement, &options.statements) {
        OneLineSuiteDecision::PreferOneLine
    } else {
        OneLineSuiteDecision::UseBlock
    }
}

pub(crate) fn one_line_definition_fits(
    f: &PyFormatter,
    content: &[FormatElement],
) -> FormatResult<bool> {
    let document = Document::from(content.to_vec());
    let indentation = f
        .context()
        .indent_level()
        .to_ascii_spaces(NonZeroU8::MIN.into());
    let printed = Formatted::new(document, f.context()).print_with_indent(indentation)?;
    let width = TextWidth::from_text(
        printed.as_code().trim_end_matches(['\r', '\n']),
        f.options().indent_width(),
    );
    Ok(width
        .width()
        .is_some_and(|width| width.value() <= u32::from(f.options().line_width().value())))
}

fn statement_is_allowed(statement: &Stmt, allowed: &[OneLineSuiteStatement]) -> bool {
    match statement {
        Stmt::Return(_) => allowed.contains(&OneLineSuiteStatement::Return),
        Stmt::Raise(_) => allowed.contains(&OneLineSuiteStatement::Raise),
        Stmt::Assert(_) => allowed.contains(&OneLineSuiteStatement::Assert),
        Stmt::Pass(_) => allowed.contains(&OneLineSuiteStatement::Pass),
        Stmt::Break(_) => allowed.contains(&OneLineSuiteStatement::Break),
        Stmt::Continue(_) => allowed.contains(&OneLineSuiteStatement::Continue),
        Stmt::Assign(_) => allowed.contains(&OneLineSuiteStatement::Assignment),
        Stmt::AnnAssign(_) => allowed.contains(&OneLineSuiteStatement::AnnotatedAssignment),
        Stmt::Expr(expr) => {
            allowed.contains(&OneLineSuiteStatement::Expression)
                || (expr.value.is_call_expr() && allowed.contains(&OneLineSuiteStatement::Call))
                || (expr.value.is_ellipsis_literal_expr()
                    && allowed.contains(&OneLineSuiteStatement::Ellipsis))
        }
        Stmt::FunctionDef(_)
        | Stmt::ClassDef(_)
        | Stmt::Delete(_)
        | Stmt::TypeAlias(_)
        | Stmt::AugAssign(_)
        | Stmt::For(_)
        | Stmt::While(_)
        | Stmt::If(_)
        | Stmt::With(_)
        | Stmt::Match(_)
        | Stmt::Try(_)
        | Stmt::Import(_)
        | Stmt::ImportFrom(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_)
        | Stmt::IpyEscapeCommand(_) => false,
    }
}
