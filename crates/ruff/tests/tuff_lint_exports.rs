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

fn module_all_reports(files: &[(&str, &str)], options: &str) -> Vec<String> {
    common::check_project("respect-module-all", files, options, "app/main.py")
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["message"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn configured_module_exports_limit_imported_names() {
    let options = "module_exports = ['pkg=exported', 'pkg.module=exported']";
    for (source, name, module) in [
        ("from pkg import hidden\n", "hidden", "pkg"),
        ("from pkg import exported, hidden\n", "hidden", "pkg"),
        (
            "from pkg.module import hidden as alias\n",
            "hidden",
            "pkg.module",
        ),
    ] {
        let reports = module_all_reports(&[("app/main.py", source)], options);
        assert_eq!(
            reports,
            [format!(
                "Do not import '{name}' from '{module}'; that module defines __all__ and does not export it."
            )]
        );
    }
    for source in [
        "from pkg import exported\n",
        "from pkg import exported as alias\n",
        "from pkg import *\n",
        "from other import hidden\n",
        "from .pkg import hidden\n",
    ] {
        assert!(
            module_all_reports(&[("app/main.py", source)], options).is_empty(),
            "{source}"
        );
    }
}

#[test]
fn local_modules_are_checked_against_their_static_all() {
    let main = "from service.api import build, helper\n";
    for api in [
        "__all__ = ['build']\n",
        "__all__: list[str] = ('build',)\n",
        "__all__ = []\n__all__ += ['build']\n",
        "__all__ = []\n__all__.append('build')\n",
        "__all__ = []\n__all__.extend({'build'})\n",
    ] {
        let reports = module_all_reports(&[("app/main.py", main), ("service/api.py", api)], "");
        assert_eq!(
            reports,
            [
                "Do not import 'helper' from repo-local module 'service.api'; either stop importing it or add it to that module's __all__ exports."
            ],
            "{api}"
        );
    }
    for api in [
        "def build(): pass\n",
        "__all__ = ['build'] + EXTRA\n",
        "__all__ = ['build']\n__all__[0] = 'helper'\n",
        "__all__ = ['build']\n__all__.insert(0, 'helper')\n",
        "__all__ = ['build']\ndel __all__\n",
        "__all__ = ['build']\n__all__ += names()\n",
        "__all__ = ['build', 'helper']\n",
        "__all__ = [\n",
    ] {
        let reports = module_all_reports(&[("app/main.py", main), ("service/api.py", api)], "");
        assert!(reports.is_empty(), "{api}: {reports:?}");
    }
}

#[test]
fn local_modules_resolve_packages_and_configured_source_roots() {
    let main = "from service import hidden\n";
    let package = ("src/service/__init__.py", "__all__ = ['public']\n");
    assert!(module_all_reports(&[("app/main.py", main), package], "").is_empty());
    assert_eq!(
        module_all_reports(
            &[("app/main.py", main), package],
            "source_roots = ['lib', 'src']"
        )
        .len(),
        1
    );
    let configured = module_all_reports(
        &[("app/main.py", main), package],
        "source_roots = ['src']\nmodule_exports = ['service=hidden']",
    );
    assert!(configured.is_empty());
    let shadowed_module = module_all_reports(
        &[
            ("app/main.py", main),
            ("service/__init__.py", "__all__ = ['hidden']\n"),
            ("service.py", "__all__ = ['public']\n"),
        ],
        "",
    );
    assert!(shadowed_module.is_empty());
    for path in [
        "tests/main.py",
        "app/test_main.py",
        "app/main_test.py",
        "conftest.py",
    ] {
        let reports = common::check_project(
            "respect-module-all",
            &[(path, main), package],
            "source_roots = ['src']",
            path,
        );
        assert_eq!(reports.as_array().unwrap().len(), 0, "{path}");
    }
}
