use ruff_formatter::{FormatRuleWithOptions, format_args, write};
use ruff_python_ast::AnyNodeRef;
use ruff_python_ast::ExprTuple;
use ruff_text_size::{Ranged, TextRange};

use crate::builders::parenthesize_if_expands;
use crate::custom::collections::{self, Element};
use crate::custom::hooks::{self, CollectionDecision, CollectionSubject};
use crate::expression::parentheses::{
    NeedsParentheses, OptionalParentheses, empty_parenthesized, optional_parentheses, parenthesized,
};
use crate::other::commas::has_trailing_comma;
use crate::prelude::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum TupleParentheses {
    /// By default tuples with a single element will include parentheses. Tuples with multiple elements
    /// will parenthesize if the expression expands. This means that tuples will often *preserve*
    /// their parentheses, but this differs from `Preserve` in that we may also *introduce*
    /// parentheses as well.
    #[default]
    Default,

    /// Handle special cases where parentheses are to be preserved.
    ///
    /// Black omits parentheses for tuples inside subscripts except if the tuple is already
    /// parenthesized in the source code.
    /// ```python
    /// x[a, :]
    /// x[a, b:]
    /// x[(a, b):]
    /// ```
    Preserve,

    /// The same as [`Self::Default`] except that it uses [`optional_parentheses`] rather than
    /// [`parenthesize_if_expands`]. This avoids adding parentheses if breaking any containing parenthesized
    /// expression makes the tuple fit.
    ///
    /// Avoids adding parentheses around the tuple because breaking the `sum` call expression is sufficient
    /// to make it fit.
    ///
    /// ```python
    /// return len(self.nodeseeeeeeeee), sum(
    ///     len(node.parents) for node in self.node_map.values()
    /// )
    /// ```
    OptionalParentheses,

    /// Handle the special cases where we don't include parentheses at all.
    ///
    /// Black never formats tuple targets of for loops with parentheses if inside a comprehension.
    /// For example, tuple targets will always be formatted on the same line, except when an element supports
    /// line-breaking in an un-parenthesized context.
    /// ```python
    /// # Input
    /// {k: v for x, (k, v) in this_is_a_very_long_variable_which_will_cause_a_trailing_comma_which_breaks_the_comprehension}
    ///
    /// # Black
    /// {
    ///     k: v
    ///     for x, (
    ///         k,
    ///         v,
    ///     ) in this_is_a_very_long_variable_which_will_cause_a_trailing_comma_which_breaks_the_comprehension
    /// }
    /// ```
    Never,

    /// Handle the special cases where we don't include parentheses if they are not required.
    ///
    /// Normally, black keeps parentheses, but in the case of for loops it formats
    /// ```python
    /// for (a, b) in x:
    ///     pass
    /// ```
    /// to
    /// ```python
    /// for a, b in x:
    ///     pass
    /// ```
    /// Black still does use parentheses in these positions if the group breaks or magic trailing
    /// comma is used.
    ///
    /// Additional examples:
    /// ```python
    /// for (a,) in []:
    /// pass
    /// for a, b in []:
    ///     pass
    /// for a, b in []:  # Strips parentheses
    ///     pass
    /// for (
    ///     aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,
    ///     b,
    /// ) in []:
    ///     pass
    /// ```
    NeverPreserve,
}

#[derive(Default)]
pub struct FormatExprTuple {
    parentheses: TupleParentheses,
}

impl FormatRuleWithOptions<ExprTuple, PyFormatContext<'_>> for FormatExprTuple {
    type Options = TupleParentheses;

    fn with_options(mut self, options: Self::Options) -> Self {
        self.parentheses = options;
        self
    }
}

