use ruff_formatter::LineWidth;
use tuff_python_formatter::{
    AlignmentMode, FeatureMode, OneLineSuiteClause, OneLineSuiteStatement, PyFormatOptions,
    TuffCustomOptions, format_module_source,
};

fn format(source: &str, custom: TuffCustomOptions, width: u16) -> String {
    format_module_source(
        source,
        PyFormatOptions::default()
            .with_line_width(LineWidth::try_from(width).unwrap())
            .with_custom(custom),
    )
    .unwrap()
    .into_code()
}

fn check(source: &str, expected: &str, custom: TuffCustomOptions, width: u16) {
    let result = format(source, custom.clone(), width);
    assert_eq!(result, expected);
    assert_eq!(format(&result, custom, width), result);
    assert!(result.lines().all(|line| line.len() <= usize::from(width)));
}

#[test]
fn calls_fill_rows_and_respect_magic_comma() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    check(
        "result = transform(source, destination, encoding=\"utf-8\", overwrite=True, preserve_metadata=False)\n",
        "result = transform(\n    source, destination, encoding=\"utf-8\",\n    overwrite=True, preserve_metadata=False,\n)\n",
        custom.clone(),
        48,
    );
    check("f(a, b, c,)\n", "f(\n    a, b, c,\n)\n", custom.clone(), 48);
    custom.arguments.ignore_magic_trailing_comma = true;
    check("f(a, b, c,)\n", "f(a, b, c)\n", custom, 48);
}

#[test]
fn mixed_calls_pack_short_prefix_and_suffix_independently() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.mixed = FeatureMode::Enabled;
    check(
        "result = request(\"POST\", endpoint, timeout=30, headers={\"Authorization\": token, \"Content-Type\": \"application/json\",}, retry=True, cache=False)\n",
        "result = request(\n    \"POST\", endpoint, timeout=30,\n    headers={\n        \"Authorization\": token,\n        \"Content-Type\": \"application/json\",\n    },\n    retry=True, cache=False,\n)\n",
        custom.clone(),
        48,
    );
    check(
        "f(a, b, \"\"\"first\nsecond\"\"\", c, d)\n",
        "f(\n    a, b,\n    \"\"\"first\nsecond\"\"\",\n    c, d,\n)\n",
        custom,
        48,
    );
}

#[test]
fn mixed_calls_are_stable_when_literals_merge() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.mixed = FeatureMode::Enabled;
    for (source, expected) in [
        (
            "f(a, \"b\" \"c\", d,)\n",
            "f(\n    a,\n    \"bc\",\n    d,\n)\n",
        ),
        (
            "f(a, b\"b\" b\"c\", d,)\n",
            "f(\n    a,\n    b\"bc\",\n    d,\n)\n",
        ),
    ] {
        check(source, expected, custom.clone(), 48);
    }
}

#[test]
fn parameters_preserve_all_separators_and_lambda_grammar() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.parameters = FeatureMode::Enabled;
    check(
        "def f(a, b, /, c, *, d, **kwargs,):\n    pass\n",
        "def f(\n    a, b, /, c, *, d, **kwargs,\n):\n    pass\n",
        custom.clone(),
        48,
    );
    check(
        "def f(a, /, *args, b, **kwargs,):\n    pass\n",
        "def f(\n    a, /, *args, b, **kwargs,\n):\n    pass\n",
        custom.clone(),
        48,
    );
    check(
        "f = lambda a, b, /, *, c, **kw: a\n",
        "f = lambda a, b, /, *, c, **kw: a\n",
        custom,
        48,
    );
}

#[test]
fn comments_on_call_targets_keep_ruff_argument_layout() {
    let source = "result = (\n    client  # fmt: skip\n    .fetch(\"key\", {})\n    .send()\n)\n";
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    custom.arguments.mixed = FeatureMode::Enabled;
    let expected = format(source, TuffCustomOptions::default(), 40);
    check(source, &expected, custom, 40);
}

#[test]
fn comments_use_ruff_layout_and_remain_attached() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    custom.arguments.parameters = FeatureMode::Enabled;
    custom.arguments.mixed = FeatureMode::Enabled;
    for source in [
        "f(  # opening\n    a,  # trailing\n    # leading\n    b,\n)\n",
        "f(\n    # dangling\n)\n",
        "def f(  # opening\n    a,\n    /,  # slash\n    # star leading\n    *,  # star\n    b,  # trailing\n):\n    pass\n",
        "f(a, headers={\n    # inner\n    \"key\": value,\n}, b=True)\n",
    ] {
        let expected = format(source, TuffCustomOptions::default(), 48);
        check(source, &expected, custom.clone(), 48);
    }
}

