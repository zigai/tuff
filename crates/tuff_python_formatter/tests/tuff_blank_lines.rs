use ruff_db::files::{File, Files, system_path_to_file};
use ruff_db::system::{InMemorySystem, System, SystemPath};
use ruff_db::vendored::VendoredFileSystem;
use ruff_formatter::{
    LineWidth,
    printer::{LineEnding, SourceMapGeneration},
};
use ruff_python_parser::{ParseOptions, parse};
use ruff_python_trivia::TriviaRanges;
use ruff_text_size::{TextRange, TextSize};
use tuff_lint::builtin::spacing::SpacingRule::{
    self, AfterControlBlock, AfterTerminalControlBlock, BeforeAssignment, BeforeBranch,
    BeforeUnrelatedBlock, BlockHeaderCuddleStrict, MatchCaseSeparation,
    NoSuiteLeadingTrailingBlankLines,
};
use tuff_python_formatter::{
    AlignmentMode, FeatureMode, OneLineSuiteClause, OneLineSuiteStatement, PyFormatOptions,
    TuffCustomOptions, format_module_ast, format_module_source, format_range,
};

#[salsa::db]
#[derive(Default, Clone)]
struct Database {
    storage: salsa::Storage<Self>,
    files: Files,
    system: InMemorySystem,
    vendored: VendoredFileSystem,
    options: PyFormatOptions,
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl ruff_db::Db for Database {
    fn vendored(&self) -> &VendoredFileSystem {
        &self.vendored
    }
    fn system(&self) -> &dyn System {
        &self.system
    }
    fn files(&self) -> &Files {
        &self.files
    }
}

#[salsa::db]
impl tuff_python_formatter::Db for Database {
    fn format_options(&self, _: File) -> PyFormatOptions {
        self.options.clone()
    }
}

fn options(rules: &[SpacingRule], width: u16) -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.blank_lines.rules = rules.to_vec();
    PyFormatOptions::default()
        .with_line_width(LineWidth::try_from(width).unwrap())
        .with_custom(custom)
}

fn check(source: &str, expected: &str, options: PyFormatOptions) {
    let result = format_module_source(source, options.clone()).unwrap();
    assert_eq!(result.as_code(), expected);
    assert_eq!(
        format_module_source(result.as_code(), options)
            .unwrap()
            .as_code(),
        expected,
        "second format must be identical"
    );
}

#[test]
fn all_eight_native_policies() {
    for (rule, source, expected) in [
        (
            AfterControlBlock,
            "def f(x):\n    if x:\n        x += 1\n    return x\n",
            "def f(x):\n    if x:\n        x += 1\n\n    return x\n",
        ),
        (
            AfterTerminalControlBlock,
            "def f(x):\n    if x:\n        work()\n    else:\n        return\n    followup()\n",
            "def f(x):\n    if x:\n        work()\n    else:\n        return\n\n    followup()\n",
        ),
        (
            BeforeAssignment,
            "def f(logger, handler):\n    logger.addHandler(handler)\n\n    logger.propagate = False\n",
            "def f(logger, handler):\n    logger.addHandler(handler)\n    logger.propagate = False\n",
        ),
        (
            BeforeBranch,
            "def f():\n    a = 1\n    b = 2\n    c = 3\n    return c\n",
            "def f():\n    a = 1\n    b = 2\n    c = 3\n\n    return c\n",
        ),
        (
            BeforeUnrelatedBlock,
            "def f(flag):\n    log()\n    if flag:\n        work()\n",
            "def f(flag):\n    log()\n\n    if flag:\n        work()\n",
        ),
        (
            BlockHeaderCuddleStrict,
            "def f(flag):\n    prepared = compute()\n    if flag:\n        consume(prepared)\n",
            "def f(flag):\n    prepared = compute()\n\n    if flag:\n        consume(prepared)\n",
        ),
        (
            MatchCaseSeparation,
            "match value:\n    case 1:\n        a = 1\n        b = 2\n        c = 3\n    case _:\n        pass\n",
            "match value:\n    case 1:\n        a = 1\n        b = 2\n        c = 3\n\n    case _:\n        pass\n",
        ),
        (
            NoSuiteLeadingTrailingBlankLines,
            "def f():\n\n    work()\n\n\n",
            "def f():\n    work()\n",
        ),
    ] {
        check(source, expected, options(&[rule], 88));
        // Already-satisfied constraints must remain authoritative over Ruff's defaults.
        check(expected, expected, options(&[rule], 88));
    }
}

#[test]
fn satisfied_suite_constraints_override_definition_spacing() {
    let source = "if flag:\n    def inner():\n        work()\nelse:\n    pass\n";
    check(
        source,
        source,
        options(&[SpacingRule::NoSuiteLeadingTrailingBlankLines], 88),
    );
    let source = "def f():\n    if flag:\n        def inner():\n            work()\n    else:\n        pass\n";
    check(
        source,
        source,
        options(&[SpacingRule::NoSuiteLeadingTrailingBlankLines], 88),
    );
}

