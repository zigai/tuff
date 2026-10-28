use tuff_python_formatter::{
    CollectionLayout, DictAlignmentMode, PyFormatOptions, TuffCustomOptions, format_module_source,
};

fn format_with_custom(source: &str, custom: TuffCustomOptions) -> String {
    format_module_source(source, PyFormatOptions::default().with_custom(custom))
        .unwrap()
        .into_code()
}

#[test]
fn compact_dictionaries_do_not_keep_padding_from_expanded_input() {
    for mode in [DictAlignmentMode::Value, DictAlignmentMode::Colon] {
        let mut custom = TuffCustomOptions::default();
        custom.alignment.dict_alignment = mode;
        custom.collections.dicts.layout = CollectionLayout::PreferCompact;
        custom.collections.dicts.ignore_magic_trailing_comma = true;
        let source = "a = {\n    \"x\": 1,\n    \"long\": 2,\n}\n";
        let formatted = format_with_custom(source, custom.clone());
        assert_eq!(formatted, "a = {\"x\": 1, \"long\": 2}\n");
        assert_eq!(format_with_custom(&formatted, custom), formatted);
    }
}

#[test]
fn compact_collections_keep_flat_interpolations_flat() {
    let mut custom = compact_options();
    custom.collections.lists.ignore_magic_trailing_comma = false;
    custom.collections.dicts.ignore_magic_trailing_comma = false;
    custom.collections.sets.ignore_magic_trailing_comma = false;
    custom.collections.tuples.ignore_magic_trailing_comma = false;
    custom.collections.lists.compact_nested = true;
    for source in [
        "print(f\"{[1, 2,]}\")\n",
        "print(f\"{(1, 2,)}\")\n",
        "print(f\"{ {1, 2,} }\")\n",
        "print(f\"{ {1: 2,} }\")\n",
        "print(f\"{[(1, 2,), (3, 4,)]}\")\n",
    ] {
        let options =
            PyFormatOptions::default().with_target_version(ruff_python_ast::PythonVersion::PY311);
        let expected = format_module_source(source, options.clone())
            .unwrap()
            .into_code();
        let options = options.with_custom(custom.clone());
        let formatted = format_module_source(source, options.clone())
            .unwrap()
            .into_code();
        assert_eq!(formatted, expected, "{source}");
        assert_eq!(
            format_module_source(&formatted, options)
                .unwrap()
                .into_code(),
            formatted
        );
    }
}

#[test]
fn force_expanded_lists() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::ForceExpanded;

    let source = "values = [1, 2, 3]\n";
    let expected = r#"values = [
    1,
    2,
    3,
]
"#;

    let formatted = format_with_custom(source, custom.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}

#[test]
fn fill_lists() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::Fill;

    let source = r#"values = [
    "read",
    "write",
    "delete",
    "admin",
    "billing",
    "support",
    "analytics",
]
"#;
    let expected = r#"values = [
    "read", "write", "delete", "admin", "billing", "support", "analytics",
]
"#;

    let formatted = format_with_custom(source, custom.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}

#[test]
fn fill_tuples() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.tuples.layout = CollectionLayout::Fill;

    let source = r#"values = (
    1,
    2,
    3,
    4,
    5,
    6,
    7,
    8,
)
"#;
    let expected = "values = (\n    1, 2, 3, 4, 5, 6, 7, 8,\n)\n";

    let formatted = format_with_custom(source, custom.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}

#[test]
fn fill_keeps_single_line_collections_flat() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::Fill;
    custom.collections.tuples.layout = CollectionLayout::Fill;
    custom.collections.sets.layout = CollectionLayout::Fill;
    custom.collections.dicts.layout = CollectionLayout::Fill;

    let source = r#"list_values = ["read", "write", "delete"]
tuple_values = (1, 2, 3)
set_values = {"read", "write", "delete"}
dict_values = {"id": 1, "display_name": "Ada"}
"#;

    let formatted = format_with_custom(source, custom.clone());

    assert_eq!(formatted, source);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}

#[test]
fn force_expanded_dicts() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.dicts.layout = CollectionLayout::ForceExpanded;

    let source = r#"values = {"id": 1, "display_name": "Ada"}"#;
    let expected = r#"values = {
    "id": 1,
    "display_name": "Ada",
}
"#;

    assert_eq!(format_with_custom(source, custom), expected);
}

