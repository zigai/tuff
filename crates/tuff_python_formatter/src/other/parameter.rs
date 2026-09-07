use crate::custom::hooks::{self, AnnotationSubject};
use crate::prelude::*;
use ruff_python_ast::Parameter;

#[derive(Default)]
pub struct FormatParameter;

impl FormatNodeRule<Parameter> for FormatParameter {
    fn fmt_fields(&self, item: &Parameter, f: &mut PyFormatter) -> FormatResult<()> {
        let Parameter {
            range: _,
            node_index: _,
            name,
            annotation,
        } = item;

        name.format().fmt(f)?;

        if let Some(annotation) = annotation.as_deref() {
            token(":").fmt(f)?;
            hooks::after_annotation_colon(f, AnnotationSubject::FunctionParameter(item))?;

            if f.context().comments().has_leading(annotation)
                && !f.context().is_expression_parenthesized(annotation.into())
            {
                hard_line_break().fmt(f)?;
            } else {
                space().fmt(f)?;
            }

            annotation.format().fmt(f)?;
        }

        Ok(())
    }
}
