//! Opt-in row packing for argument lists, signatures, and imports.
//!
//! Commented lists retain the upstream layout. Alignment takes precedence over
//! packing so padding planned for one-entry-per-line layouts remains meaningful.

use ruff_formatter::format_element::TextWidth;
use ruff_formatter::prelude::tag::Condition;
use ruff_formatter::{FormatOptions, GroupId, format_args, write};
use ruff_python_ast::{
    Alias, ArgOrKeyword, Arguments, Expr, Parameter, ParameterWithDefault, Parameters,
    StmtImportFrom,
};
use ruff_python_trivia::{SimpleTokenKind, SimpleTokenizer};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::context::{NodeLevel, WithNodeLevel};
use crate::custom::hooks;
use crate::other::commas::has_magic_trailing_comma;
use crate::prelude::*;

fn eligible(f: &PyFormatter, range: TextRange) -> bool {
    f.context().docstring().is_none()
        && f.context()
            .interpolated_string_state()
            .can_contain_line_breaks()
            != Some(false)
        && f.context()
            .trivia()
            .comments()
            .comments_in_range(range)
            .is_empty()
        && f.context()
            .custom_options()
            .alignment
            .trailing_comments
            .is_disabled()
}

fn can_pack_arguments(item: &Arguments, f: &PyFormatter) -> bool {
    let options = &f.context().custom_options().arguments;
    let alignment = &f.context().custom_options().alignment;
    (!options.calls.is_disabled() || !options.mixed.is_disabled())
        && eligible(f, item.range())
        && !f
            .context()
            .custom_layout()
            .unpacked_arguments
            .contains(&item.range())
        && alignment.call_keyword_args.is_disabled()
        && alignment.repeated_call_args.is_disabled()
}

pub(crate) fn argument_sequence_end(item: &Arguments, f: &PyFormatter) -> TextSize {
    if f.context()
        .custom_options()
        .arguments
        .ignore_magic_trailing_comma
        && can_pack_arguments(item, f)
    {
        item.iter_source_order()
            .last()
            .map_or(item.end(), |arg| arg.end())
    } else {
        item.end()
    }
}

pub(crate) fn arguments(item: &Arguments, f: &mut PyFormatter) -> FormatResult<bool> {
    // Preserve the upstream single-argument hugging and generator-expression rules.
    if item.args.len() + item.keywords.len() < 2 || !can_pack_arguments(item, f) {
        return Ok(false);
    }
    let options = &f.context().custom_options().arguments;
    let entries: Vec<_> = item.iter_source_order().map(Entry::Argument).collect();
    // Mixed mode alone leaves purely atomic argument lists alone. Complex
    // entries are measured independently by fill, so a multiline entry starts
    // and ends a row rather than expanding its short neighbours.
    if options.calls.is_disabled()
        && !item.iter_source_order().any(|arg| match arg {
            ArgOrKeyword::Arg(expr) => complex(expr, f),
            ArgOrKeyword::Keyword(keyword) => complex(&keyword.value, f),
        })
    {
        return Ok(false);
    }
    let last_end = item
        .iter_source_order()
        .last()
        .map_or(item.start(), |arg| arg.end());
    let magic = !options.ignore_magic_trailing_comma
        && has_magic_trailing_comma(TextRange::new(last_end, item.end()), f.context());
    packed(f, &entries, magic, false)?;
    Ok(true)
}

