use ruff_formatter::write;
use ruff_python_ast::AnyNodeRef;
use ruff_python_ast::ExprSet;
use ruff_text_size::Ranged;

use crate::custom::collections::{self, Element};
use crate::custom::hooks::{self, CollectionDecision, CollectionSubject};
use crate::expression::parentheses::{NeedsParentheses, OptionalParentheses, parenthesized};
use crate::prelude::*;

#[derive(Default)]
pub struct FormatExprSet;

impl FormatNodeRule<ExprSet> for FormatExprSet {
    fn fmt_fields(&self, item: &ExprSet, f: &mut PyFormatter) -> FormatResult<()> {
        let ExprSet {
            range: _,
            node_index: _,
            elts,
        } = item;
        // That would be a dict expression
        assert!(!elts.is_empty());
        // Avoid second mutable borrow of f
        let joined = format_with(|f: &mut PyFormatter| {
            let end = collections::sequence_end(
                f,
                CollectionSubject::Set,
                elts.last().map_or(item.end(), Ranged::end),
                item.end(),
            );
            f.join_comma_separated(end)
                .entries(
                    elts.iter()
                        .map(|element| (element, Element(element, CollectionSubject::Set))),
                )
                .finish()
        });

        let comments = f.context().comments().clone();
        let dangling = comments.dangling(item);

        let collection_layout = hooks::collection_layout(
            f,
            CollectionSubject::Set,
            elts.len(),
            collections::has_comments(f, item.range()),
            collections::magic_comma(f, elts.last().map_or(item.end(), Ranged::end), item.end()),
        );
        let source_is_multiline = f.context().source()[item.range()].contains('\n');

        match collection_layout {
            CollectionDecision::ForceExpanded => {
                write!(f, [token("{"), block_indent(&joined), token("}")])
            }
            CollectionDecision::PreferCompact => collections::packed(
                f,
                "{",
                "}",
                elts.iter()
                    .map(|element| Element(element, CollectionSubject::Set)),
                false,
                false,
            ),
            CollectionDecision::CompactExpanded | CollectionDecision::Fill
                if source_is_multiline
                    || collection_layout == CollectionDecision::CompactExpanded =>
            {
                collections::packed(
                    f,
                    "{",
                    "}",
                    elts.iter()
                        .map(|element| Element(element, CollectionSubject::Set)),
                    true,
                    false,
                )
            }
            CollectionDecision::UseRuffDefault
            | CollectionDecision::Fill
            | CollectionDecision::CompactExpanded
            | CollectionDecision::PreserveInput => parenthesized("{", &joined, "}")
                .with_dangling_comments(dangling)
                .fmt(f),
        }
    }
}

impl NeedsParentheses for ExprSet {
    fn needs_parentheses(
        &self,
        _parent: AnyNodeRef,
        _context: &PyFormatContext,
    ) -> OptionalParentheses {
        OptionalParentheses::Never
    }
}
