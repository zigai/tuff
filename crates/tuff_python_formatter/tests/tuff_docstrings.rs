use tuff_python_formatter::{
    CollectionLayout, DocstringCode, FeatureMode, OneLineSuiteClause, OneLineSuiteStatement,
    PyFormatOptions, TuffCustomOptions, format_module_source,
};

fn docstring_args_options() -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.docstrings.args_sections = FeatureMode::Enabled;
    PyFormatOptions::default().with_custom(custom)
}

fn format(source: &str, options: PyFormatOptions) -> String {
    format_module_source(source, options).unwrap().into_code()
}

#[test]
fn default_keeps_google_args_section_spacing() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short: Short value.
        credentials_path: Credentials file.
    """
"#;

    assert_eq!(format(source, PyFormatOptions::default()), source);
}

#[test]
fn aligns_google_args_sections() {
    let source = r#"def upload(short, credentials_path, metadata):
    """Upload a video.

    Args:
        short: Short value.
        credentials_path: Credentials file.
        metadata (dict[str, str]): Metadata values.

    Returns:
        None.
    """
"#;

    let expected = r#"def upload(short, credentials_path, metadata):
    """Upload a video.

    Args:
        short:                     Short value.
        credentials_path:          Credentials file.
        metadata (dict[str, str]): Metadata values.

    Returns:
        None.
    """
"#;

    let options = docstring_args_options();
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_keyword_args_sections() {
    let source = r#"def upload(**kwargs):
    """Upload a video.

    Keyword Args:
        timeout: Timeout in seconds.
        upstream_http_timeout_s: Upstream timeout.
    """
"#;

    let expected = r#"def upload(**kwargs):
    """Upload a video.

    Keyword Args:
        timeout:                 Timeout in seconds.
        upstream_http_timeout_s: Upstream timeout.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn aligns_starred_google_args() {
    let source = r#"def upload(*args, **kwargs):
    """Upload a video.

    Args:
        *args: Positional arguments.
        **kwargs: Keyword arguments.
    """
"#;

    let expected = r#"def upload(*args, **kwargs):
    """Upload a video.

    Args:
        *args:    Positional arguments.
        **kwargs: Keyword arguments.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn aligns_other_args_sections() {
    let source = r#"def upload(*args):
    """Upload a video.

    Other Args:
        short: Short value.
        credentials_path: Credentials file.
    """
"#;

    let expected = r#"def upload(*args):
    """Upload a video.

    Other Args:
        short:            Short value.
        credentials_path: Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_stops_at_blank_line() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short: Short value.
        credentials_path: Credentials file.

        This paragraph is not an argument.
        url: https://example.com
    """
"#;

    let expected = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short:            Short value.
        credentials_path: Credentials file.

        This paragraph is not an argument.
        url: https://example.com
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_stops_at_non_entry_paragraph() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        These values come from the service request.
        short: Short value.
        credentials_path: Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), source);
}

#[test]
fn aligns_google_args_with_continuation_lines() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short: Short value.
            Continued details with a colon: yes.
        credentials_path: Credentials file.
    """
"#;

    let expected = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short:            Short value.
            Continued details with a colon: yes.
        credentials_path: Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_skips_restructured_text_params() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        :param short: Short value.
        credentials_path: Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), source);
}

