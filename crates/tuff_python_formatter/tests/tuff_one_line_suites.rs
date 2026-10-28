use ruff_formatter::LineWidth;
use ruff_python_ast::PySourceType;
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

fn assert_stable(source: &str, expected: &str, options: PyFormatOptions) {
    let formatted = format(source, options.clone());
    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

fn definition_options(statements: Vec<OneLineSuiteStatement>) -> PyFormatOptions {
    one_line_suite_options(
        vec![OneLineSuiteClause::Function, OneLineSuiteClause::Class],
        statements,
    )
}

#[test]
fn compact_definitions_are_opt_in() {
    let source = "def close():\n    pass\n\n\nclass InvalidInput(ValueError):\n    pass\n";
    assert_stable(source, source, PyFormatOptions::default());
    let mut disabled = definition_options(vec![OneLineSuiteStatement::Pass])
        .custom()
        .clone();
    disabled.one_line_suites.mode = FeatureMode::Disabled;
    assert_stable(
        source,
        source,
        PyFormatOptions::default().with_custom(disabled),
    );
    assert_stable(
        source,
        source,
        one_line_suite_options(
            vec![OneLineSuiteClause::If],
            vec![OneLineSuiteStatement::Pass],
        ),
    );
    assert_stable(
        source,
        source,
        definition_options(vec![OneLineSuiteStatement::Return]),
    );
}

#[test]
fn collapses_functions_async_functions_and_classes() {
    let source = "def close():\n    pass\n\n\nasync def stop():\n    pass\n\n\nclass InvalidInput(ValueError):\n    pass\n";
    let expected =
        "def close(): pass\n\n\nasync def stop(): pass\n\n\nclass InvalidInput(ValueError): pass\n";
    assert_stable(
        source,
        expected,
        definition_options(vec![OneLineSuiteStatement::Pass]),
    );
}

#[test]
fn collapses_executable_definitions_only_when_allowed() {
    let source = "def name(self):\n    return self._name\n\n\nclass Settings(Base, metaclass=Meta):\n    enabled = True\n";
    let expected = "def name(self): return self._name\n\n\nclass Settings(Base, metaclass=Meta): enabled = True\n";
    assert_stable(
        source,
        expected,
        definition_options(vec![
            OneLineSuiteStatement::Return,
            OneLineSuiteStatement::Assignment,
        ]),
    );
}

#[test]
fn decorated_definitions_keep_decorators_on_separate_lines() {
    let source = "@cached\ndef name(self):\n    return self._name\n\n\n@register\nclass Error(ValueError):\n    pass\n";
    let expected = "@cached\ndef name(self): return self._name\n\n\n@register\nclass Error(ValueError): pass\n";
    assert_stable(
        source,
        expected,
        definition_options(vec![
            OneLineSuiteStatement::Return,
            OneLineSuiteStatement::Pass,
        ]),
    );
}

#[test]
fn definition_comments_keep_block_layout() {
    let options = definition_options(vec![
        OneLineSuiteStatement::Pass,
        OneLineSuiteStatement::Return,
    ]);
    for source in [
        "def close():  # header\n    pass\n",
        "def close():\n    # body\n    pass\n",
        "def close():\n    pass  # body\n",
        "def name(\n    self,  # parameter\n):\n    return self._name\n",
        "class Error(\n    ValueError,  # base\n):\n    pass\n",
        "def close(\n    self,\n):\n    pass\n",
        "class Error(\n    ValueError,\n):\n    pass\n",
    ] {
        // Compare comment handling to Ruff without changing the surrounding layout.
        let expected = format(source, PyFormatOptions::default());
        assert_stable(source, &expected, options.clone());
    }
}

#[test]
fn definition_width_limits_include_body_and_indentation() {
    let options = definition_options(vec![
        OneLineSuiteStatement::Pass,
        OneLineSuiteStatement::Return,
    ])
    .with_line_width(LineWidth::try_from(40).unwrap());
    for source in [
        "def retrieve_current_status(value):\n    return value\n",
        "def retrieve_current_status(\n    first_argument, second_argument\n):\n    return first_argument\n",
        "class VeryDescriptiveErrorName(\n    ValueError, AnotherException\n):\n    pass\n",
        "class Container:\n    def retrieve_current_status(self):\n        return self.status\n",
        "def get():\n    return call(first_argument, second_argument, third_argument)\n",
        "def get():\n    return [first_argument, second_argument, third_argument]\n",
    ] {
        let expected = format(
            source,
            PyFormatOptions::default().with_line_width(LineWidth::try_from(40).unwrap()),
        );
        assert_stable(source, &expected, options.clone());
    }
}

#[test]
fn short_multiline_definition_headers_can_normalize_to_one_line() {
    let source =
        "def close(\n    self\n):\n    pass\n\n\nclass Error(\n    ValueError\n):\n    pass\n";
    let expected = "def close(self): pass\n\n\nclass Error(ValueError): pass\n";
    assert_stable(
        source,
        expected,
        definition_options(vec![OneLineSuiteStatement::Pass]),
    );
    assert_stable(
        "def close(                                          self                                         ):\n    pass\n",
        "def close(self): pass\n",
        definition_options(vec![OneLineSuiteStatement::Pass]),
    );
}

#[test]
fn definition_docstrings_require_separate_opt_in() {
    let source = "def label():\n    \"\"\"Label.\"\"\"\n\n\nclass Label:\n    \"\"\"Label.\"\"\"\n";
    assert_stable(
        source,
        source,
        definition_options(vec![OneLineSuiteStatement::Expression]),
    );
}

#[test]
fn compact_docstrings_preserve_string_values() {
    let mut custom = TuffCustomOptions::default();
    custom.one_line_suites.mode = FeatureMode::Enabled;
    custom.one_line_suites.clauses = vec![OneLineSuiteClause::Function, OneLineSuiteClause::Class];
    custom.one_line_suites.docstrings = FeatureMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let source = "def label():\n    \"\"\"  Label.  \"\"\"\n\n\nclass Label:\n    r\"\"\"Label\\ntext\"\"\"\n";
    let expected =
        "def label(): \"\"\"  Label.  \"\"\"\n\n\nclass Label: r\"\"\"Label\\ntext\"\"\"\n";
    assert_stable(source, expected, options.clone());
    let multiline = "def label():\n    \"\"\"Label.\n\n    More detail.\n    \"\"\"\n";
    assert_stable(multiline, multiline, options.clone());
    let multiple = "def label():\n    \"\"\"Label.\"\"\"\n    return 1\n";
    assert_stable(multiple, multiple, options);
}

fn docstring_options() -> PyFormatOptions {
    let mut custom = definition_options(vec![]).custom().clone();
    custom.one_line_suites.docstrings = FeatureMode::Enabled;
    PyFormatOptions::default().with_custom(custom)
}

#[test]
fn multiline_docstrings_that_normalize_to_one_line_compact_stably() {
    for (source, expected) in [
        (
            "def f():\n    \"\"\"Doc.\n    \"\"\"\n",
            "def f(): \"\"\"Doc.\"\"\"\n",
        ),
        (
            "async def f():\n    \"\"\"Doc.\n    \"\"\"\n",
            "async def f(): \"\"\"Doc.\"\"\"\n",
        ),
        (
            "class C:\n    \"\"\"Doc.\n    \"\"\"\n",
            "class C: \"\"\"Doc.\"\"\"\n",
        ),
    ] {
        assert_stable(source, expected, docstring_options());
    }
}

#[test]
fn compact_docstrings_normalize_only_after_raw_candidate_exceeds_width() {
    for (header, raw_width) in [("def f():", 20), ("async def f():", 26), ("class C:", 20)] {
        let source = format!("{header}\n    \"\"\"  x  \"\"\"\n");
        let raw = format!("{header} \"\"\"  x  \"\"\"\n");
        let normalized = format!("{header} \"\"\"x\"\"\"\n");
        assert_stable(
            &source,
            &raw,
            docstring_options().with_line_width(LineWidth::try_from(raw_width).unwrap()),
        );
        assert_stable(
            &source,
            &normalized,
            docstring_options().with_line_width(LineWidth::try_from(raw_width - 1).unwrap()),
        );
    }
}

#[test]
fn implicitly_concatenated_docstrings_can_normalize_before_compacting() {
    let source = "def f():\n    (\n        \"  x\"\n        \"  \"\n    )\n";
    assert_stable(
        source,
        "def f(): \"x\"\n",
        docstring_options().with_line_width(LineWidth::try_from(15).unwrap()),
    );
}

#[test]
fn docstring_comments_and_multiline_content_keep_block_layout() {
    for source in [
        "def f():  # header\n    \"\"\"Doc.\n    \"\"\"\n",
        "async def f():\n    # body\n    \"\"\"Doc.\n    \"\"\"\n",
        "class C:\n    \"\"\"Doc.\n    \"\"\"  # body\n",
        "def f():\n    (\n        \"Doc.\"  # part\n        \"More.\"\n    )\n",
        "def f():\n    \"\"\"Summary.\n\n    More detail.\n    \"\"\"\n",
    ] {
        let expected = format(source, PyFormatOptions::default());
        assert_stable(source, &expected, docstring_options());
    }
}

#[test]
fn normalized_docstrings_that_still_exceed_width_keep_block_layout() {
    let source = "def f():\n    \"\"\"A longer summary.\n    \"\"\"\n";
    let width = LineWidth::try_from(24).unwrap();
    let expected = format(source, PyFormatOptions::default().with_line_width(width));
    assert_stable(
        source,
        &expected,
        docstring_options().with_line_width(width),
    );
}

#[test]
fn stub_docstrings_that_normalize_to_one_line_compact_stably() {
    let source = "def f():\n    \"\"\"Doc.\n    \"\"\"\n";
    assert_stable(
        source,
        "def f(): \"\"\"Doc.\"\"\"\n",
        PyFormatOptions::from_source_type(PySourceType::Stub)
            .with_custom(docstring_options().custom().clone()),
    );
}

#[test]
fn existing_stub_collapsing_is_unchanged() {
    let source = "def close():\n    ...\n\nclass Error(ValueError):\n    ...\n";
    let default_options = PyFormatOptions::from_source_type(PySourceType::Stub);
    let expected = format(source, default_options.clone());
    assert_eq!(
        expected,
        "def close(): ...\n\nclass Error(ValueError): ...\n"
    );
    assert_stable(source, &expected, default_options);
    assert_stable(
        source,
        &expected,
        PyFormatOptions::from_source_type(PySourceType::Stub).with_custom(
            definition_options(vec![OneLineSuiteStatement::Pass])
                .custom()
                .clone(),
        ),
    );
}