#[test]
fn expand_if_more_than_threshold() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::ExpandIfMoreThan { threshold: 2 };

    assert_eq!(
        format_with_custom("values = [1, 2]\n", custom.clone()),
        "values = [1, 2]\n"
    );

    assert_eq!(
        format_with_custom("values = [1, 2, 3]\n", custom),
        r#"values = [
    1,
    2,
    3,
]
"#
    );
}

#[test]
fn force_expanded_tuples() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.tuples.layout = CollectionLayout::ForceExpanded;

    let source = "values = (1, 2, 3)\n";
    let expected = r#"values = (
    1,
    2,
    3,
)
"#;

    assert_eq!(format_with_custom(source, custom), expected);
}

#[test]
fn force_expanded_sets() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.sets.layout = CollectionLayout::ForceExpanded;

    let source = "values = {1, 2, 3}\n";
    let expected = r#"values = {
    1,
    2,
    3,
}
"#;

    assert_eq!(format_with_custom(source, custom), expected);
}

fn format_at_width(source: &str, custom: TuffCustomOptions, width: u16) -> String {
    let options = PyFormatOptions::default()
        .with_custom(custom)
        .with_line_width(ruff_formatter::LineWidth::try_from(width).unwrap());
    format_module_source(source, options).unwrap().into_code()
}

fn compact_options() -> TuffCustomOptions {
    let mut custom = TuffCustomOptions::default();
    for policy in [
        &mut custom.collections.lists,
        &mut custom.collections.dicts,
        &mut custom.collections.tuples,
        &mut custom.collections.sets,
    ] {
        policy.layout = CollectionLayout::PreferCompact;
        policy.ignore_magic_trailing_comma = true;
    }
    custom
}

#[test]
fn prefer_compact_collapses_magic_commas_only_when_enabled() {
    let source = "a = [\n    1,\n    2,\n]\nb = {\n    1,\n    2,\n}\nc = (\n    1,\n    2,\n)\nd = {\n    1: 2,\n    3: 4,\n}\n";
    assert_eq!(
        format_with_custom(source, TuffCustomOptions::default()),
        source
    );
    let mut custom = compact_options();
    assert_eq!(
        format_with_custom(source, custom.clone()),
        "a = [1, 2]\nb = {1, 2}\nc = (1, 2)\nd = {1: 2, 3: 4}\n"
    );
    custom.collections.lists.ignore_magic_trailing_comma = false;
    assert!(format_with_custom(source, custom).starts_with("a = [\n    1, 2,\n]\n"));
}

#[test]
fn prefer_compact_packs_overflowing_collections_idempotently() {
    for (source, expected) in [
        (
            "a = [111, 222, 333, 444, 555, 666]\n",
            "a = [\n    111, 222, 333, 444,\n    555, 666,\n]\n",
        ),
        (
            "a = (111, 222, 333, 444, 555, 666)\n",
            "a = (\n    111, 222, 333, 444,\n    555, 666,\n)\n",
        ),
        (
            "a = {111, 222, 333, 444, 555, 666}\n",
            "a = {\n    111, 222, 333, 444,\n    555, 666,\n}\n",
        ),
        (
            "a = {1: 111, 2: 222, 3: 333, 4: 444}\n",
            "a = {\n    1: 111, 2: 222,\n    3: 333, 4: 444,\n}\n",
        ),
    ] {
        let custom = compact_options();
        let formatted = format_at_width(source, custom.clone(), 24);
        assert_eq!(formatted, expected);
        assert!(formatted.lines().all(|line| line.len() <= 24));
        assert_eq!(format_at_width(&formatted, custom, 24), formatted);
    }
}

#[test]
fn compact_collections_preserve_comments_and_singleton_tuple_commas() {
    let source =
        "a = [\n    1,  # keep\n    2,\n]\nb = {\n    1: 2,  # pair\n    3: 4,\n}\nc = (1,)\n";
    let custom = compact_options();
    assert_eq!(format_with_custom(source, custom.clone()), source);
    assert_eq!(format_with_custom(source, custom), source);
}

#[test]
fn ignore_magic_comma_also_works_without_compact_layout() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.ignore_magic_trailing_comma = true;
    assert_eq!(format_with_custom("a = [1, 2,]\n", custom), "a = [1, 2]\n");
}

