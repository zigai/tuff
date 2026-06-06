use ruff_formatter::prelude::{format_args, format_with};
use ruff_formatter::write;
use ruff_python_ast::AnyNodeRef;
use ruff_python_ast::ExprList;
use ruff_text_size::Ranged;

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
            f.join_comma_separated(item.end())
                .entries(
                    elts.iter()
                        .map(|element| (element, CollectionElement(element))),
                )
                .finish()
        });

        let collection_layout = hooks::collection_layout(
            f,
            CollectionSubject::List,
            elts.len(),
            !dangling.is_empty(),
            false,
        );
        let source_is_multiline = f.context().source()[item.range()].contains('\n');

        match collection_layout {
            CollectionDecision::ForceExpanded => {
                write!(f, [token("["), block_indent(&items), token("]")])
            }
            CollectionDecision::Fill if source_is_multiline => write!(
                f,
                [
                    token("["),
                    block_indent(&format_with(|f| {
                        let mut fill = f.fill();
                        for element in elts {
                            fill.entry(
                                &format_args![token(","), soft_line_break_or_space()],
                                &CollectionElement(element),
                            );
                        }
                        fill.finish()?;
                        token(",").fmt(f)
                    })),
                    token("]")
                ]
            ),
            CollectionDecision::UseRuffDefault
            | CollectionDecision::PreferFlat
            | CollectionDecision::Fill
            | CollectionDecision::PreserveInput => parenthesized("[", &items, "]")
                .with_dangling_comments(dangling)
                .fmt(f),
        }
    }
}

struct CollectionElement<'a>(&'a ruff_python_ast::Expr);

impl Format<PyFormatContext<'_>> for CollectionElement<'_> {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        hooks::before_collection_element(f, self.0)?;
        self.0.format().fmt(f)
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
