use tuff_python_formatter::{
    CollectionLayout, PyFormatOptions, TuffCustomOptions, format_module_source,
};

fn format_with_custom(source: &str, custom: TuffCustomOptions) -> String {
    format_module_source(source, PyFormatOptions::default().with_custom(custom))
        .unwrap()
        .into_code()
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
