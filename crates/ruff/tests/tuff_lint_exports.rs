#[path = "tuff/mod.rs"]
mod common;

#[test]
fn bottom_movement_is_narrow() {
    common::assert_fix(
        "module-all-at-bottom",
        "__all__ = ['build']\n\ndef build():\n    return 1\n",
        "",
        "def build():\n    return 1\n\n__all__ = ['build']\n",
    );
    for source in [
        "__all__ = ['Item']\nclass Item: pass\n",
        "if enabled:\n    __all__ = ['build']\ndef build(): pass\n",
        "__all__ = []\n__all__.append('build')\ndef build(): pass\n",
    ] {
        let reports = common::check("module-all-at-bottom", source, "");
        assert!(!reports.as_array().unwrap().is_empty());
        assert!(reports[0]["fix"].is_null());
    }
    common::assert_clean(
        "module-all-at-bottom",
        "def build(): pass\n__all__ = []\n__all__.append('build')\n",
        "",
    );
}

#[test]
fn private_exports_aliases_and_configuration() {
    for source in [
        "EXPORTS = ['public']\nALIAS = EXPORTS\nALIAS += ['_hidden']\n__all__ = EXPORTS\n",
        "EXPORTS = ['public']\nEXPORTS.append('_hidden')\n__all__ = tuple(EXPORTS)\n",
        "__all__ = [*['_hidden']]\n",
        "__all__.insert(0, '_hidden')\n",
        "__all__ = ['_hid' 'den']\n",
    ] {
        assert_eq!(
            common::check("no-underscore-all-exports", source, "")
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    for source in [
        "EXPORTS = ['_hidden']\nALIAS = EXPORTS\nALIAS.clear()\n__all__ = EXPORTS\n",
        "EXPORTS = ('public',)\nALIAS = EXPORTS\nALIAS += ('_hidden',)\n__all__ = EXPORTS\n",
        "EXPORTS = ['_hidden']\nEXPORTS[0] = 'public'\n__all__ = EXPORTS\n",
        "def build():\n    __all__ = ['_hidden']\n",
        "__all__.append(['_hidden'])\n",
    ] {
        common::assert_clean("no-underscore-all-exports", source, "");
    }
    common::assert_clean(
        "no-underscore-all-exports",
        "__all__ = ['__version__', '_API']\n",
        "allow_dunder_exports = true\nallowed_exports = ['_API']",
    );
}

#[test]
fn imports_and_shadowed_constructors_invalidate_export_facts() {
    for source in [
        "EXPORTS = ['_hidden']\nfrom provider import EXPORTS\n__all__ = EXPORTS\n",
        "EXPORTS = ['_hidden']\ndef list(values): return []\n__all__ = list(EXPORTS)\n",
    ] {
        common::assert_clean("no-underscore-all-exports", source, "");
    }
    assert_eq!(
        common::check(
            "no-underscore-all-exports",
            "from builtins import list\nEXPORTS = ['_hidden']\n__all__ = list(EXPORTS)\n",
            ""
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    common::assert_fix(
        "module-all-at-bottom",
        "from __future__ import annotations\n\n__all__ = ['build']\n\ndef build(value: Item) -> Item:\n    return value\n",
        "",
        "from __future__ import annotations\n\ndef build(value: Item) -> Item:\n    return value\n\n__all__ = ['build']\n",
    );
}