#[test]
fn suite_footer_constraints_are_not_reinserted_by_the_containing_suite() {
    check(
        "def f(flag):\n    if flag:\n        work()\n        \n    followup()\n",
        "def f(flag):\n    if flag:\n        work()\n    followup()\n",
        options(&[SpacingRule::NoSuiteLeadingTrailingBlankLines], 88),
    );
    // An unindented blank belongs to the outer separator, not the inner footer.
    check(
        "def f(flag):\n    if flag:\n        work()\n        \n\n    followup()\n",
        "def f(flag):\n    if flag:\n        work()\n\n    followup()\n",
        options(&[SpacingRule::NoSuiteLeadingTrailingBlankLines], 88),
    );
}

#[test]
fn thresholds_measure_rendered_not_source_lines() {
    let source = "def f():\n    result = compute(first_argument, second_argument, third_argument)\n    return result\n";
    let mut wide = options(&[SpacingRule::BeforeBranch], 100);
    let mut custom = wide.custom().clone();
    custom
        .blank_lines
        .options
        .before_branch
        .allow_related_return_tails = false;
    wide = wide.with_custom(custom.clone());
    check(source, source, wide);
    check(
        source,
        "def f():\n    result = compute(\n        first_argument,\n        second_argument,\n        third_argument,\n    )\n\n    return result\n",
        options(&[SpacingRule::BeforeBranch], 40).with_custom(custom.clone()),
    );
    // Source is tall but renders to two nonempty body lines: no separation is required.
    check(
        "def f():\n    result = (\n        1\n    )\n    return result\n",
        "def f():\n    result = 1\n    return result\n",
        options(&[SpacingRule::BeforeBranch], 88).with_custom(custom),
    );

    check(
        "match value:\n    case 1:\n        consume(first_argument, second_argument, third_argument)\n    case _:\n        pass\n",
        "match value:\n    case 1:\n        consume(\n            first_argument,\n            second_argument,\n            third_argument,\n        )\n\n    case _:\n        pass\n",
        options(&[SpacingRule::MatchCaseSeparation], 40),
    );
}

#[test]
fn alignment_and_one_line_suites_remain_enabled() {
    let mut custom = TuffCustomOptions::default();
    custom.blank_lines.rules = vec![
        SpacingRule::BeforeBranch,
        SpacingRule::BlockHeaderCuddleStrict,
    ];
    custom
        .blank_lines
        .options
        .before_branch
        .allow_related_return_tails = false;
    custom
        .blank_lines
        .options
        .before_branch
        .max_suite_non_empty_lines = 5;
    custom.alignment.assignments = AlignmentMode::Enabled;
    custom.one_line_suites.mode = FeatureMode::Enabled;
    let source = "def f(flag):\n    x = 1\n    longer = 2\n    if flag:\n        return x\n    audit()\n    return longer\n";
    check(
        source,
        "def f(flag):\n    x      = 1\n    longer = 2\n\n    if flag: return x\n    audit()\n    return longer\n",
        options(&[SpacingRule::BeforeBranch], 88).with_custom(custom),
    );
}

#[test]
fn comments_and_suppressed_regions_are_not_rewritten() {
    let source = "def f(flag):\n    # fmt: off\n    if  flag:\n        work( )\n    followup( )\n    # fmt: on\n    if flag:\n        work()\n    # separator\n    followup()\n    if flag:\n        work()  # fmt: skip\n    followup()\n";
    let expected = "def f(flag):\n    # fmt: off\n    if  flag:\n        work( )\n    followup( )\n    # fmt: on\n    if flag:\n        work()\n    # separator\n    followup()\n    if flag:\n        work()  # fmt: skip\n\n    followup()\n";
    check(
        source,
        expected,
        options(&[SpacingRule::AfterControlBlock], 88),
    );
    let source =
        "def f(flag):\n    if flag:\n        work()\n        # body footer\n    followup()\n";
    check(
        source,
        "def f(flag):\n    if flag:\n        work()\n        # body footer\n\n    followup()\n",
        options(&[SpacingRule::AfterControlBlock], 88),
    );
    let source = include_str!("../resources/test/fixtures/ruff/fmt_on_off/trailing_semicolon.py");
    let opts = options(
        &SpacingRule::ALL
            .into_iter()
            .filter(|rule| *rule != SpacingRule::BeforeUnrelatedBlock)
            .collect::<Vec<_>>(),
        88,
    );
    let printed = format_module_source(source, opts.clone()).unwrap();
    assert!(
        printed
            .as_code()
            .contains("with_semicolon = 10   \\\n            ;")
    );
    assert_eq!(
        format_module_source(printed.as_code(), opts)
            .unwrap()
            .as_code(),
        printed.as_code()
    );
}

