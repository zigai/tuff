#[path = "tuff/mod.rs"]
mod common;

#[test]
fn callable_bindings_follow_runtime_evaluation_order() {
    for source in [
        "import os\nos = os.remove('entry')\n",
        "import os\nos += os.remove('entry')\n",
        "import os\n(os := os.remove('entry'))\n",
        "import os\nfor os in [os.remove('entry')]:\n    pass\n",
        "print = print('entry')\n",
        "def print(value=print('entry')):\n    pass\n",
        "import os\nwith manager(os.remove('entry')) as os:\n    pass\n",
    ] {
        assert_eq!(
            count(
                "forbidden-call",
                source,
                "forbidden_calls = ['os.remove', 'builtins.print']"
            ),
            1,
            "{source}"
        );
    }
    assert_eq!(
        count(
            "no-unsafe-tempfile-factories",
            "import tempfile\ntempfile = tempfile.mkstemp()\n",
            ""
        ),
        1
    );
}

#[test]
fn deferred_writes_do_not_execute_during_scope_traversal() {
    for source in [
        "import os\ndef unused():\n    global os\n    os = fake\nos.remove('entry')\n",
        "def outer():\n    import os\n    def unused():\n        nonlocal os\n        os = fake\n    os.remove('entry')\n",
        "import os\nclass Container:\n    def unused():\n        global os\n        os = fake\nos.remove('entry')\n",
    ] {
        assert_eq!(
            count("forbidden-call", source, "forbidden_calls = ['os.remove']"),
            1,
            "{source}"
        );
    }
    assert_eq!(
        count(
            "forbidden-call",
            "import os\nclass Container:\n    global os\n    os = fake\nos.remove('entry')\n",
            "forbidden_calls = ['os.remove']"
        ),
        0
    );
}

fn count(rule: &str, source: &str, options: &str) -> usize {
    common::check(rule, source, options)
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn forbidden_calls_track_reaching_assignments_and_scopes() {
    let options = "forbidden_calls = ['os.remove']";
    for source in [
        "import os\nplatform = os\nplatform.remove('x')\n",
        "from os import remove\na: object = remove\na('x')\n",
        "import os\na = b = os.remove\nb('x')\n",
        "import os\n(a, other) = (os.remove, print)\na('x')\na = print\n",
        "import os\nif a := os.remove:\n    a('x')\n",
        "from os import *\nremove('x')\n",
        "import os\nos.remove('x')\nos = fake\n",
    ] {
        assert_eq!(count("forbidden-call", source, options), 1, "{source}");
    }
    for source in [
        "import os\nos = fake\nos.remove('x')\n",
        "import os\na = os.remove\ndef f(a):\n    a('x')\n",
        "from os import *\ndef f(remove):\n    remove('x')\n",
        "from .os import *\nremove('x')\n",
        "import os\nimport fake as os\nos.remove('x')\n",
    ] {
        common::assert_clean("forbidden-call", source, options);
    }
    let diagnostics = common::check(
        "forbidden-call",
        "import os\nos.remove('x')\n",
        "forbidden_calls = ['os.remove|Avoid deletion|pathlib.Path.unlink']",
    );
    assert_eq!(
        diagnostics[0]["message"],
        "Avoid deletion Use instead: pathlib.Path.unlink."
    );
    assert_eq!(
        count(
            "forbidden-call",
            "print('x')\n",
            "forbidden_calls = ['builtins.print']"
        ),
        1
    );
}

#[test]
fn forbidden_import_boundaries_and_precedence() {
    let options = "forbidden_imports = ['old', 'old.internal|Use public API']";
    for source in [
        "import old.internal as implementation\n",
        "from old import internal\n",
        "from old . internal import *\n",
    ] {
        let diagnostics = common::check("forbidden-import", source, options);
        assert_eq!(diagnostics[0]["message"], "Use public API");
    }
    common::assert_clean("forbidden-import", "import oldish.internal\n", options);
    assert_eq!(
        count(
            "forbidden-import",
            "import café\n",
            "forbidden_imports = ['café']"
        ),
        1
    );
}

#[test]
fn forbidden_names_cover_identifier_kinds() {
    for (source, kind) in [
        ("class Blocked: pass\n", "class"),
        ("def Blocked(): pass\n", "function"),
        ("def f(Blocked): pass\n", "parameter"),
        ("obj.Blocked = 1\n", "attribute"),
        ("import pkg.Blocked\n", "import"),
        ("from Blocked import x\n", "import"),
        ("import pkg as Blocked\n", "alias"),
        ("(Blocked, [*BlockedTail]) = things\n", "variable"),
        ("[x for Blocked in things]\n", "variable"),
        (
            "try:\n    run()\nexcept ValueError as Blocked:\n    pass\n",
            "variable",
        ),
        ("match x:\n    case [*Blocked]: pass\n", "variable"),
    ] {
        let expected = if source.contains("BlockedTail") { 2 } else { 1 };
        assert_eq!(
            count(
                "forbidden-name",
                source,
                &format!("forbidden_names = ['{kind}:Blocked*']")
            ),
            expected,
            "{source}"
        );
    }
    common::assert_clean(
        "forbidden-name",
        "def f(Blocked): pass\n",
        "forbidden_names = ['variable:Blocked']",
    );
}

#[test]
fn contracts_direct_imports_and_type_only_guards() {
    let options = "contracts = [{type = 'forbidden', name = 'boundary', source = ['**'], forbid = ['blocked'], message = 'Keep boundaries.'}]";
    for source in [
        "import blocked\n",
        "from blocked import member\n",
        "def f():\n    import blocked\n",
    ] {
        assert_eq!(count("import-contracts", source, options), 1);
    }
    for source in [
        "from typing import TYPE_CHECKING as TC\nif TC:\n    import blocked\n",
        "import typing as t\nif not t.TYPE_CHECKING:\n    pass\nelse:\n    import blocked\n",
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING and enabled:\n    import blocked\n",
    ] {
        common::assert_clean("import-contracts", source, options);
        assert_eq!(
            count(
                "import-contracts",
                source,
                &format!("{options}\nignore_type_checking = false")
            ),
            1
        );
    }
    for source in [
        "from typing import TYPE_CHECKING\ndef f(TYPE_CHECKING):\n    if TYPE_CHECKING:\n        import blocked\n",
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING or enabled:\n    import blocked\n",
    ] {
        assert_eq!(count("import-contracts", source, options), 1);
    }
    common::assert_clean("import-contracts", "import wrapper\n", options);
    common::assert_clean(
        "import-contracts",
        "import blocked\n",
        &format!("{options}\nsource_roots = ['not-the-source-root']"),
    );
}

#[test]
fn line_limits_include_decorators_and_exempt_nested_functions() {
    let source =
        "@first\n@second\ndef oversized():\n    def inner():\n        first()\n        second()\n";
    let diagnostics = common::check("line-count-limit", source, "max_function_lines = 2");
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        "Function 'oversized' has 6 lines, exceeding the configured limit of 2."
    );
    assert_eq!(
        count(
            "line-count-limit",
            "class C:\n    def f(self):\n        a()\n        b()\n",
            "max_method_lines = 2"
        ),
        1
    );
    assert_eq!(
        count("line-count-limit", "a()\nb()\nc()\n", "max_file_lines = 2"),
        1
    );
    common::assert_clean(
        "line-count-limit",
        source,
        "max_function_lines = 2\nglob_limits = {'*' = {max_function_lines = 20}}",
    );
}

