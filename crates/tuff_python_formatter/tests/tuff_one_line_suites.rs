use ruff_formatter::LineWidth;
use tuff_python_formatter::{
    FeatureMode, OneLineSuiteClause, OneLineSuiteStatement, PyFormatOptions, TuffCustomOptions,
    format_module_source,
};

fn one_line_suite_options(
    clauses: Vec<OneLineSuiteClause>,
    statements: Vec<OneLineSuiteStatement>,
) -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.one_line_suites.mode = FeatureMode::Enabled;
    custom.one_line_suites.clauses = clauses;
    custom.one_line_suites.statements = statements;
    PyFormatOptions::default().with_custom(custom)
}

fn format(source: &str, options: PyFormatOptions) -> String {
    format_module_source(source, options).unwrap().into_code()
}

#[test]
fn default_keeps_simple_if_body_expanded() {
    let source = r#"def f(on_progress):
    if on_progress is None:
        return
"#;

    assert_eq!(format(source, PyFormatOptions::default()), source);
}

#[test]
fn collapses_allowed_return_body() {
    let source = r#"def f(on_progress):
    if on_progress is None:
        return
"#;

    let expected = r#"def f(on_progress):
    if on_progress is None: return
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Return],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn collapses_allowed_call_body() {
    let source = r#"def f(on_progress):
    if on_progress is not None:
        on_progress()
"#;

    let expected = r#"def f(on_progress):
    if on_progress is not None: on_progress()
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Call],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn statement_allowlist_is_respected() {
    let source = r#"def f(on_progress):
    if on_progress is None:
        return
    if on_progress is not None:
        on_progress()
"#;

    let expected = r#"def f(on_progress):
    if on_progress is None:
        return
    if on_progress is not None: on_progress()
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Call],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn collapses_other_configured_simple_statements() {
    let source = r#"def f(flag):
    if flag:
        value = 1
    if not flag:
        raise RuntimeError
"#;

    let expected = r#"def f(flag):
    if flag: value = 1
    if not flag: raise RuntimeError
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![
            OneLineSuiteStatement::Assignment,
            OneLineSuiteStatement::Raise,
        ],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn clause_allowlist_is_respected() {
    let source = r#"def f(condition):
    if condition:
        return
    else:
        return
"#;

    let expected = r#"def f(condition):
    if condition:
        return
    else: return
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::Else],
        vec![OneLineSuiteStatement::Return],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn collapses_configured_elif_body() {
    let source = r#"def f(value):
    if value < 0:
        return
    elif value == 0:
        return
    else:
        return
"#;

    let expected = r#"def f(value):
    if value < 0:
        return
    elif value == 0: return
    else:
        return
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::Elif],
        vec![OneLineSuiteStatement::Return],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn long_body_keeps_block_layout() {
    let source = r#"def f(on_progress, item):
    if on_progress is not None:
        on_progress(item, "a fairly long status message")
"#;

    let expected = r#"def f(on_progress, item):
    if on_progress is not None:
        on_progress(item, "a fairly long status message")
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Call],
    )
    .with_line_width(LineWidth::try_from(60).unwrap());

    assert_eq!(format(source, options), expected);
}

#[test]
fn long_header_and_body_keep_block_layout() {
    let source = r#"def f(value):
    if isinstance(value, list):
        return [redact(item) for item in value]
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Return],
    )
    .with_line_width(LineWidth::try_from(70).unwrap());

    assert_eq!(format(source, options), source);
}

#[test]
fn wide_header_uses_display_width_when_checking_line_length() {
    let source = r#"def f(動画):
    if 動画:
        return
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Return],
    )
    .with_line_width(LineWidth::try_from(18).unwrap());

    assert_eq!(format(source, options), source);
}

#[test]
fn long_existing_one_line_suite_expands_to_block() {
    let source = r#"def f(on_progress, item):
    if on_progress is not None: on_progress(item, "a fairly long status message")
"#;

    let expected = r#"def f(on_progress, item):
    if on_progress is not None:
        on_progress(item, "a fairly long status message")
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Call],
    )
    .with_line_width(LineWidth::try_from(60).unwrap());

    assert_eq!(format(source, options), expected);
}

#[test]
fn comments_keep_block_layout() {
    let source = r#"def f(on_progress):
    if on_progress is None:
        # nothing to report
        return
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::If],
        vec![OneLineSuiteStatement::Return],
    );

    assert_eq!(format(source, options), source);
}

#[test]
fn collapses_match_case_bodies() {
    let source = r#"def label(status):
    match status:
        case 200:
            return "ok"
        case 503:
            return "unavailable"
"#;

    let expected = r#"def label(status):
    match status:
        case 200: return "ok"
        case 503: return "unavailable"
"#;

    let options = one_line_suite_options(
        vec![OneLineSuiteClause::MatchCase],
        vec![OneLineSuiteStatement::Return],
    );

    assert_eq!(format(source, options), expected);
}

#[test]
fn normalizes_long_boolean_conditions() {
    let source = r#"def upload(user, service, quota):
    if user.is_active and user.has_upload_access and not service.is_draining and quota.remaining > 0:
        upload()
"#;

    let expected = r#"def upload(user, service, quota):
    if (
        user.is_active
        and user.has_upload_access
        and not service.is_draining
        and quota.remaining > 0
    ):
        upload()
"#;

    let options = PyFormatOptions::default().with_line_width(LineWidth::try_from(70).unwrap());

    assert_eq!(format(source, options), expected);
}
