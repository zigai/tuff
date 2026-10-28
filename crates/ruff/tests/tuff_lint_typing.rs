#[path = "tuff/mod.rs"]
mod common;

use std::fmt::Write as _;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn typing_replacements_require_available_unshadowed_names() {
    for source in [
        "from typing import List\nvalue: [list[int] for List in [str]]\n",
        "from typing import List\ndef run(value: (list[int] for List in [str])): pass\n",
        "from typing import List\nvalue: (lambda List: list[int])(str)\n",
        "value: list[int]\nfrom typing import List\n",
        "from typing import List\ntry: pass\nexcept Exception as List:\n    value: list[int]\n",
        "from __future__ import annotations\nfrom typing import List\ndef run(List):\n    value: list[int]\n",
    ] {
        let reports = check_at("TUF019", source, "module.py", "py38");
        assert_eq!(
            reports.as_array().unwrap().len(),
            1,
            "{source}: {reports:#}"
        );
        assert!(reports[0]["fix"].is_null(), "{source}: {reports:#}");
    }
    let reports = check_at(
        "TUF019",
        "from typing import List\nvalue: list[int]\n",
        "module.py",
        "py38",
    );
    assert_eq!(reports[0]["fix"]["applicability"], "safe");
    let reports = check_at(
        "TUF019",
        "from typing import List\ndef run[List](value: list[int]): pass\n",
        "module.py",
        "py38",
    );
    let reports = reports.as_array().unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0]["code"], "invalid-syntax");
    assert_eq!(reports[1]["code"], "TUF019");
    assert!(reports[1]["fix"].is_null());
}

#[test]
fn compound_quoted_object_annotations_resolve_nested_names() {
    for source in [
        "class object: pass\nvalue: 'object | None'\n",
        "value: 'object | None'\nclass object: pass\n",
        "from typing import Optional\nvalue: 'Optional[object]'\nclass object: pass\n",
        "from typing import Optional\nclass object: pass\nvalue: 'Optional[object]'\n",
        "from typing import Optional\ndef run[object](value: 'Optional[object]'): pass\n",
        "from typing import Optional, TypeAlias\nObject: TypeAlias = object\nclass Container:\n    Object = int\n    def run(self, value: 'Optional[Object]'): pass\n",
    ] {
        common::assert_clean("no-bare-object-annotations", source, "");
    }
    for source in [
        "from typing import Optional, TypeAlias\nObject: TypeAlias = object\nvalue: 'Optional[Object]'\n",
        "from typing import Optional, TypeAlias\nObject: TypeAlias = object\nclass Container:\n    Object = int\n    def run(self):\n        value: 'Optional[Object]'\n",
        "value: 'object | None'\n",
    ] {
        assert_eq!(
            common::check("no-bare-object-annotations", source, "")
                .as_array()
                .unwrap()
                .len(),
            1,
            "{source}"
        );
    }
    common::assert_clean(
        "no-bare-object-annotations",
        "from typing import Optional, TypeAlias\nObject: TypeAlias = 'Optional[Object]'\nvalue: 'Optional[Object]'\n",
        "",
    );
}

#[test]
fn object_annotations_and_exemptions() {
    for annotation in [
        "object",
        "object | None",
        "None | object",
        "'object'",
        "'object' ''",
        "Optional[object]",
        "Union[object, None]",
        "Annotated[object, 'metadata']",
    ] {
        let source =
            format!("from typing import Optional, Union, Annotated\nvalue: {annotation}\n");
        assert_eq!(
            common::check("no-bare-object-annotations", &source, "")
                .as_array()
                .unwrap()
                .len(),
            1,
            "{annotation}"
        );
    }
    common::assert_clean(
        "no-bare-object-annotations",
        "def f(value: dict[str, object]) -> type[object]: pass\n",
        "",
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "class Item:\n    def __eq__(self, other: object, /) -> bool: pass\n    def __ne__(self, o: object) -> bool: pass\n",
        "",
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "class object: pass\nvalue: 'object'\n",
        "",
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "from other import Optional\nvalue: Optional[object]\n",
        "",
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "from typing import Literal\nvalue: Literal[object]\n",
        "",
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "from typing import Optional\nO = Optional\nvalue: O[object]\n",
        "",
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "from typing import TypeAlias\nObject: TypeAlias = object\nvalue: Object\nclass Object: pass\n",
        "",
    );
}

