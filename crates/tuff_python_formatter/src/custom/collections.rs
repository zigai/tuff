use ruff_formatter::{GroupId, format_args};
use ruff_python_ast::{Expr, StmtDelete};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::custom::hooks::{self, CollectionDecision, CollectionSubject};
use crate::custom::options::{CollectionLayout, CollectionPolicy};
use crate::expression::expr_dict::KeyValuePair;
use crate::other::commas::has_magic_trailing_comma;
use crate::prelude::*;

pub(crate) fn can_pack(f: &PyFormatter) -> bool {
    f.context().docstring().is_none()
        && f.context()
            .interpolated_string_state()
            .can_contain_line_breaks()
            != Some(false)
}

pub(crate) fn policy<'f>(f: &'f PyFormatter, subject: CollectionSubject) -> &'f CollectionPolicy {
    let options = &f.context().custom_options().collections;
    match subject {
        CollectionSubject::List => &options.lists,
        CollectionSubject::Dict => &options.dicts,
        CollectionSubject::Tuple => &options.tuples,
        CollectionSubject::Set => &options.sets,
    }
}

pub(crate) fn has_comments(f: &PyFormatter, range: TextRange) -> bool {
    !f.context()
        .trivia()
        .comments()
        .comments_in_range(range)
        .is_empty()
}

// An empty scan range suppresses the builder's magic-comma signal without
// changing the comma required by a singleton tuple.
pub(crate) fn sequence_end(
    f: &PyFormatter,
    subject: CollectionSubject,
    last_end: TextSize,
    end: TextSize,
) -> TextSize {
    if can_pack(f) && policy(f, subject).ignore_magic_trailing_comma {
        last_end
    } else {
        end
    }
}

pub(crate) fn magic_comma(f: &PyFormatter, last_end: TextSize, end: TextSize) -> bool {
    has_magic_trailing_comma(TextRange::new(last_end, end), f.context())
}

// Include commas in each fill entry so that fitting also accounts for the
// punctuation at the end of a packed row.
pub(crate) fn packed<'ast, I, T>(
    f: &mut PyFormatter<'ast, '_>,
    left: &'static str,
    right: &'static str,
    entries: I,
    multiline: bool,
    optional: bool,
) -> FormatResult<()>
where
    I: IntoIterator<Item = T>,
    T: Format<PyFormatContext<'ast>>,
{
    let id = f.group_id("compact collection");
    let content = format_once(|f| {
        let mut entries = entries.into_iter().peekable();
        let mut fill = f.fill();
        while let Some(entry) = entries.next() {
            let comma = entries.peek().is_some() || multiline;
            fill.entry(
                &soft_line_break_or_space(),
                &format_args![entry, EntryComma { comma, id }],
            );
        }
        fill.finish()
    });
    if optional {
        crate::custom::packing::optional_delimited(f, left, &content, right, id, multiline)
    } else {
        crate::custom::packing::delimited(f, left, &content, right, id, multiline)
    }
}

struct EntryComma {
    comma: bool,
    id: GroupId,
}

impl Format<PyFormatContext<'_>> for EntryComma {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        if self.comma {
            token(",").fmt(f)
        } else {
            if_group_breaks(&token(","))
                .with_group_id(Some(self.id))
                .fmt(f)
        }
    }
}

pub(crate) fn expression(
    f: &mut PyFormatter,
    expression: &Expr,
    outer: CollectionSubject,
) -> FormatResult<()> {
    if !can_pack(f) || !policy(f, outer).compact_nested {
        return expression.format().fmt(f);
    }
    let (subject, last_end, count) = match expression {
        Expr::List(node) => (
            CollectionSubject::List,
            node.elts.last().map(Ranged::end),
            node.elts.len(),
        ),
        Expr::Set(node) => (
            CollectionSubject::Set,
            node.elts.last().map(Ranged::end),
            node.elts.len(),
        ),
        Expr::Tuple(node) if node.elts.len() > 1 => (
            CollectionSubject::Tuple,
            node.elts.last().map(Ranged::end),
            node.elts.len(),
        ),
        Expr::Dict(node) => (
            CollectionSubject::Dict,
            node.items.last().map(Ranged::end),
            node.items.len(),
        ),
        _ => return expression.format().fmt(f),
    };
    let Some(last_end) = last_end else {
        return expression.format().fmt(f);
    };
    let decision = hooks::collection_layout(
        f,
        subject,
        count,
        has_comments(f, expression.range()),
        magic_comma(f, last_end, expression.end()),
    );
    // Explicit expansion and input-preservation policies take precedence over
    // the enclosing collection's preference for compact rows.
    if matches!(
        decision,
        CollectionDecision::ForceExpanded
            | CollectionDecision::PreserveInput
            | CollectionDecision::Fill
    ) || has_comments(f, expression.range())
        || f.context().comments().contains_comments(expression.into())
        || matches!(policy(f, subject).layout, CollectionLayout::PreserveInput)
    {
        return expression.format().fmt(f);
    }
    let multiline = magic_comma(f, last_end, expression.end())
        && !policy(f, subject).ignore_magic_trailing_comma;
    FormatNodeRule::fmt(&NestedRow { multiline }, expression, f)
}

struct NestedRow {
    multiline: bool,
}

impl FormatNodeRule<Expr> for NestedRow {
    fn fmt_fields(&self, expression: &Expr, f: &mut PyFormatter) -> FormatResult<()> {
        match expression {
            Expr::List(node) => packed(
                f,
                "[",
                "]",
                node.elts
                    .iter()
                    .map(|expr| Element(expr, CollectionSubject::List)),
                self.multiline,
                false,
            ),
            Expr::Set(node) => packed(
                f,
                "{",
                "}",
                node.elts
                    .iter()
                    .map(|expr| Element(expr, CollectionSubject::Set)),
                self.multiline,
                false,
            ),
            Expr::Tuple(node) if node.elts.len() > 1 => packed(
                f,
                "(",
                ")",
                node.elts
                    .iter()
                    .map(|expr| Element(expr, CollectionSubject::Tuple)),
                self.multiline,
                false,
            ),
            Expr::Dict(node) => packed(
                f,
                "{",
                "}",
                node.items
                    .iter()
                    .map(|item| KeyValuePair::new(item).with_alignment(false)),
                self.multiline,
                false,
            ),
            _ => expression.format().fmt(f),
        }
    }
}

pub(crate) struct Element<'a>(pub(crate) &'a Expr, pub(crate) CollectionSubject);

impl Format<PyFormatContext<'_>> for Element<'_> {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        hooks::before_collection_element(f, self.0)?;
        expression(f, self.0, self.1)
    }
}

// Multiple delete targets become an ExprTuple when parentheses are introduced.
// Both forms need the same layout to keep formatting stable.
pub(crate) fn delete(item: &StmtDelete, f: &mut PyFormatter) -> FormatResult<bool> {
    if item.targets.len() < 2 {
        return Ok(false);
    }
    let layout = hooks::collection_layout(
        f,
        CollectionSubject::Tuple,
        item.targets.len(),
        has_comments(f, item.range()),
        magic_comma(
            f,
            item.targets.last().map_or(item.end(), Ranged::end),
            item.end(),
        ),
    );
    if !matches!(
        layout,
        CollectionDecision::PreferCompact | CollectionDecision::CompactExpanded
    ) {
        return Ok(false);
    }
    packed(
        f,
        "(",
        ")",
        item.targets
            .iter()
            .map(|target| Element(target, CollectionSubject::Tuple)),
        layout == CollectionDecision::CompactExpanded,
        true,
    )?;
    Ok(true)
}