impl FormatNodeRule<ExprTuple> for FormatExprTuple {
    fn fmt_fields(&self, item: &ExprTuple, f: &mut PyFormatter) -> FormatResult<()> {
        let ExprTuple {
            elts,
            ctx: _,
            range: _,
            node_index: _,
            parenthesized: is_parenthesized,
        } = item;

        let comments = f.context().comments().clone();
        let dangling = comments.dangling(item);

        // Handle the edge cases of an empty tuple and a tuple with one element
        //
        // there can be dangling comments, and they can be in two
        // positions:
        // ```python
        // a3 = (  # end-of-line
        //     # own line
        // )
        // ```
        // In all other cases comments get assigned to a list element
        match elts.as_slice() {
            [] => empty_parenthesized("(", dangling, ")").fmt(f),
            [single] => match self.parentheses {
                TupleParentheses::Preserve if !is_parenthesized => {
                    collections::expression(f, single, CollectionSubject::Tuple)?;
                    // The `TupleParentheses::Preserve` is only set by subscript expression
                    // formatting. With PEP 646, a single element starred expression in the slice
                    // position of a subscript expression is actually a tuple expression. For
                    // example:
                    //
                    // ```python
                    // data[*x]
                    // #    ^^ single element tuple expression without a trailing comma
                    //
                    // data[*x,]
                    // #    ^^^ single element tuple expression with a trailing comma
                    // ```
                    //
                    //
                    // This means that the formatter should only add a trailing comma if there is
                    // one already.
                    if has_trailing_comma(TextRange::new(single.end(), item.end()), f.context()) {
                        token(",").fmt(f)?;
                    }
                    Ok(())
                }
                _ =>
                // A single element tuple always needs parentheses and a trailing comma, except when inside of a subscript
                {
                    parenthesized(
                        "(",
                        &format_args![
                            format_with(|f| collections::expression(
                                f,
                                single,
                                CollectionSubject::Tuple
                            )),
                            token(",")
                        ],
                        ")",
                    )
                    .with_dangling_comments(dangling)
                    .fmt(f)
                }
            },
            _ if should_force_expand_tuple(item, self.parentheses, *is_parenthesized, f) => {
                write!(
                    f,
                    [
                        token("("),
                        block_indent(&ExprSequence::new(item)),
                        token(")")
                    ]
                )
            }
            _ if should_fill_tuple(item, self.parentheses, *is_parenthesized, f) => {
                let layout = tuple_collection_layout(item, f);
                let optional = matches!(
                    layout,
                    CollectionDecision::PreferCompact | CollectionDecision::CompactExpanded
                ) && (!*is_parenthesized
                    || self.parentheses == TupleParentheses::NeverPreserve);
                collections::packed(
                    f,
                    "(",
                    ")",
                    elts.iter()
                        .map(|element| Element(element, CollectionSubject::Tuple)),
                    matches!(
                        layout,
                        CollectionDecision::Fill | CollectionDecision::CompactExpanded
                    ),
                    optional,
                )
            }
            // If the tuple has parentheses, we generally want to keep them. The exception are for
            // loops, see `TupleParentheses::NeverPreserve` doc comment.
            //
            // Unlike other expression parentheses, tuple parentheses are part of the range of the
            // tuple itself.
            _ if *is_parenthesized
                && !(self.parentheses == TupleParentheses::NeverPreserve
                    && dangling.is_empty()) =>
            {
                parenthesized("(", &ExprSequence::new(item), ")")
                    .with_dangling_comments(dangling)
                    .fmt(f)
            }
            _ => match self.parentheses {
                TupleParentheses::Never => {
                    let separator =
                        format_with(|f| group(&format_args![token(","), space()]).fmt(f));
                    f.join_with(separator)
                        .entries(elts.iter().formatted())
                        .finish()
                }
                TupleParentheses::Preserve => group(&ExprSequence::new(item)).fmt(f),
                TupleParentheses::NeverPreserve => {
                    optional_parentheses(&ExprSequence::new(item)).fmt(f)
                }
                TupleParentheses::OptionalParentheses if item.len() == 2 => {
                    optional_parentheses(&ExprSequence::new(item)).fmt(f)
                }
                TupleParentheses::Default | TupleParentheses::OptionalParentheses => {
                    parenthesize_if_expands(&ExprSequence::new(item)).fmt(f)
                }
            },
        }
    }
}

fn should_force_expand_tuple(
    item: &ExprTuple,
    parentheses: TupleParentheses,
    is_parenthesized: bool,
    f: &PyFormatter,
) -> bool {
    if !is_parenthesized
        && !matches!(
            parentheses,
            TupleParentheses::Default | TupleParentheses::OptionalParentheses
        )
    {
        return false;
    }

    tuple_collection_layout(item, f) == CollectionDecision::ForceExpanded
}

fn should_fill_tuple(
    item: &ExprTuple,
    parentheses: TupleParentheses,
    is_parenthesized: bool,
    f: &PyFormatter,
) -> bool {
    let layout = tuple_collection_layout(item, f);
    // For-loop targets may acquire parentheses when they overflow. Pack them on
    // the first pass too, but leave the existing Fill and subscript/comprehension
    // rules unchanged.
    let compact_for_target = parentheses == TupleParentheses::NeverPreserve
        && matches!(
            layout,
            CollectionDecision::PreferCompact | CollectionDecision::CompactExpanded
        );
    if !is_parenthesized
        && !matches!(
            parentheses,
            TupleParentheses::Default | TupleParentheses::OptionalParentheses
        )
        && !compact_for_target
    {
        return false;
    }

    match layout {
        CollectionDecision::PreferCompact | CollectionDecision::CompactExpanded => true,
        CollectionDecision::Fill => f.context().source()[item.range()].contains('\n'),
        _ => false,
    }
}

fn tuple_collection_layout(item: &ExprTuple, f: &PyFormatter) -> CollectionDecision {
    hooks::collection_layout(
        f,
        CollectionSubject::Tuple,
        item.elts.len(),
        collections::has_comments(f, item.range()),
        collections::magic_comma(
            f,
            item.elts.last().map_or(item.end(), Ranged::end),
            item.end(),
        ),
    )
}

#[derive(Debug)]
struct ExprSequence<'a> {
    tuple: &'a ExprTuple,
}

impl<'a> ExprSequence<'a> {
    const fn new(expr: &'a ExprTuple) -> Self {
        Self { tuple: expr }
    }
}

impl Format<PyFormatContext<'_>> for ExprSequence<'_> {
    fn fmt(&self, f: &mut PyFormatter) -> FormatResult<()> {
        let end = collections::sequence_end(
            f,
            CollectionSubject::Tuple,
            self.tuple.elts.last().map_or(self.tuple.end(), Ranged::end),
            self.tuple.end(),
        );
        f.join_comma_separated(end)
            .entries(
                self.tuple
                    .elts
                    .iter()
                    .map(|element| (element, Element(element, CollectionSubject::Tuple))),
            )
            .finish()
    }
}

impl NeedsParentheses for ExprTuple {
    fn needs_parentheses(
        &self,
        _parent: AnyNodeRef,
        _context: &PyFormatContext,
    ) -> OptionalParentheses {
        OptionalParentheses::Never
    }
}