#[test]
fn relative_imports_and_alias_ranges() {
    assert_eq!(count("no-relative-imports", "from ..x import y\n", ""), 1);
    common::assert_clean("no-relative-imports", "from x import y\n", "");
    let diagnostics = common::check("no-underscore-import-aliases", "import json as _json\n", "");
    assert_eq!(diagnostics[0]["location"]["column"], 16);
    assert_eq!(diagnostics[0]["end_location"]["column"], 21);
    common::assert_clean(
        "no-underscore-import-aliases",
        "from pkg import _private\n",
        "",
    );
}

#[test]
fn unsafe_tempfile_aliases_shadowing_and_exclusions() {
    let options = "excluded_path_parts = []";
    for source in [
        "import tempfile\ntempfile.mkstemp()\n",
        "from tempfile import *\na = mkdtemp\na()\n",
        "import tempfile\nt: object = tempfile\nt.mkstemp()\n",
        "import tempfile\n(t := tempfile).mkdtemp()\n",
        "from tempfile import mkstemp\n[a, b] = [mkstemp, print]\na()\n",
    ] {
        assert_eq!(
            count("no-unsafe-tempfile-factories", source, options),
            1,
            "{source}"
        );
    }
    for source in [
        "import tempfile\ntempfile = fake\ntempfile.mkstemp()\n",
        "from tempfile import *\ndef f(mkstemp):\n    mkstemp()\n",
        "import tempfile\nwith tempfile.TemporaryDirectory() as path:\n    pass\n",
    ] {
        common::assert_clean("no-unsafe-tempfile-factories", source, options);
    }
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    std::fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    for (path, options, expected) in [
        ("tests/example.py", "", 0),
        ("test_temp.py", "", 1),
        ("tests/example.py", "excluded_path_parts = []", 1),
        (
            "generated/example.py",
            "excluded_path_parts = ['generated']",
            0,
        ),
    ] {
        std::fs::write(directory.path().join("pyproject.toml"), format!("[tool.ruff.lint]\nselect=[]\n[tool.tuff.lint]\nenable=['no-unsafe-tempfile-factories']\n[tool.tuff.lint.rules.no-unsafe-tempfile-factories]\n{options}\n")).unwrap();
        let path = directory.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "import tempfile\ntempfile.mkstemp()\n").unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["check", "--no-cache", "--output-format", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if expected == 0 { 0 } else { 1 }),
            "{options}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reports.as_array().unwrap().len(),
            expected,
            "{}: {reports:#}",
            path.display()
        );
        if expected == 1 {
            assert_eq!(reports[0]["code"], "TUF008");
        }
    }
}