#[test]
fn aligns_google_args_by_display_width() {
    let source = r#"def upload(x, 動画):
    """Upload a video.

    Args:
        x: Latin name.
        動画: Wide name.
    """
"#;

    let expected = r#"def upload(x, 動画):
    """Upload a video.

    Args:
        x:    Latin name.
        動画: Wide name.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn aligns_tab_indented_google_args_sections() {
    let source = "def upload(short, credentials_path):\n\t\"\"\"Upload a video.\n\n\tArgs:\n\t\tshort: Short value.\n\t\tcredentials_path: Credentials file.\n\t\"\"\"\n";

    let expected = "def upload(short, credentials_path):\n    \"\"\"Upload a video.\n\n    Args:\n            short:            Short value.\n            credentials_path: Credentials file.\n    \"\"\"\n";

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_ignores_colons_inside_types() {
    let source = r#"def upload(mode, credentials_path):
    """Upload a video.

    Args:
        mode (Literal["fast:unsafe"]): Upload mode.
        credentials_path: Credentials file.
    """
"#;

    let expected = r#"def upload(mode, credentials_path):
    """Upload a video.

    Args:
        mode (Literal["fast:unsafe"]): Upload mode.
        credentials_path:              Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_ignores_colons_inside_nested_types() {
    let source = r#"def upload(headers, credentials_path):
    """Upload a video.

    Args:
        headers (dict[str, Literal["x:y"]]): Request headers.
        credentials_path: Credentials file.
    """
"#;

    let expected = r#"def upload(headers, credentials_path):
    """Upload a video.

    Args:
        headers (dict[str, Literal["x:y"]]): Request headers.
        credentials_path:                    Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_skips_double_colon_lines() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short:: Short value.
        credentials_path: Credentials file.
    """
"#;

    assert_eq!(format(source, docstring_args_options()), source);
}

#[test]
fn docstring_args_alignment_skips_markdown_code_fences() {
    let source = r#"def example():
    """Show generated code.

    ```python
    def generated(short, credentials_path):
        '''Generated docstring.

        Args:
            short: Short value.
            credentials_path: Credentials file.
        '''
    ```
    """
"#;

    assert_eq!(format(source, docstring_args_options()), source);
}

#[test]
fn docstring_args_alignment_resumes_after_markdown_code_fence() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    ```text
    Args:
        short: Leave this example alone.
        credentials_path: Leave this example alone.
    ```

    Args:
        short: Short value.
        credentials_path: Credentials file.
    """
"#;

    let expected = r#"def upload(short, credentials_path):
    """Upload a video.

    ```text
    Args:
        short: Leave this example alone.
        credentials_path: Leave this example alone.
    ```

    Args:
        short:            Short value.
        credentials_path: Credentials file.
    """
"#;

    let options = docstring_args_options();
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn docstring_args_alignment_skips_rst_literal_blocks() {
    let source = r#"def example():
    """Show generated code::

        def generated(short, credentials_path):
            '''Generated docstring.

            Args:
                short: Short value.
                credentials_path: Credentials file.
            '''
    """
"#;

    let expected = r#"def example():
    """Show generated code::

    def generated(short, credentials_path):
        '''Generated docstring.

        Args:
            short: Short value.
            credentials_path: Credentials file.
        '''
    """
"#;

    assert_eq!(format(source, docstring_args_options()), expected);
}

#[test]
fn docstring_args_alignment_can_run_with_docstring_code_formatting() {
    let source = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short: Short value.
        credentials_path: Credentials file.

    Example:
        >>> values=[1,2]
    """
"#;

    let expected = r#"def upload(short, credentials_path):
    """Upload a video.

    Args:
        short:            Short value.
        credentials_path: Credentials file.

    Example:
        >>> values = [1, 2]
    """
"#;

    let options = docstring_args_options().with_docstring_code(DocstringCode::Enabled);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn tuff_layout_options_do_not_apply_inside_docstring_code_blocks() {
    let source = r#"def example():
    """Show generated code.

    ```python
    values = [1, 2]
    if ready:
        return
    ```
    """
"#;

    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::ForceExpanded;
    custom.one_line_suites.mode = FeatureMode::Enabled;
    custom.one_line_suites.clauses = vec![OneLineSuiteClause::If];
    custom.one_line_suites.statements = vec![OneLineSuiteStatement::Return];
    let options = PyFormatOptions::default()
        .with_docstring_code(DocstringCode::Enabled)
        .with_custom(custom);

    assert_eq!(format(source, options), source);
}
