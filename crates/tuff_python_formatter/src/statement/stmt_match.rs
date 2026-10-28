use ruff_formatter::{format_args, write};
use ruff_python_ast::StmtMatch;

use crate::comments::leading_alternate_branch_comments;
use crate::context::{NodeLevel, WithNodeLevel};
use crate::custom::blank_lines;
use crate::expression::maybe_parenthesize_expression;
use crate::expression::parentheses::Parenthesize;
use crate::prelude::*;
use crate::statement::clause::{ClauseHeader, clause_header};
use ruff_text_size::Ranged;
use tuff_lint::builtin::spacing::GapKind;

#[derive(Default)]
pub struct FormatStmtMatch;

impl FormatNodeRule<StmtMatch> for FormatStmtMatch {
    fn fmt_fields(&self, item: &StmtMatch, f: &mut PyFormatter) -> FormatResult<()> {
        let StmtMatch {
            range: _,
            node_index: _,
            subject,
            cases,
        } = item;

        let comments = f.context().comments().clone();
        let dangling_item_comments = comments.dangling(item);

        // There can be at most one dangling comment after the colon in a match statement.
        debug_assert!(dangling_item_comments.len() <= 1);

        clause_header(
            ClauseHeader::Match(item),
            dangling_item_comments,
            &format_args![
                token("match"),
                space(),
                maybe_parenthesize_expression(subject, item, Parenthesize::IfBreaks),
            ],
        )
        .fmt(f)?;

        let mut cases_iter = cases.iter();
        let Some(first) = cases_iter.next() else {
            return Ok(());
        };

        // The new level is for the `case` nodes.
        let mut f = WithNodeLevel::new(NodeLevel::CompoundStatement, f);

        write!(f, [block_indent(&first.format())])?;
        let mut last_case = first;

        for case in cases_iter {
            let planned = f.context().custom_layout().blank_lines.between(
                GapKind::BetweenCases,
                last_case.start(),
                case.start(),
            );
            let separator = format_with(|f| {
                if let Some(blanks) = planned {
                    // Replace, rather than prepend to, source whitespace preservation.
                    blank_lines::separator(blanks, f)
                } else {
                    leading_alternate_branch_comments(comments.leading(case), last_case.body.last())
                        .fmt(f)
                }
            });
            let last_suite_in_statement = Some(case) == cases.last();
            write!(
                f,
                [block_indent(&format_args!(
                    separator,
                    case.format().with_options(last_suite_in_statement)
                ))]
            )?;
            last_case = case;
        }

        Ok(())
    }
}
