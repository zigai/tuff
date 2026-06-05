use ruff_formatter::write;
use ruff_python_ast::Keyword;

use crate::custom::hooks;
use crate::prelude::*;

#[derive(Default)]
pub struct FormatKeyword;

impl FormatNodeRule<Keyword> for FormatKeyword {
    fn fmt_fields(&self, item: &Keyword, f: &mut PyFormatter) -> FormatResult<()> {
        let Keyword {
            range: _,
            node_index: _,
            arg,
            value,
        } = item;
        // Comments after the `=` or `**` are reassigned as leading comments on the value.
        if let Some(arg) = arg {
            write!(
                f,
                [
                    arg.format(),
                    format_with(|f| hooks::keyword_separator(f, item)),
                    value.format()
                ]
            )
        } else {
            write!(f, [token("**"), value.format()])
        }
    }
}
