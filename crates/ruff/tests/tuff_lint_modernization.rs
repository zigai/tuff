#[path = "tuff/mod.rs"]
mod common;

#[test]
fn identity_folding_does_not_assume_tuple_or_complex_allocation() {
    for condition in [
        "(1, 2) is (1, 2) and flag",
        "1j is 1j and flag",
        "(1, 2) is not (1, 2) or flag",
        "(1, (2,)) is (1, (2,)) and flag",
    ] {
        common::assert_clean(
            "no-static-if-condition",
            &format!("if {condition}:\n    pass\n"),
            "",
        );
    }
    assert_eq!(
        common::check("no-static-if-condition", "if True:\n    pass\n", "")
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn static_conditions_do_not_assume_string_interning() {
    common::assert_clean(
        "no-static-if-condition",
        "if 'identifier' is 'identifier' or dynamic:\n    pass\n",
        "",
    );
    common::assert_clean(
        "no-static-if-condition",
        "if 'identifier' is not 'identifier' or dynamic:\n    pass\n",
        "",
    );
}

#[test]
fn dataclass_mutability_and_active_aliases() {
    for source in [
        "from dataclasses import dataclass\n@dataclass\nclass Model: pass\n",
        "import dataclasses as d\n@d.dataclass()\nclass Model: pass\n",
        "from dataclasses import dataclass\nD = E = dataclass\n@E(order=True)\nclass Model: pass\n",
        "from dataclasses import *\n@dataclass\nclass Model: pass\n",
    ] {
        assert_eq!(
            common::check("explicit-frozen-dataclass", source, "")
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    for source in [
        "from dataclasses import dataclass\n@dataclass(frozen=False)\nclass Model: pass\n",
        "from dataclasses import dataclass\n@dataclass(frozen=True)\nclass Model: pass\n",
        "import dataclasses\ndataclasses = other\n@dataclasses.dataclass\nclass Model: pass\n",
    ] {
        common::assert_clean("explicit-frozen-dataclass", source, "");
    }
}

#[test]
fn named_tuple_bases_and_factories() {
    for source in [
        "from typing import NamedTuple\nN = NamedTuple\nclass Record(N): pass\n",
        "from collections import namedtuple\nf = namedtuple\nRecord = f('Record', [])\n",
        "from typing_extensions import *\nclass Record(NamedTuple): pass\n",
        "from collections import *\nRecord = namedtuple('Record', [])\n",
    ] {
        let diagnostics = common::check("no-named-tuple", source, "");
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert!(diagnostics[0]["fix"].is_null());
    }
    common::assert_clean(
        "no-named-tuple",
        "import collections\ncollections = custom\nRecord = collections.namedtuple('Record', [])\n",
        "",
    );
}

#[test]
fn static_if_conditions() {
    for condition in [
        "True",
        "None",
        "-1",
        "...",
        "b''",
        "x or True",
        "x and not True",
        "False or False",
        "True and True",
        "(x := True)",
        "(x for x in values)",
        "lambda: None",
        "f''",
        "f'prefix{value}'",
        "f'{value=}'",
        "[*values, sentinel]",
        "{**mapping, 'key': value}",
        "1 == 1",
        "1 < 2 < 3",
        "1 in [1, 2]",
    ] {
        let source = format!("if {condition}:\n    pass\n");
        assert_eq!(
            common::check("no-static-if-condition", &source, "")
                .as_array()
                .unwrap()
                .len(),
            1,
            "{condition}"
        );
    }
    for condition in [
        "f'{value}'",
        "[*values]",
        "{**mapping}",
        "False or fn()",
        "1 < 'a'",
        "1 in 2",
        "1 < 2 < 'a'",
    ] {
        common::assert_clean(
            "no-static-if-condition",
            &format!("if {condition}:\n    pass\n"),
            "",
        );
    }
}

#[test]
fn named_tuff_suppression_migration() {
    for source in [
        "value = 1 # noqa\n",
        "# flake8:noqa\n",
        "# ruff: noqa\n",
        "value = 1 # noqa: F401\n",
        "value = 1 # type: ignore # noqa\n",
        "value = 1 # noqa: TUF017\n",
    ] {
        assert_eq!(
            common::check("use-tuff-ignore-comment", source, "")
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    for source in [
        "value = 1 # noqaed\n",
        "value = 1 # noqa-compatible\n",
        "value = 1 # The text \"# noqa\" names old syntax\n",
        "value = 1 # https://example.test/#noqa\n",
        "value = 1 # tuff: ignore[no-bare-object-annotations]\n",
    ] {
        common::assert_clean("use-tuff-ignore-comment", source, "");
    }
}

#[test]
fn migration_policy_can_be_suppressed_by_name() {
    common::assert_clean(
        "use-tuff-ignore-comment",
        "value = 1 # noqa # tuff: ignore[use-tuff-ignore-comment]\n",
        "",
    );
}