#[test]
fn contracts_layers_and_sibling_apis() {
    let layers = "contracts = [{type = 'layers', name = 'downwards', layers = [{modules = ['edge.**']}, {modules = ['**']}]}]";
    assert_eq!(
        count("import-contracts", "from edge import Handler\n", layers),
        1
    );
    common::assert_clean("import-contracts", "import peer.storage\n", layers);
    let rooted = "contracts = [{type = 'layers', name = 'rooted', root = 'unrelated', layers = [{modules = ['edge.**']}, {modules = ['**']}]}]";
    common::assert_clean(
        "import-contracts",
        "from unrelated.edge import Handler\n",
        rooted,
    );
    let siblings = "contracts = [{type = 'independence', name = 'siblings', packages = ['*'], public = ['api']} ]";
    assert_eq!(
        count(
            "import-contracts",
            "import unrelated_sibling.storage\n",
            siblings
        ),
        1
    );
    common::assert_clean(
        "import-contracts",
        "from unrelated_sibling.api import Item\n",
        siblings,
    );
    let source_exception = "contracts = [{type = 'forbidden', name = 'exception', source = ['**'], forbid = ['blocked'], allow = ['**']}]";
    common::assert_clean("import-contracts", "import blocked\n", source_exception);
    let assignment_not_import =
        "from typing import TYPE_CHECKING\nTC = TYPE_CHECKING\nif TC:\n    import blocked\n";
    assert_eq!(
        count(
            "import-contracts",
            assignment_not_import,
            "contracts = [{type = 'forbidden', name = 'runtime', source = ['**'], forbid = ['blocked']} ]"
        ),
        1
    );
}

#[test]
fn replacement_callable_is_advice_not_an_automatic_fix() {
    let diagnostics = common::check(
        "forbidden-call",
        "import legacy\nimport modern\nlegacy.run('x')\n",
        "forbidden_calls = ['legacy.run||modern.run']",
    );
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        "Do not call forbidden callable 'legacy.run'. Use instead: modern.run."
    );
    assert!(diagnostics[0]["fix"].is_null());
}

#[test]
fn source_order_decorators_and_with_contexts() {
    assert_eq!(
        count(
            "forbidden-call",
            "from pkg import decorator\n@decorator()\ndef decorator(): pass\n",
            "forbidden_calls = ['pkg.decorator']"
        ),
        1
    );
    assert_eq!(
        count(
            "forbidden-call",
            "from pkg import manager\nwith manager() as manager:\n    pass\n",
            "forbidden_calls = ['pkg.manager']"
        ),
        1
    );
}

#[test]
fn plain_callable_names_do_not_override_local_alias_provenance() {
    common::assert_clean(
        "forbidden-call",
        "alias = print\nalias('x')\n",
        "forbidden_calls = ['alias']",
    );
    common::assert_clean(
        "forbidden-call",
        "alias = print\nalias('x')\n",
        "forbidden_calls = ['print']",
    );
    assert_eq!(
        count(
            "forbidden-call",
            "alias = print\nalias('x')\n",
            "forbidden_calls = ['builtins.print']"
        ),
        1
    );
    common::assert_clean(
        "forbidden-call",
        "from tempfile import *\nalias = mkstemp\nalias()\n",
        "forbidden_calls = ['tempfile.mkstemp']",
    );
}

#[test]
fn deferred_global_and_nonlocal_assignments_do_not_create_aliases() {
    assert_eq!(
        count(
            "forbidden-call",
            "import os\ndef setup():\n    global delete\n    delete = os.remove\ndelete('x')\n",
            "forbidden_calls = ['os.remove']"
        ),
        0
    );
    assert_eq!(
        count(
            "forbidden-call",
            "import os\ndef outer():\n    delete = print\n    def setup():\n        nonlocal delete\n        delete = os.remove\n    delete('x')\n",
            "forbidden_calls = ['os.remove']"
        ),
        0
    );
}

