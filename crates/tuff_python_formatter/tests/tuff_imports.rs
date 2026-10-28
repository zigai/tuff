use ruff_formatter::LineWidth;
use tuff_python_formatter::{
    AlignmentMode, FeatureMode, PyFormatOptions, TuffCustomOptions, format_module_source,
};

fn format(source: &str, custom: TuffCustomOptions) -> String {
    format_module_source(
        source,
        PyFormatOptions::default()
            .with_line_width(LineWidth::try_from(48).unwrap())
            .with_custom(custom),
    )
    .unwrap()
    .into_code()
}

fn check(source: &str, expected: &str, custom: TuffCustomOptions) {
    let result = format(source, custom.clone());
    assert_eq!(result, expected);
    assert_eq!(format(&result, custom), result);
    assert!(result.lines().all(|line| line.len() <= 48));
}

#[test]
fn imports_fill_rows_in_existing_order() {
    let mut custom = TuffCustomOptions::default();
    custom.imports.mode = FeatureMode::Enabled;
    check(
        "from package import Client, Config, Connection, Request, Response, Transport\n",
        "from package import (\n    Client, Config, Connection, Request,\n    Response, Transport,\n)\n",
        custom.clone(),
    );
    check(
        "from .package import (Z as z, A as a,)\n",
        "from .package import (\n    Z as z, A as a,\n)\n",
        custom.clone(),
    );
    custom.imports.ignore_magic_trailing_comma = true;
    check(
        "from .package import (Z as z, A as a,)\n",
        "from .package import Z as z, A as a\n",
        custom,
    );
}

#[test]
fn comments_star_and_disabled_layout_are_preserved() {
    let mut custom = TuffCustomOptions::default();
    custom.imports.mode = FeatureMode::Enabled;
    for source in [
        "from package import *\n",
        "from package import (  # opening\n    A,  # trailing\n    # leading\n    B,\n)\n",
        "from package import (\n    # leading\n    A,\n    # final\n)\n",
    ] {
        let expected = format(source, TuffCustomOptions::default());
        check(source, &expected, custom.clone());
    }
    let source = "from package import (A, B, C,)\n";
    let expected = format(source, TuffCustomOptions::default());
    check(source, &expected, TuffCustomOptions::default());
}

#[test]
fn alias_alignment_takes_precedence() {
    let source = "from package import (short as a, longer as b,)\n";
    let mut custom = TuffCustomOptions::default();
    custom.alignment.import_aliases = AlignmentMode::Enabled;
    let expected = format(source, custom.clone());
    custom.imports.mode = FeatureMode::Enabled;
    check(source, &expected, custom);
}
