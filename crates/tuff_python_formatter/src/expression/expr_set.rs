use ruff_formatter::{format_args, write};
use ruff_python_ast::AnyNodeRef;
use ruff_python_ast::ExprSet;
use ruff_text_size::Ranged;

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
            f.join_comma_separated(item.end())
                .entries(
                    elts.iter()
                        .map(|element| (element, CollectionElement(element))),
                )
                .finish()
        });

        let comments = f.context().comments().clone();
        let dangling = comments.dangling(item);

        let collection_layout = hooks::collection_layout(
            f,
            CollectionSubject::Set,
            elts.len(),
            !dangling.is_empty(),
            false,
        );
        let source_is_multiline = f.context().source()[item.range()].contains('\n');

        match collection_layout {
            CollectionDecision::ForceExpanded => {
                write!(f, [token("{"), block_indent(&joined), token("}")])
            }
            CollectionDecision::Fill if source_is_multiline => write!(
                f,
                [
                    token("{"),
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
                    token("}")
                ]
            ),
            CollectionDecision::UseRuffDefault
            | CollectionDecision::PreferFlat
            | CollectionDecision::Fill
            | CollectionDecision::PreserveInput => parenthesized("{", &joined, "}")
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

impl NeedsParentheses for ExprSet {
    fn needs_parentheses(
        &self,
        _parent: AnyNodeRef,
        _context: &PyFormatContext,
    ) -> OptionalParentheses {
        OptionalParentheses::Never
    }
}