#[test]
fn nested_collections_are_rows_and_respect_inner_overrides() {
    let source = "a = [[111, 222, 333, 444, 555, 666], [777, 888]]\n";
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::ExpandIfMoreThan { threshold: 4 };
    // Explicit expansion wins even when nested compaction is enabled.
    custom.collections.lists.compact_nested = true;
    let formatted = format_at_width(source, custom.clone(), 28);
    assert!(formatted.contains("        111,\n        222,"));
    assert_eq!(format_at_width(&formatted, custom, 28), formatted);

    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::ForceExpanded;
    custom.collections.lists.compact_nested = true;
    let source = "a = [(111, 222, 333, 444, 555, 666), {1: 111, 2: 222, 3: 333}]\n";
    let formatted = format_at_width(source, custom.clone(), 28);
    assert!(formatted.contains("111, 222, 333, 444,"));
    assert!(formatted.contains("1: 111, 2: 222,"));
    assert!(formatted.lines().all(|line| line.len() <= 28));
    assert_eq!(format_at_width(&formatted, custom, 28), formatted);
}

#[test]
fn singleton_tuples_compact_their_nested_collection() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.tuples.compact_nested = true;
    let source = "a = ([111, 222, 333, 444, 555, 666],)\n";
    let formatted = format_at_width(source, custom.clone(), 28);
    assert!(formatted.contains("111, 222, 333, 444,"), "{formatted}");
    assert_eq!(format_at_width(&formatted, custom, 28), formatted);
}

#[test]
fn nested_dict_values_can_be_compact_rows() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.dicts.layout = CollectionLayout::ForceExpanded;
    custom.collections.dicts.compact_nested = true;
    let source = "a = {1: [111, 222, 333, 444, 555, 666]}\n";
    let formatted = format_at_width(source, custom.clone(), 28);
    assert!(formatted.contains("111, 222, 333, 444,"));
    assert_eq!(format_at_width(&formatted, custom, 28), formatted);
}

#[test]
fn nested_compaction_preserves_parentheses_comments_and_tuple_semantics() {
    let source = r#"a = [
    (
        (1,),  # singleton
        (x for x in items),
    ),
    {(1, 2): [*(3, 4), (5,)]},
    ((6, 7)),
    ((x for x in items), 1),
]
"#;
    let mut custom = compact_options();
    custom.collections.lists.layout = CollectionLayout::ForceExpanded;
    custom.collections.lists.compact_nested = true;
    custom.collections.dicts.compact_nested = true;
    let formatted = format_at_width(source, custom.clone(), 32);
    assert!(formatted.contains("# singleton"));
    let parse_options = ruff_python_parser::ParseOptions::from(ruff_python_parser::Mode::Module);
    let before = ruff_python_parser::parse(source, parse_options.clone())
        .unwrap()
        .into_syntax();
    let after = ruff_python_parser::parse(&formatted, parse_options)
        .unwrap()
        .into_syntax();
    assert_eq!(
        ruff_python_ast::comparable::ComparableMod::from(&before),
        ruff_python_ast::comparable::ComparableMod::from(&after),
    );
    assert_eq!(format_at_width(&formatted, custom, 32), formatted);
}

#[test]
fn compact_collection_options_do_not_affect_docstring_code() {
    let source = "def example():\n    \"\"\"Example.\n\n    ```python\n    a = [\n        1,\n        2,\n    ]\n    ```\n    \"\"\"\n";
    let mut custom = compact_options();
    custom.collections.lists.compact_nested = true;
    let options = PyFormatOptions::default()
        .with_docstring_code(tuff_python_formatter::DocstringCode::Enabled)
        .with_custom(custom);
    let formatted = format_module_source(source, options.clone())
        .unwrap()
        .into_code();
    assert_eq!(formatted, source);
    assert_eq!(
        format_module_source(&formatted, options)
            .unwrap()
            .into_code(),
        formatted
    );
}

#[test]
fn prefer_compact_respects_magic_comma_without_losing_idempotence() {
    let mut custom = TuffCustomOptions::default();
    custom.collections.lists.layout = CollectionLayout::PreferCompact;
    let formatted = format_at_width("a = [111, 222, 333, 444, 555, 666]\n", custom.clone(), 24);
    assert_eq!(
        formatted,
        "a = [\n    111, 222, 333, 444,\n    555, 666,\n]\n"
    );
    assert_eq!(format_at_width(&formatted, custom, 24), formatted);
}

