use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::{TempDir, tempdir_in};

struct Case {
    directory: TempDir,
    source: PathBuf,
}

impl Case {
    fn new(rule: &str, source: &str, options: &str) -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
        fs::create_dir_all(&base).unwrap();
        let directory = tempdir_in(base).unwrap();
        let config = format!(
            "[tool.ruff]\ntarget-version = 'py313'\n[tool.ruff.lint]\nselect = []\n[tool.tuff.lint]\nenable = [{rule:?}]\n[tool.tuff.lint.rules.{rule}]\n{options}\n"
        );
        fs::write(directory.path().join("pyproject.toml"), config).unwrap();
        let path = directory.path().join("input.py");
        fs::write(&path, source).unwrap();
        Self {
            directory,
            source: path,
        }
    }

    fn run(&self, flags: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tuff"));
        command.current_dir(self.directory.path()).args([
            "check",
            "--no-cache",
            "--output-format",
            "json",
        ]);
        command.args(flags);
        let output = command.arg(&self.source).output().unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "Tuff failed: {}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        output
    }
}

pub(crate) fn check(rule: &str, source: &str, options: &str) -> Value {
    let case = Case::new(rule, source, options);
    serde_json::from_slice(&case.run(&[]).stdout).unwrap()
}

pub(crate) fn assert_clean(rule: &str, source: &str, options: &str) {
    let diagnostics = check(rule, source, options);
    assert_eq!(
        diagnostics.as_array().map(Vec::len),
        Some(0),
        "{rule}: {source}\n{diagnostics:#}"
    );
}

#[allow(
    dead_code,
    reason = "This helper is used by the integration test targets that exercise fixable native rules."
)]
pub(crate) fn assert_fix(rule: &str, source: &str, options: &str, expected: &str) {
    let case = Case::new(rule, source, options);
    case.run(&["--fix", "--unsafe-fixes"]);
    assert_eq!(
        fs::read_to_string(&case.source).unwrap(),
        expected,
        "{rule}"
    );
    let diagnostics: Value = serde_json::from_slice(&case.run(&[]).stdout).unwrap();
    assert_eq!(
        diagnostics.as_array().map(Vec::len),
        Some(0),
        "fix did not converge for {rule}: {diagnostics:#}"
    );
    case.run(&["--fix", "--unsafe-fixes"]);
    assert_eq!(
        fs::read_to_string(&case.source).unwrap(),
        expected,
        "second fix changes {rule}"
    );
}

#[allow(
    dead_code,
    reason = "Only integration targets for rules with unsafe fixes use this helper."
)]
pub(crate) fn assert_unsafe_fix(rule: &str, source: &str) {
    let case = Case::new(rule, source, "");
    let diagnostics: Value = serde_json::from_slice(&case.run(&[]).stdout).unwrap();
    assert_eq!(
        diagnostics.as_array().unwrap().len(),
        1,
        "{rule}: {diagnostics:#}"
    );
    assert_eq!(
        diagnostics[0]["fix"]["applicability"], "unsafe",
        "{rule}: {diagnostics:#}"
    );
    case.run(&["--fix"]);
    assert_eq!(
        fs::read_to_string(&case.source).unwrap(),
        source,
        "safe fix changes {rule}"
    );
    case.run(&["--fix"]);
    assert_eq!(
        fs::read_to_string(&case.source).unwrap(),
        source,
        "second safe fix changes {rule}"
    );
}
