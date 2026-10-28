#[path = "tuff/mod.rs"]
mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

const OPTIONS: &str = "forbidden_calls = ['builtins.print']";

#[test]
fn spacing_ownership_is_independent_of_fix_configuration() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let path = directory.path().join("input.py");
    for unfixable in ["[]", "['TUF108']", "['E303']", "['ALL']"] {
        fs::write(directory.path().join("pyproject.toml"), format!("[tool.ruff]\npreview = true\n[tool.ruff.lint]\nselect = ['E303', 'TUF108']\nunfixable = {unfixable}\n")).unwrap();
        fs::write(&path, "def run():\n\n\n\n    first()\n\n\n\n    second()\n").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["check", "--no-cache", "--output-format", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut codes: Vec<_> = reports
            .as_array()
            .unwrap()
            .iter()
            .map(|report| report["code"].as_str().unwrap())
            .collect();
        codes.sort_unstable();
        assert_eq!(codes, ["E303", "TUF108"], "{unfixable}: {reports:#}");
    }
}

#[test]
fn own_line_comments_support_named_ignores() {
    for source in [
        "# ruff: noqa  # tuff: ignore[use-tuff-ignore-comment]\nvalue = 1\n",
        "# tuff: ignore[use-tuff-ignore-comment]\n# ruff: noqa\nvalue = 1\n",
        "# ruff: noqa  # tuff: ignore[use-tuff-ignore-comment]",
    ] {
        common::assert_clean("use-tuff-ignore-comment", source, "");
    }
    assert_eq!(
        common::check(
            "use-tuff-ignore-comment",
            "# ruff: noqa  # tuff: ignore[forbidden-call]\nvalue = 1\n",
            ""
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn upstream_comment_ignores_stay_with_the_target_comment() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(
        directory.path().join("pyproject.toml"),
        "[tool.ruff.lint]\nselect = ['FIX002', 'RUF100']\n",
    )
    .unwrap();
    let path = directory.path().join("input.py");
    for (source, expected) in [
        (
            "# TODO: later  # tuff: ignore[line-contains-todo]\nvalue = 1\n",
            0,
        ),
        (
            "# tuff: ignore[line-contains-todo]\n# TODO: later\nvalue = 1\n",
            0,
        ),
        ("# TODO: later  # tuff: ignore[line-contains-todo]", 0),
        (
            "# TODO: later  # tuff: ignore[forbidden-call]\nvalue = 1\n",
            1,
        ),
        (
            "# tuff: ignore[line-contains-todo]\n# unrelated\n# TODO: later\nvalue = 1\n",
            1,
        ),
        (
            "# tuff: ignore[line-contains-todo]\ndef run():\n    # TODO: later\n    pass\n",
            1,
        ),
    ] {
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["check", "--no-cache", "--output-format", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if expected == 0 { 0 } else { 1 }),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reports.as_array().unwrap().len(),
            expected,
            "{source}: {reports:#}"
        );
        if expected != 0 {
            assert_eq!(reports[0]["code"], "FIX002");
        }
    }
}

#[test]
fn replacements_follow_native_activity_and_per_file_ignores() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let path = directory.path().join("input.py");
    for (config, source, code) in [
        (
            "[tool.ruff.lint]\nselect = ['F541', 'TUF206']\n",
            "value = f'text'\n",
            "TUF206",
        ),
        (
            "[tool.ruff.lint]\nselect = ['F541', 'TUF206']\nper-file-ignores = {'input.py' = ['TUF206']}\n",
            "value = f'text'\n",
            "F541",
        ),
        (
            "[tool.ruff.lint]\nselect = ['F541']\n[tool.tuff.lint]\nenable = ['no-redundant-f-string']\ndisable = ['no-redundant-f-string']\n",
            "value = f'text'\n",
            "F541",
        ),
        (
            "[tool.ruff]\ntarget-version = 'py39'\n[tool.ruff.lint]\nselect = ['UP035', 'TUF019']\n",
            "from typing import List\n",
            "UP035",
        ),
        (
            "[tool.ruff.lint]\nselect = ['TID251', 'TUF001', 'TUF002']\n[tool.ruff.lint.flake8-tidy-imports.banned-api]\n'blocked.module' = {msg = 'avoid this import'}\n",
            "import blocked.module\n",
            "TID251",
        ),
    ] {
        fs::write(directory.path().join("pyproject.toml"), config).unwrap();
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["check", "--no-cache", "--output-format", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{config}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reports.as_array().unwrap().len(),
            1,
            "{config}: {reports:#}"
        );
        assert_eq!(reports[0]["code"], code);
    }
}