#[test]
fn ast_entrypoint_source_maps_crlf_and_range() {
    for source in [
        "def f(x):\r\n    if x:\r\n        x += 1\r\n    return x\r\n",
        "def f(x):\r\n    if x: x += 1\r\n    return x\r\n",
    ] {
        let opts = options(&[SpacingRule::AfterControlBlock], 88)
            .with_line_ending(LineEnding::CarriageReturnLineFeed)
            .with_source_map_generation(SourceMapGeneration::Enabled);
        let parsed = parse(source, ParseOptions::from(opts.source_type())).unwrap();
        let trivia = TriviaRanges::from(parsed.tokens());
        let printed = format_module_ast(&parsed, &trivia, source, opts.clone())
            .unwrap()
            .print()
            .unwrap();
        assert!(printed.as_code().contains("x += 1\r\n\r\n    return x"));
        assert!(!printed.as_code().replace("\r\n", "").contains('\n'));
        let return_source = TextSize::try_from(source.find("return x").unwrap()).unwrap();
        let return_dest = TextSize::try_from(printed.as_code().find("return x").unwrap()).unwrap();
        assert!(
            printed
                .sourcemap()
                .iter()
                .any(|marker| { marker.source == return_source && marker.dest == return_dest })
        );
        for marker in printed.sourcemap() {
            assert!(source.is_char_boundary(usize::from(marker.source)));
            assert!(printed.as_code().is_char_boundary(usize::from(marker.dest)));
        }
        let start = TextSize::try_from(source.find("if x:").unwrap()).unwrap();
        let end = TextSize::try_from(source.find("return x").unwrap() + "return x".len()).unwrap();
        let result = format_range(source, TextRange::new(start, end), opts).unwrap();
        assert!(result.as_code().contains("\r\n\r\n    return x"));
    }
}

#[test]
fn alternate_clause_headers_have_layout_markers() {
    check(
        "try: work()\nexcept ValueError: recover()\nelse: finish()\nfinally: clean()\n",
        "try:\n    work()\nexcept ValueError:\n    recover()\nelse:\n    finish()\nfinally:\n    clean()\n",
        options(&[NoSuiteLeadingTrailingBlankLines], 88),
    );
    check(
        "for value in []:\n    ...\nelse:\n    ...\n",
        "for value in []: ...\nelse: ...\n",
        options(&[NoSuiteLeadingTrailingBlankLines], 88)
            .with_source_type(ruff_python_ast::PySourceType::Stub),
    );
}

#[test]
fn nested_inline_expansion_and_compact_preservation_follow_layout() {
    check(
        "def run(flag):\n    if flag:\n        with manager(): work()\n        finish()\n    done()\n",
        "def run(flag):\n    if flag:\n        with manager():\n            work()\n\n        finish()\n\n    done()\n",
        options(&[AfterControlBlock], 88),
    );
    let mut custom = TuffCustomOptions::default();
    custom.blank_lines.rules = vec![AfterControlBlock];
    custom.one_line_suites.mode = FeatureMode::Enabled;
    custom.one_line_suites.clauses = vec![
        OneLineSuiteClause::If,
        OneLineSuiteClause::With,
        OneLineSuiteClause::For,
    ];
    custom.one_line_suites.statements = vec![OneLineSuiteStatement::Call];
    let opts = PyFormatOptions::default().with_custom(custom);
    for source in [
        "def run(flag):\n    if flag: work()\n    done()\n",
        "def run():\n    with manager(): work()\n    done()\n",
        "def run():\n    for item in items: work()\n    done()\n",
    ] {
        check(source, source, opts.clone());
    }
}

#[test]
fn database_entrypoint_tracks_rendered_width_and_source_revisions() {
    let mut database = Database {
        options: options(&[SpacingRule::BeforeBranch], 40),
        ..Database::default()
    };
    let path = SystemPath::new("/module.py");
    let source = "def run():\n    consume(first_argument, second_argument, third_argument)\n    return result\n";
    database.system.fs().write_file(path, source).unwrap();
    let file = system_path_to_file(&database, path).unwrap();
    let narrow = tuff_python_formatter::formatted_file(&database, file)
        .unwrap()
        .unwrap();
    assert!(narrow.contains(")\n\n    return result"));
    database.options = options(&[SpacingRule::BeforeBranch], 200);
    assert_eq!(
        tuff_python_formatter::formatted_file(&database, file).unwrap(),
        None
    );
    database.options = options(&[SpacingRule::BeforeBranch], 40);
    database.system.fs().write_file(path, narrow).unwrap();
    File::sync_path(&mut database, path);
    assert_eq!(
        tuff_python_formatter::formatted_file(&database, file).unwrap(),
        None
    );
}

#[test]
fn incompatible_header_policies_are_rejected() {
    let result = format_module_source(
        "pass\n",
        options(
            &[
                SpacingRule::BeforeUnrelatedBlock,
                SpacingRule::BlockHeaderCuddleStrict,
            ],
            88,
        ),
    );
    assert!(result.is_err());
}
