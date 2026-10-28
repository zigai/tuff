use ruff_formatter::prelude::format_with;
use ruff_formatter::write;
use ruff_python_ast::AnyNodeRef;
use ruff_python_ast::ExprList;
use ruff_text_size::Ranged;

use crate::custom::collections::{self, Element};
use crate::custom::hooks::{self, CollectionDecision, CollectionSubject};
use crate::expression::parentheses::{
    NeedsParentheses, OptionalParentheses, empty_parenthesized, parenthesized,
};
use crate::prelude::*;

#[derive(Default)]
pub struct FormatExprList;

impl FormatNodeRule<ExprList> for FormatExprList {
    fn fmt_fields(&self, item: &ExprList, f: &mut PyFormatter) -> FormatResult<()> {
        let ExprList {
            range: _,
            node_index: _,
            elts,
            ctx: _,
        } = item;

        let comments = f.context().comments().clone();
        let dangling = comments.dangling(item);

        if elts.is_empty() {
            return empty_parenthesized("[", dangling, "]").fmt(f);
        }

        let items = format_with(|f| {
            let end = collections::sequence_end(
                f,
                CollectionSubject::List,
                elts.last().map_or(item.end(), Ranged::end),
                item.end(),
            );
            f.join_comma_separated(end)
                .entries(
                    elts.iter()
                        .map(|element| (element, Element(element, CollectionSubject::List))),
                )
                .finish()
        });

        let collection_layout = hooks::collection_layout(
            f,
            CollectionSubject::List,
            elts.len(),
            collections::has_comments(f, item.range()),
            collections::magic_comma(f, elts.last().map_or(item.end(), Ranged::end), item.end()),
        );
        let source_is_multiline = f.context().source()[item.range()].contains('\n');

        match collection_layout {
            CollectionDecision::ForceExpanded => {
                write!(f, [token("["), block_indent(&items), token("]")])
            }
            CollectionDecision::PreferCompact => collections::packed(
                f,
                "[",
                "]",
                elts.iter()
                    .map(|element| Element(element, CollectionSubject::List)),
                false,
                false,
            ),
            CollectionDecision::CompactExpanded | CollectionDecision::Fill
                if source_is_multiline
                    || collection_layout == CollectionDecision::CompactExpanded =>
            {
                collections::packed(
                    f,
                    "[",
                    "]",
                    elts.iter()
                        .map(|element| Element(element, CollectionSubject::List)),
                    true,
                    false,
                )
            }
            CollectionDecision::UseRuffDefault
            | CollectionDecision::Fill
            | CollectionDecision::CompactExpanded
            | CollectionDecision::PreserveInput => parenthesized("[", &items, "]")
                .with_dangling_comments(dangling)
                .fmt(f),
        }
    }
}

impl NeedsParentheses for ExprList {
    fn needs_parentheses(
        &self,
        _parent: AnyNodeRef,
        _context: &PyFormatContext,
    ) -> OptionalParentheses {
        OptionalParentheses::Never
    }
}
