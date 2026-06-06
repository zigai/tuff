use ruff_formatter::{format_args, write};
use ruff_python_ast::{AnyNodeRef, DictItem, Expr, ExprDict};
use ruff_text_size::{Ranged, TextRange};

use crate::comments::{SourceComment, dangling_comments, leading_comments};
use crate::custom::hooks::{self, CollectionDecision, CollectionSubject};
use crate::expression::parentheses::{
    NeedsParentheses, OptionalParentheses, empty_parenthesized, parenthesized,
};
use crate::prelude::*;

#[derive(Default)]
pub struct FormatExprDict;

impl FormatNodeRule<ExprDict> for FormatExprDict {
    fn fmt_fields(&self, item: &ExprDict, f: &mut PyFormatter) -> FormatResult<()> {
        let ExprDict {
            range: _,
            node_index: _,
            items,
        } = item;

        let comments = f.context().comments().clone();
        let dangling = comments.dangling(item);

        let Some(first_dict_item) = items.first() else {
            return empty_parenthesized("{", dangling, "}").fmt(f);
        };

        // Dangling comments can either appear after the open bracket, or around the key-value
        // pairs:
        // ```python
        // {  # open_parenthesis_comments
        //     x:  # key_value_comments
        //     y
        // }
        // ```
        let (open_parenthesis_comments, key_value_comments) =
            dangling.split_at(dangling.partition_point(|comment| {
                comment.end() < KeyValuePair::new(first_dict_item).start()
            }));

        let format_pairs = format_with(|f| {
            let mut joiner = f.join_comma_separated(item.end());

            let mut key_value_comments = key_value_comments;
            for dict_item in items {
                let mut key_value_pair = KeyValuePair::new(dict_item);

                let partition = key_value_comments
                    .partition_point(|comment| comment.start() < key_value_pair.end());
                key_value_pair = key_value_pair.with_comments(&key_value_comments[..partition]);
                key_value_comments = &key_value_comments[partition..];

                joiner.entry(&key_value_pair, &key_value_pair);
            }

            joiner.finish()
        });

        let collection_layout = hooks::collection_layout(
            f,
            CollectionSubject::Dict,
            items.len(),
            !open_parenthesis_comments.is_empty() || !key_value_comments.is_empty(),
            false,
        );
        let source_is_multiline = f.context().source()[item.range()].contains('\n');

        match collection_layout {
            CollectionDecision::ForceExpanded => {
                write!(f, [token("{"), block_indent(&format_pairs), token("}")])
            }
            CollectionDecision::Fill if source_is_multiline => write!(
                f,
                [
                    token("{"),
                    block_indent(&format_with(|f| {
                        let mut fill = f.fill();
                        for dict_item in items {
                            fill.entry(
                                &format_args![token(","), soft_line_break_or_space()],
                                &KeyValuePair::new(dict_item),
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
            | CollectionDecision::PreserveInput => parenthesized("{", &format_pairs, "}")
                .with_dangling_comments(open_parenthesis_comments)
                .fmt(f),
        }
    }
}

impl NeedsParentheses for ExprDict {
    fn needs_parentheses(
        &self,
        _parent: AnyNodeRef,
        _context: &PyFormatContext,
    ) -> OptionalParentheses {
        OptionalParentheses::Never
    }
}

#[derive(Debug)]
struct KeyValuePair<'a> {
    item: &'a DictItem,
    key: &'a Option<Expr>,
    value: &'a Expr,
    comments: &'a [SourceComment],
}

impl<'a> KeyValuePair<'a> {
    fn new(item: &'a DictItem) -> Self {
        Self {
            item,
            key: &item.key,
            value: &item.value,
            comments: &[],
        }
    }

    fn with_comments(self, comments: &'a [SourceComment]) -> Self {
        Self { comments, ..self }
    }
}

impl Ranged for KeyValuePair<'_> {
    fn range(&self) -> TextRange {
        if let Some(key) = self.key {
            TextRange::new(key.start(), self.value.end())
        } else {
            self.value.range()
        }
    }
}

impl Format<PyFormatContext<'_>> for KeyValuePair<'_> {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        if let Some(key) = self.key {
            write!(
                f,
                [group(&format_with(|f| {
                    key.format().fmt(f)?;

                    if self.comments.is_empty() {
                        hooks::dict_key_value_separator(f, self.item)?;
                    } else {
                        token(":").fmt(f)?;
                        dangling_comments(self.comments).fmt(f)?;
                    }

                    self.value.format().fmt(f)
                }))]
            )
        } else {
            // TODO(charlie): Make these dangling comments on the `ExprDict`, and identify them
            // dynamically, so as to avoid the parent rendering its child's comments.
            let comments = f.context().comments().clone();
            let leading_value_comments = comments.leading(self.value);
            write!(
                f,
                [
                    // make sure the leading comments are hoisted past the `**`
                    leading_comments(leading_value_comments),
                    group(&format_args![token("**"), self.value.format()])
                ]
            )
        }
    }
}