fn assert_tuple_semantics(before: &str, after: &str) {
    use ruff_python_ast::visitor::transformer::{self, Transformer};
    use ruff_python_ast::{Expr, Stmt};

    struct NormalizeDelete;
    impl Transformer for NormalizeDelete {
        fn visit_stmt(&self, statement: &mut Stmt) {
            // `del a, b` and `del (a, b)` delete the same targets.
            if let Stmt::Delete(delete) = statement {
                if let [Expr::Tuple(tuple)] = delete.targets.as_slice() {
                    delete.targets = tuple.elts.clone();
                }
            }
            transformer::walk_stmt(self, statement);
        }
    }

    let parse = |source| {
        let mut module = ruff_python_parser::parse(
            source,
            ruff_python_parser::ParseOptions::from(ruff_python_parser::Mode::Module),
        )
        .unwrap()
        .into_syntax();
        match &mut module {
            ruff_python_ast::Mod::Module(module) => NormalizeDelete.visit_body(&mut module.body),
            ruff_python_ast::Mod::Expression(expression) => {
                NormalizeDelete.visit_expr(&mut expression.body);
            }
        }
        module
    };
    let before = parse(before);
    let after = parse(after);
    assert_eq!(
        ruff_python_ast::comparable::ComparableMod::from(&before),
        ruff_python_ast::comparable::ComparableMod::from(&after),
    );
}

#[test]
fn compact_tuples_only_add_parentheses_when_needed() {
    for (source, expected, width) in [
        ("a = 1, 2, 3\n", "a = 1, 2, 3\n", 32),
        ("a = (1, 2, 3)\n", "a = (1, 2, 3)\n", 32),
        (
            "a = 111, 222, 333, 444, 555, 666\n",
            "a = (\n    111, 222, 333, 444,\n    555, 666,\n)\n",
            24,
        ),
    ] {
        let custom = compact_options();
        let formatted = format_at_width(source, custom.clone(), width);
        assert_eq!(formatted, expected);
        assert_tuple_semantics(source, &formatted);
        assert_eq!(format_at_width(&formatted, custom, width), formatted);
    }
}

#[test]
fn compact_for_targets_pack_on_the_first_pass() {
    for source in [
        "for first, second in items:\n    pass\n",
        "for (first, second) in items:\n    pass\n",
    ] {
        let custom = compact_options();
        let formatted = format_at_width(source, custom.clone(), 32);
        assert_eq!(formatted, "for first, second in items:\n    pass\n");
        assert_tuple_semantics(source, &formatted);
        assert_eq!(format_at_width(&formatted, custom, 32), formatted);
    }

    let source = "for first_item, second_item, third_item in items:\n    pass\n";
    let custom = compact_options();
    let formatted = format_at_width(source, custom.clone(), 32);
    assert_eq!(
        formatted,
        "for (\n    first_item, second_item,\n    third_item,\n) in items:\n    pass\n"
    );
    assert_tuple_semantics(source, &formatted);
    assert_eq!(format_at_width(&formatted, custom, 32), formatted);
}

#[test]
fn compact_delete_targets_pack_on_the_first_pass() {
    for (source, expected) in [
        ("del first, second\n", "del first, second\n"),
        (
            "del first_item, second_item, third_item\n",
            "del (\n    first_item, second_item,\n    third_item,\n)\n",
        ),
    ] {
        let custom = compact_options();
        let formatted = format_at_width(source, custom.clone(), 32);
        assert_eq!(formatted, expected);
        assert_tuple_semantics(source, &formatted);
        assert_eq!(format_at_width(&formatted, custom, 32), formatted);
    }
}

#[test]
fn compact_interpolation_tuples_do_not_introduce_parentheses() {
    let source = "f\"{ {}, 1 }\"\n";
    let custom = compact_options();
    let formatted = format_with_custom(source, custom.clone());
    assert_eq!(formatted, source);
    assert_tuple_semantics(source, &formatted);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}

#[test]
fn compact_tuple_special_contexts_keep_semantic_commas() {
    let source = "a = (1,)\nb = data[1, 2]\nc = data[*items]\nd = data[*items,]\ne = [x for x, y in items]\n";
    let custom = compact_options();
    let formatted = format_with_custom(source, custom.clone());
    assert_eq!(formatted, source);
    assert_tuple_semantics(source, &formatted);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}

#[test]
fn compact_template_tuple_does_not_introduce_parentheses() {
    let source = "t\"{ {}, 1 }\"\n";
    let custom = compact_options();
    let formatted = format_with_custom(source, custom.clone());
    assert_eq!(formatted, source);
    assert_eq!(format_with_custom(&formatted, custom), formatted);
}