fn complex(expr: &Expr, f: &PyFormatter) -> bool {
    match expr {
        Expr::Name(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => false,
        Expr::StringLiteral(string) => {
            TextWidth::from_text(string.value.to_str(), f.options().indent_width())
                .width()
                .is_none_or(|width| {
                    width.value().saturating_add(2) > u32::from(f.options().line_width().value())
                })
        }
        Expr::BytesLiteral(bytes) => {
            bytes
                .value
                .bytes()
                .any(|byte| matches!(byte, b'\n' | b'\r'))
                || bytes.value.len() > usize::from(f.options().line_width().value())
        }
        _ => true,
    }
}

fn can_pack_parameters(item: &Parameters, f: &PyFormatter) -> bool {
    !f.context()
        .custom_options()
        .arguments
        .parameters
        .is_disabled()
        && eligible(f, item.range())
        && f.context()
            .custom_options()
            .alignment
            .function_params
            .is_disabled()
}

pub(crate) fn ignore_parameter_magic_comma(item: &Parameters, f: &PyFormatter) -> bool {
    f.context()
        .custom_options()
        .arguments
        .ignore_magic_trailing_comma
        && can_pack_parameters(item, f)
}

pub(crate) fn parameters(item: &Parameters, f: &mut PyFormatter) -> FormatResult<bool> {
    if item.is_empty() || !can_pack_parameters(item, f) {
        return Ok(false);
    }
    let options = &f.context().custom_options().arguments;
    let mut entries = Vec::new();
    for parameter in &item.posonlyargs {
        entries.push(Entry::Parameter(parameter));
    }
    if !item.posonlyargs.is_empty() {
        entries.push(Entry::Marker("/"));
    }
    for parameter in &item.args {
        entries.push(Entry::Parameter(parameter));
    }
    if let Some(parameter) = &item.vararg {
        entries.push(Entry::Variadic("*", parameter));
    } else if !item.kwonlyargs.is_empty() {
        entries.push(Entry::Marker("*"));
    }
    for parameter in &item.kwonlyargs {
        entries.push(Entry::Parameter(parameter));
    }
    if let Some(parameter) = &item.kwarg {
        entries.push(Entry::Variadic("**", parameter));
    }
    if entries.len() < 2 {
        return Ok(false);
    }
    // A final positional-only slash can follow the last AST parameter. Scan
    // through that separator rather than mistaking the comma before it for a
    // magic trailing comma.
    let last_end = item
        .iter_source_order()
        .last()
        .map_or(item.start(), |param| param.end());
    let mut last_kind = None;
    for token in SimpleTokenizer::new(f.context().source(), TextRange::new(last_end, item.end()))
        .skip_trivia()
    {
        if token.kind() != SimpleTokenKind::RParen {
            last_kind = Some(token.kind());
        }
    }
    let magic = !options.ignore_magic_trailing_comma
        && f.options().magic_trailing_comma().is_respect()
        && last_kind == Some(SimpleTokenKind::Comma);
    packed(f, &entries, magic, false)?;
    Ok(true)
}

pub(crate) fn imports(item: &StmtImportFrom, f: &mut PyFormatter) -> FormatResult<bool> {
    let options = &f.context().custom_options().imports;
    if options.mode.is_disabled()
        || item.names.is_empty()
        || item.names.iter().any(|name| name.name.as_str() == "*")
        || !eligible(f, item.range())
        || !f
            .context()
            .custom_options()
            .alignment
            .import_aliases
            .is_disabled()
    {
        return Ok(false);
    }
    let entries: Vec<_> = item.names.iter().map(Entry::Alias).collect();
    let last_end = item.names.last().map_or(item.start(), Ranged::end);
    let magic = !options.ignore_magic_trailing_comma
        && has_magic_trailing_comma(TextRange::new(last_end, item.end()), f.context());
    packed(f, &entries, magic, true)?;
    Ok(true)
}

fn packed(
    f: &mut PyFormatter,
    entries: &[Entry<'_>],
    magic: bool,
    optional: bool,
) -> FormatResult<()> {
    let id = f.group_id("packed rows");
    let content = format_with(|f: &mut PyFormatter| {
        let mut fill = f.fill();
        for (index, entry) in entries.iter().enumerate() {
            fill.entry(
                &soft_line_break_or_space(),
                &format_args![
                    entry,
                    Comma {
                        last: index + 1 == entries.len(),
                        id
                    }
                ],
            );
        }
        fill.finish()
    });
    if optional {
        optional_delimited(f, "(", &content, ")", id, magic)
    } else {
        delimited(f, "(", &content, ")", id, magic)
    }
}

pub(crate) fn optional_delimited<'ast>(
    f: &mut PyFormatter<'ast, '_>,
    left: &'static str,
    content: &impl Format<PyFormatContext<'ast>>,
    right: &'static str,
    id: GroupId,
    expanded: bool,
) -> FormatResult<()> {
    let mut f = WithNodeLevel::new(NodeLevel::ParenthesizedExpression, f);
    group(&format_args![
        if_group_breaks(&token(left)).with_group_id(Some(id)),
        soft_block_indent(content),
        if_group_breaks(&token(right)).with_group_id(Some(id)),
    ])
    .with_id(Some(id))
    .should_expand(expanded)
    .fmt(&mut f)
}

pub(crate) fn delimited<'ast>(
    f: &mut PyFormatter<'ast, '_>,
    left: &'static str,
    content: &impl Format<PyFormatContext<'ast>>,
    right: &'static str,
    id: GroupId,
    expanded: bool,
) -> FormatResult<()> {
    let level = f.context().node_level();
    let block = soft_block_indent(content);
    let indented = group(&block).with_id(Some(id)).should_expand(expanded);
    let inner = format_with(|f| {
        if let NodeLevel::Expression(Some(parentheses)) = level {
            // Breaking inside delimiters must not force optional outer parentheses.
            fits_expanded(&indented)
                .with_condition(Some(Condition::if_group_fits_on_line(parentheses)))
                .fmt(f)
        } else {
            indented.fmt(f)
        }
    });
    let mut f = WithNodeLevel::new(NodeLevel::ParenthesizedExpression, f);
    write!(f, [token(left), inner, token(right)])
}

struct Comma {
    last: bool,
    id: GroupId,
}

impl Format<PyFormatContext<'_>> for Comma {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        if self.last {
            if_group_breaks(&token(","))
                .with_group_id(Some(self.id))
                .fmt(f)
        } else {
            token(",").fmt(f)
        }
    }
}

enum Entry<'a> {
    Argument(ArgOrKeyword<'a>),
    Parameter(&'a ParameterWithDefault),
    Variadic(&'static str, &'a Parameter),
    Marker(&'static str),
    Alias(&'a Alias),
}

impl Format<PyFormatContext<'_>> for Entry<'_> {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        match self {
            Self::Argument(ArgOrKeyword::Arg(arg)) => {
                hooks::before_call_argument(f, arg)?;
                arg.format().fmt(f)
            }
            Self::Argument(ArgOrKeyword::Keyword(keyword)) => keyword.format().fmt(f),
            Self::Parameter(parameter) => parameter.format().fmt(f),
            Self::Variadic(prefix, parameter) => write!(f, [token(prefix), parameter.format()]),
            Self::Marker(marker) => token(marker).fmt(f),
            Self::Alias(alias) => alias.format().fmt(f),
        }
    }
}