#[test]
fn spacing_replacements_preserve_unrelated_upstream_reports() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let path = directory.path().join("input.py");
    let source = "def run():\n\n\n\n    first()\n\n\n\n    second()\n\n\n";
    for (ignore, expected) in [
        ("", vec!["E303", "TUF108", "TUF108"]),
        (
            "per-file-ignores = {'input.py' = ['TUF108']}\n",
            vec!["E303", "E303", "W391"],
        ),
    ] {
        fs::write(directory.path().join("pyproject.toml"), format!("[tool.ruff]\npreview = true\n[tool.ruff.lint]\nselect = ['E303', 'W391', 'TUF108']\n{ignore}")).unwrap();
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["check", "--no-cache", "--output-format", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut codes: Vec<_> = reports
            .as_array()
            .unwrap()
            .iter()
            .map(|report| report["code"].as_str().unwrap())
            .collect();
        codes.sort_unstable();
        assert_eq!(codes, expected);
    }
}

#[test]
fn inline_named_ignore() {
    common::assert_clean(
        "forbidden-call",
        "print('value')  # tuff: ignore[forbidden-call]\n",
        OPTIONS,
    );
}

#[test]
fn canonical_names_suppress_upstream_checks() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let path = directory.path().join("input.py");
    for (source, expected) in [
        ("import unused  # tuff: ignore[unused-import]\n", 0),
        ("# tuff: ignore[unused-import]\nimport unused\n", 0),
        ("import unused  # tuff: ignore[unused-variable]\n", 1),
        ("import unused  # noqa: F401\n", 0),
        ("import unused  # tuff: ignore\n", 0),
    ] {
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args([
                "check",
                "--isolated",
                "--select",
                "F401,RUF100",
                "--output-format",
                "json",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reports.as_array().unwrap().len(),
            expected,
            "{source}: {reports:#}"
        );
    }
}

#[test]
fn inline_suite_ignore_covers_its_header() {
    common::assert_clean(
        "no-underscore-class",
        "class _Hidden: ...  # tuff: ignore[no-underscore-class]\n",
        "",
    );
    common::assert_clean(
        "forbidden-name",
        "class Hidden: ...  # tuff: ignore[forbidden-name]\n",
        "forbidden_names = ['class:Hidden']",
    );
    common::assert_clean(
        "forbidden-call",
        "if flag: print('value')  # tuff: ignore[forbidden-call]\n",
        OPTIONS,
    );
}

#[test]
fn leading_ignore_applies_to_next_statement() {
    let diagnostics = common::check(
        "forbidden-call",
        "# tuff: ignore[forbidden-call]\nprint('first')\nprint('second')\n",
        OPTIONS,
    );
    assert_eq!(diagnostics.as_array().map(Vec::len), Some(1));
    assert_eq!(diagnostics[0]["location"]["row"], 3);
}

#[test]
fn header_ignore_does_not_ignore_function_body() {
    let diagnostics = common::check(
        "forbidden-call",
        "def run():  # tuff: ignore[forbidden-call]\n    print('value')\n",
        OPTIONS,
    );
    assert_eq!(diagnostics.as_array().map(Vec::len), Some(1));
}

#[test]
fn blanket_ignore_and_standard_noqa() {
    common::assert_clean(
        "forbidden-call",
        "print('value')  # tuff: ignore\n",
        OPTIONS,
    );
    common::assert_clean(
        "forbidden-call",
        "print('value')  # noqa: TUF001\n",
        OPTIONS,
    );
    common::assert_clean(
        "forbidden-call",
        "# ruff: noqa: TUF001\nprint('value')\n",
        OPTIONS,
    );
}

#[test]
fn different_rule_name_does_not_suppress() {
    let diagnostics = common::check(
        "forbidden-call",
        "print('value')  # tuff: ignore[forbidden-import]\n",
        OPTIONS,
    );
    assert_eq!(diagnostics.as_array().map(Vec::len), Some(1));
}

#[test]
fn unknown_ignore_name_is_reported() {
    let diagnostics = common::check(
        "forbidden-call",
        "print('value')  # tuff: ignore[unknown-rule]\n",
        OPTIONS,
    );
    assert_eq!(diagnostics.as_array().map(Vec::len), Some(2));
    assert!(
        diagnostics
            .as_array()
            .is_some_and(
                |diagnostics| diagnostics.iter().any(|diagnostic| diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("unknown-rule")))
            )
    );
}

#[test]
fn malformed_ignore_does_not_suppress() {
    let diagnostics = common::check(
        "forbidden-call",
        "print('value')  # tuff: ignore[forbidden-call\n",
        OPTIONS,
    );
    assert_eq!(diagnostics.as_array().map(Vec::len), Some(1));
}