#[test]
fn object_aliases() {
    for source in [
        "from typing import TypeAlias\nObject: TypeAlias = object\nOther: TypeAlias = Object\nvalue: Other\n",
        "type Object = object | None\nvalue: 'Object'\n",
        "import builtins as b\nfrom typing import Optional as O\nvalue: O[b.object]\n",
        "value: \"'ob' 'ject'\"\n",
    ] {
        assert_eq!(
            common::check("no-bare-object-annotations", source, "")
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn callable_aliases_and_comment_preservation() {
    for prefix in [
        "from typing import Callable\n",
        "from typing_extensions import Callable\n",
        "from collections.abc import Callable\n",
        "from typing import *\n",
    ] {
        let source = format!("{prefix}C = Other = Callable\nvalue: Other[[...], int]\n");
        let expected = format!("{prefix}C = Other = Callable\nvalue: Other[..., int]\n");
        // Star-import names are recognized directly; assignment aliases follow explicit bindings.
        if !prefix.contains('*') {
            common::assert_fix("use-callable-ellipsis", &source, "", &expected);
        }
    }
    common::assert_fix(
        "use-callable-ellipsis",
        "from typing import *\nvalue: Callable[[...], int]\n",
        "",
        "from typing import *\nvalue: Callable[..., int]\n",
    );
    let diagnostics = common::check(
        "use-callable-ellipsis",
        "from typing import Callable\nvalue: Callable[[\n ... # retained\n], int]\n",
        "",
    );
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert!(diagnostics[0]["fix"].is_null());
    common::assert_clean(
        "use-callable-ellipsis",
        "import typing\ntyping = custom\nvalue: typing.Callable[[...], int]\n",
        "",
    );
    common::assert_clean(
        "use-callable-ellipsis",
        "from typing import Callable\nC = Callable\nclass C: pass\nvalue: C[[...], int]\n",
        "",
    );
    common::assert_clean(
        "use-callable-ellipsis",
        "from typing import Callable\nvalue: Callable[[int, ...], int]\n",
        "",
    );
    common::assert_fix(
        "use-callable-ellipsis",
        "from typing import Callable\n(C,) = (Callable,)\nvalue: C[[...], int]\n",
        "",
        "from typing import Callable\n(C,) = (Callable,)\nvalue: C[..., int]\n",
    );
}

#[test]
fn modern_python_allows_builtin_generics() {
    common::assert_clean("use-types-from-typing", "value: list[str]\n", "");
}

fn check_at(code: &str, source: &str, filename: &str, target: &str) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tuff"))
        .args([
            "check",
            "--isolated",
            "--select",
            code,
            "--target-version",
            target,
            "--output-format",
            "json",
            "--stdin-filename",
            filename,
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn python_38_generic_annotations_and_available_fixes() {
    for source in [
        "value: list[str]\n",
        "import builtins\nvalue: builtins.dict[str, str]\n",
        "from builtins import list as L\n(Alias,) = (L,)\nvalue: Alias[str]\nAlias = Other\n",
    ] {
        assert_eq!(
            check_at("TUF019", source, "model.py", "py38")
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            check_at("TUF019", source, "model.py", "py39")
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
    let diagnostics = check_at(
        "TUF019",
        "from typing import List\nvalue: list[str]\n",
        "model.py",
        "py38",
    );
    assert!(!diagnostics[0]["fix"].is_null());
    for source in [
        "from other import List\nvalue: list[str]\n",
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from typing import List\nvalue: list[str]\n",
        "from typing import List\nvalue: list[str]\nList = Other\n",
    ] {
        let diagnostics = check_at("TUF019", source, "model.py", "py38");
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert!(diagnostics[0]["fix"].is_null());
    }
    for source in [
        "from custom import *\nvalue: list[str]\n",
        "value = list[str]\n",
        "from other import list\nvalue: list[str]\n",
    ] {
        assert_eq!(
            check_at("TUF019", source, "model.py", "py38")
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
}

#[test]
fn typing_fixes_respect_exception_and_comprehension_bindings() {
    let diagnostics = check_at(
        "TUF019",
        "from typing import List\ntry: pass\nexcept Exception as List:\n    value: list[int]\n",
        "model.py",
        "py38",
    );
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert!(diagnostics[0]["fix"].is_null());
    let diagnostics = check_at(
        "TUF019",
        "from typing import List\nunused = [List for List in values]\nvalue: list[int]\n",
        "model.py",
        "py38",
    );
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert!(!diagnostics[0]["fix"].is_null());
}

#[test]
fn quoted_annotations_follow_python_scopes() {
    common::assert_clean(
        "no-bare-object-annotations",
        "try: pass\nexcept Exception as object:\n    value: 'object'\n",
        "",
    );
    for source in [
        "def run[object](value: 'object'): pass\n",
        "class Container[object]:\n    value: 'object'\n",
        "type Wrapper[object] = 'object'\n",
    ] {
        common::assert_clean("no-bare-object-annotations", source, "");
    }
    for source in [
        "unused = [object for object in values]\nvalue: 'object'\n",
        "from typing import TypeAlias\nObject: TypeAlias = object\nclass Container:\n    Object = int\n    def method(self):\n        value: 'Object'\n",
    ] {
        assert_eq!(
            common::check("no-bare-object-annotations", source, "")
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn explicit_object_aliases_have_no_depth_limit_and_reject_cycles() -> std::fmt::Result {
    let mut source = String::from("from typing import TypeAlias\nAlias0: TypeAlias = object\n");
    for index in 1..=40 {
        writeln!(source, "Alias{index}: TypeAlias = Alias{}", index - 1)?;
    }
    source.push_str("value: Alias40\n");
    assert_eq!(
        common::check("no-bare-object-annotations", &source, "")
            .as_array()
            .unwrap()
            .len(),
        1
    );
    common::assert_clean(
        "no-bare-object-annotations",
        "type First = Second\ntype Second = First\nvalue: First\n",
        "",
    );
    Ok(())
}

#[test]
fn object_annotations_skip_test_paths() {
    for path in ["tests/model.py", "test_model.py", "model_test.py"] {
        assert_eq!(
            check_at("TUF017", "value: object\n", path, "py313")
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
}

#[test]
fn object_annotations_do_not_unwrap_calls_or_subscripts() {
    for source in [
        "import builtins\nvalue: builtins.object()\n",
        "import builtins\nvalue: builtins.object[int]\n",
        "value: 'builtins.object()'\n",
        "value: 'builtins.object[int]'\n",
    ] {
        common::assert_clean("no-bare-object-annotations", source, "");
    }
}