#[test]
fn canonical_callable_matching_precedes_plain_names_in_either_order() {
    for options in [
        "forbidden_calls = ['remove|plain', 'os.remove|qualified']",
        "forbidden_calls = ['os.remove|qualified', 'remove|plain']",
    ] {
        let diagnostics = common::check(
            "forbidden-call",
            "from os import remove\nremove('x')\n",
            options,
        );
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert_eq!(diagnostics[0]["message"], "qualified");
    }
    for options in [
        "forbidden_calls = ['print|plain', 'builtins.print|qualified']",
        "forbidden_calls = ['builtins.print|qualified', 'print|plain']",
    ] {
        let diagnostics = common::check("forbidden-call", "print('x')\n", options);
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert_eq!(diagnostics[0]["message"], "qualified");
    }
    let diagnostics = common::check(
        "forbidden-call",
        "print('x')\n",
        "forbidden_calls = ['print|plain']",
    );
    assert_eq!(diagnostics[0]["message"], "plain");
}

#[test]
fn forbidden_import_names_keep_complete_unicode_identifiers() {
    for name in ["a\u{203f}b", "a\u{1dc0}b"] {
        // Connector punctuation and a combining mark outside U+0300-036F.
        let diagnostics = common::check(
            "forbidden-name",
            &format!("import {name}\n"),
            "forbidden_names = ['import:*']",
        );
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert_eq!(
            diagnostics[0]["message"],
            format!("Do not use forbidden import name '{name}'.")
        );
        assert_eq!(diagnostics[0]["location"]["column"], 8);
        assert_eq!(diagnostics[0]["end_location"]["column"], 11);
        assert_eq!(
            count(
                "forbidden-name",
                &format!("from pkg . {name} import item\n"),
                "forbidden_names = ['import:*']"
            ),
            3
        );
    }
    assert_eq!(
        count(
            "forbidden-name",
            "import match.case.type\n",
            "forbidden_names = ['import:*']"
        ),
        3
    );
}

#[test]
fn function_and_method_limits_count_python_newline_conventions() {
    for newline in ["\n", "\r\n", "\r"] {
        let source = ["@decorator", "def f():", "    first()", "    second()", ""].join(newline);
        let diagnostics = common::check("line-count-limit", &source, "max_function_lines = 3");
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert_eq!(
            diagnostics[0]["message"],
            "Function 'f' has 4 lines, exceeding the configured limit of 3."
        );
        let source = [
            "class C:",
            "    def f(self):",
            "        first()",
            "        second()",
            "",
        ]
        .join(newline);
        let diagnostics = common::check("line-count-limit", &source, "max_method_lines = 2");
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert_eq!(
            diagnostics[0]["message"],
            "Method 'f' has 3 lines, exceeding the configured limit of 2."
        );
    }
}

#[test]
fn whole_file_limit_highlights_the_first_line() {
    for newline in ["\n", "\r\n", "\r"] {
        let diagnostics = common::check(
            "line-count-limit",
            &format!("first(){newline}second(){newline}"),
            "max_file_lines = 1",
        );
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        assert_eq!(diagnostics[0]["location"]["row"], 1);
        assert_eq!(diagnostics[0]["location"]["column"], 1);
        assert_eq!(diagnostics[0]["end_location"]["row"], 2);
        assert_eq!(diagnostics[0]["end_location"]["column"], 1);
    }
}

#[test]
fn callable_resolution_does_not_flatten_calls_or_subscripts() {
    common::assert_clean(
        "forbidden-call",
        "import os\nos.remove[0]('x')\n",
        "forbidden_calls = ['os.remove']",
    );
    assert_eq!(
        count(
            "forbidden-call",
            "import os\nos.remove('x')()\n",
            "forbidden_calls = ['os.remove']"
        ),
        1
    );
    common::assert_clean(
        "forbidden-call",
        "import os\nimport fake as os\ndef f():\n    os.remove('x')\n",
        "forbidden_calls = ['os.remove']",
    );
    common::assert_clean(
        "no-unsafe-tempfile-factories",
        "from tempfile import mkstemp\nmkstemp[0]()\n",
        "excluded_path_parts = []",
    );
}

#[test]
fn call_resolution_uses_completed_python_scopes() {
    let options = "forbidden_calls = ['os.remove']";
    assert_eq!(
        count(
            "forbidden-call",
            "def run():\n    os.remove('value')\nimport os\n",
            options
        ),
        1
    );
    assert_eq!(
        count(
            "forbidden-call",
            "import os\ndef run():\n    alias.remove('value')\nalias = os\n",
            options
        ),
        1
    );
    common::assert_clean(
        "forbidden-call",
        "import os\ndef run():\n    os.remove('value')\nimport other as os\n",
        options,
    );
    assert_eq!(
        count("forbidden-call", "os.remove('value')\nimport os\n", options),
        1
    );
}
