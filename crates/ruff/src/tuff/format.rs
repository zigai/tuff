use std::path::Path;

use anyhow::Result;
use ruff_linter::source_kind::SourceKind;
use ruff_notebook::Notebook;
use ruff_python_ast::{PySourceType, SourceType};
use ruff_python_formatter::{
    DocstringCode as RuffDocstringCode, DocstringCodeLineWidth as RuffDocstringCodeLineWidth,
    QuoteStyle,
};
use ruff_source_file::find_newline;
use ruff_text_size::TextRange;
use ruff_workspace::FormatterSettings;
use tuff_python_formatter::{
    DocstringCode, DocstringCodeLineWidth, FormatModuleError, MagicTrailingComma,
    NestedStringQuoteStyle, PreviewMode, PyFormatOptions as TuffFormatOptions,
    QuoteStyle as TuffQuoteStyle, TuffCustomOptions, format_module_source, format_range,
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

pub(crate) fn custom_options_for_source_type(
    source_type: SourceType,
    path: &Path,
) -> Result<TuffCustomOptions> {
    if source_type.is_python() {
        crate::tuff::config::custom_options(Some(path))
    } else {
        Ok(TuffCustomOptions::default())
    }
}

pub(crate) fn custom_options_for_source_kind(
    source_kind: &SourceKind,
    path: Option<&Path>,
) -> Result<TuffCustomOptions> {
    match source_kind {
        SourceKind::Python { .. } => crate::tuff::config::custom_options(path),
        SourceKind::IpyNotebook(notebook) if notebook.is_python_notebook() => {
            crate::tuff::config::custom_options(path)
        }
        SourceKind::IpyNotebook(_) | SourceKind::Markdown(_) => Ok(TuffCustomOptions::default()),
    }
}

pub(crate) fn format_python_source(
    source: &str,
    source_type: PySourceType,
    settings: &FormatterSettings,
    path: Option<&Path>,
    custom_options: TuffCustomOptions,
) -> Result<String, FormatModuleError> {
    let options = format_options(settings, source_type, source, path, custom_options);

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
    let options = format_options(settings, source_type, source, path, custom_options);

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
    let options = format_options(settings, PySourceType::Ipynb, source, path, custom_options);

    format_module_source(&source[cell_range], options).map(ruff_formatter::Printed::into_code)
}

fn format_options(
    settings: &FormatterSettings,
    source_type: PySourceType,
    source: &str,
    path: Option<&Path>,
    custom_options: TuffCustomOptions,
) -> TuffFormatOptions {
    let target_version = path
        .map(|path| settings.resolve_target_version(path))
        .unwrap_or(settings.unresolved_target_version);

    let line_ending = match settings.line_ending.to_string().as_str() {
        "lf" => ruff_formatter::printer::LineEnding::LineFeed,
        "crlf" => ruff_formatter::printer::LineEnding::CarriageReturnLineFeed,
        "native" => {
            #[cfg(target_os = "windows")]
            {
                ruff_formatter::printer::LineEnding::CarriageReturnLineFeed
            }
            #[cfg(not(target_os = "windows"))]
            {
                ruff_formatter::printer::LineEnding::LineFeed
            }
        }
        "auto" => match find_newline(source) {
            Some((_, ruff_source_file::LineEnding::Lf)) => {
                ruff_formatter::printer::LineEnding::LineFeed
            }
            Some((_, ruff_source_file::LineEnding::CrLf)) => {
                ruff_formatter::printer::LineEnding::CarriageReturnLineFeed
            }
            Some((_, ruff_source_file::LineEnding::Cr)) => {
                ruff_formatter::printer::LineEnding::CarriageReturn
            }
            None => ruff_formatter::printer::LineEnding::LineFeed,
        },
        _ => ruff_formatter::printer::LineEnding::LineFeed,
    };

    let quote_style = match settings.quote_style {
        QuoteStyle::Single => TuffQuoteStyle::Single,
        QuoteStyle::Double => TuffQuoteStyle::Double,
        QuoteStyle::Preserve => TuffQuoteStyle::Preserve,
    };
    let magic_trailing_comma = match settings.magic_trailing_comma.to_string().as_str() {
        "ignore" => MagicTrailingComma::Ignore,
        _ => MagicTrailingComma::Respect,
    };
    let preview = if settings.preview.is_enabled() {
        PreviewMode::Enabled
    } else {
        PreviewMode::Disabled
    };
    let nested_string_quote_style = match settings.nested_string_quote_style.to_string().as_str() {
        "preferred" => NestedStringQuoteStyle::Preferred,
        _ => NestedStringQuoteStyle::Alternating,
    };
    let docstring_code = match settings.docstring_code_format {
        RuffDocstringCode::Enabled => DocstringCode::Enabled,
        RuffDocstringCode::Disabled => DocstringCode::Disabled,
    };
    let docstring_code_line_width = match settings.docstring_code_line_width {
        RuffDocstringCodeLineWidth::Fixed(line_width) => DocstringCodeLineWidth::Fixed(line_width),
        RuffDocstringCodeLineWidth::Dynamic => DocstringCodeLineWidth::Dynamic,
    };

    TuffFormatOptions::from_source_type(source_type)
        .with_target_version(target_version)
        .with_indent_style(settings.indent_style)
        .with_indent_width(settings.indent_width)
        .with_quote_style(quote_style)
        .with_nested_string_quote_style(nested_string_quote_style)
        .with_magic_trailing_comma(magic_trailing_comma)
        .with_preview(preview)
        .with_line_ending(line_ending)
        .with_line_width(settings.line_width)
        .with_docstring_code(docstring_code)
        .with_docstring_code_line_width(docstring_code_line_width)
        .with_custom(custom_options)
}