#[test]
fn disabled_and_alignment_layouts_are_unchanged() {
    let source = "def f(a: int, longer: str,):\n    return call(a, longer, flag=True,)\n";
    let disabled = TuffCustomOptions::default();
    let expected = format(source, disabled.clone(), 48);
    check(source, &expected, disabled, 48);
    let mut aligned = TuffCustomOptions::default();
    aligned.alignment.function_params = AlignmentMode::Enabled;
    aligned.alignment.call_keyword_args = AlignmentMode::Enabled;
    let expected = format(source, aligned.clone(), 48);
    aligned.arguments.calls = FeatureMode::Enabled;
    aligned.arguments.parameters = FeatureMode::Enabled;
    check(source, &expected, aligned, 48);
}

#[test]
fn singleton_argument_and_parameter_commas_can_be_ignored() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    custom.arguments.parameters = FeatureMode::Enabled;
    custom.arguments.ignore_magic_trailing_comma = true;
    for (source, expected) in [
        ("f(value,)\n", "f(value)\n"),
        (
            "f((item for item in items),)\n",
            "f((item for item in items))\n",
        ),
        ("def f(value,):\n    pass\n", "def f(value):\n    pass\n"),
    ] {
        check(source, expected, custom.clone(), 48);
    }
}

#[test]
fn compact_definition_headers_are_stable_when_annotations_merge() {
    let source = "def construct_reusable_status_record(value) -> \"ListOf\" \"FormattedResultValues\":\n    pass\n";
    let mut custom = TuffCustomOptions::default();
    custom.arguments.parameters = FeatureMode::Enabled;
    custom.arguments.ignore_magic_trailing_comma = true;
    custom.one_line_suites.mode = FeatureMode::Enabled;
    custom.one_line_suites.clauses = vec![OneLineSuiteClause::Function];
    custom.one_line_suites.statements = vec![OneLineSuiteStatement::Pass];
    for width in [48, 72, 88] {
        let formatted = format(source, custom.clone(), width);
        assert_eq!(format(&formatted, custom.clone(), width), formatted);
        assert!(
            formatted
                .lines()
                .all(|line| line.len() <= usize::from(width)),
            "{formatted}"
        );
    }
}

#[test]
fn generator_arguments_remain_grammar_safe() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    for source in [
        "f(x for x in values)\n",
        "f((x for x in values), other,)\n",
        "f(*args, **kwargs,)\n",
    ] {
        let result = format(source, custom.clone(), 48);
        assert_eq!(format(&result, custom.clone(), 48), result);
    }
}

#[test]
fn packing_is_stable_across_line_widths() {
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    custom.arguments.parameters = FeatureMode::Enabled;
    for width in [24, 32, 48, 88] {
        for source in [
            "def function(a: int = 1, b: str = \"b\", /, *args, c=None, **kwargs) -> str:\n    return b\n",
            "result = call(first, second, nested=inner(first, second, third), last=True)\n",
            "def f(a, /):\n    pass\n",
            "def f(a, /,):\n    pass\n",
        ] {
            let result = format(source, custom.clone(), width);
            assert_eq!(format(&result, custom.clone(), width), result);
            assert!(
                result.lines().all(|line| line.len() <= usize::from(width)),
                "{result}"
            );
        }
    }
}

#[test]
fn packing_does_not_affect_docstring_code() {
    let source = "def example():\n    \"\"\"Example.\n\n    ```python\n    def f(a, b,):\n        pass\n\n    f(a, b,)\n    from package import (A, B,)\n    ```\n    \"\"\"\n";
    let mut custom = TuffCustomOptions::default();
    custom.arguments.calls = FeatureMode::Enabled;
    custom.arguments.parameters = FeatureMode::Enabled;
    custom.arguments.mixed = FeatureMode::Enabled;
    custom.imports.mode = FeatureMode::Enabled;
    let options = PyFormatOptions::default()
        .with_docstring_code(tuff_python_formatter::DocstringCode::Enabled);
    let expected = format_module_source(source, options.clone())
        .unwrap()
        .into_code();
    let options = options.with_custom(custom);
    let result = format_module_source(source, options.clone())
        .unwrap()
        .into_code();
    assert_eq!(result, expected);
    assert_eq!(
        format_module_source(&result, options).unwrap().into_code(),
        result
    );
}
