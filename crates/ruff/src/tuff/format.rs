use std::path::Path;

use anyhow::Result;
use ruff_notebook::Notebook;
use ruff_python_ast::PySourceType;
use ruff_text_size::TextRange;
use ruff_workspace::FormatterSettings;
use tuff_python_formatter::{
    FormatModuleError, TuffCustomOptions, format_module_source, format_range,
};

#[derive(ruff_macros::CacheKey)]
pub(crate) struct FormatCacheKey<'a> {
    custom_options: &'a TuffCustomOptions,
}

impl<'a> FormatCacheKey<'a> {
    pub(crate) const fn new(custom_options: &'a TuffCustomOptions) -> Self {
        Self { custom_options }
    }
}

pub(crate) fn format_python_source(
    source: &str,
    source_type: PySourceType,
    settings: &FormatterSettings,
    path: Option<&Path>,
    custom_options: TuffCustomOptions,
) -> Result<String, FormatModuleError> {
    let options = settings
        .to_tuff_format_options(source_type, source, path)
        .with_custom(custom_options);

    format_module_source(source, options).map(ruff_formatter::Printed::into_code)
}

pub(crate) struct FormattedRange {
    source_range: TextRange,
    code: String,
}

impl FormattedRange {
    pub(crate) const fn source_range(&self) -> TextRange {
        self.source_range
    }

    pub(crate) fn code(&self) -> &str {
        &self.code
    }
}

pub(crate) fn format_python_range(
    source: &str,
    range: TextRange,
    source_type: PySourceType,
    settings: &FormatterSettings,
    path: Option<&Path>,
    custom_options: TuffCustomOptions,
) -> Result<FormattedRange, FormatModuleError> {
    let options = settings
        .to_tuff_format_options(source_type, source, path)
        .with_custom(custom_options);

    format_range(source, range, options).map(|formatted| FormattedRange {
        source_range: formatted.source_range(),
        code: formatted.into_code(),
    })
}

pub(crate) fn format_notebook_cell(
    notebook: &Notebook,
    cell_range: TextRange,
    settings: &FormatterSettings,
    path: Option<&Path>,
    custom_options: TuffCustomOptions,
) -> Result<String, FormatModuleError> {
    let source = notebook.source_code();
    let options = settings
        .to_tuff_format_options(PySourceType::Ipynb, source, path)
        .with_custom(custom_options);

    format_module_source(&source[cell_range], options).map(ruff_formatter::Printed::into_code)
}
