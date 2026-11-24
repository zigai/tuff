use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ruff_python_ast::Expr;
use ruff_python_ast::token::TokenKind;
use ruff_python_parser::parse_module;
use ruff_python_trivia::CommentRanges;
use ruff_text_size::Ranged;
use tempfile::TempDir;

use crate::{CheckedFile, FileReport, PluginConfig, Plugins, PythonRules};

const PRINT_RULE: &str = r#"
import ast
from tuff_rules import TuffRule


class NoPrint(TuffRule):
    name = "no-print"
    source_patterns = ("print",)
    node_kinds = ("Call",)

    def check(self, tree, ctx):
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and ctx.qualified_name(node.func) == "builtins.print":
                ctx.report(node.func, "print call", [ctx.edit(node.func, ctx.options.get("to", "log"))])
"#;

struct Project {
    directory: TempDir,
}

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/python-rules-tests");
        fs::create_dir_all(&base).unwrap();
        let project = Self {
            directory: tempfile::tempdir_in(base).unwrap(),
        };
        for (path, contents) in files {
            project.write(path, contents);
        }
        project
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn config(&self) -> PluginConfig {
        PluginConfig {
            plugin_dirs: vec![self.root().join("rules")],
            python: Some(self.python()),
            workers: None,
        }
    }

    #[cfg(unix)]
    fn python(&self) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let wrapper = self.root().join("python");
        if !wrapper.exists() {
            let python = which::which("python3").unwrap();
            fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\necho started >> '{}'\nexec '{}' \"$@\"\n",
                    self.root().join("spawns.log").display(),
                    python.display()
                ),
            )
            .unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        }
        wrapper
    }

    #[cfg(not(unix))]
    fn python(&self) -> PathBuf {
        which::which("python").unwrap()
    }

    #[cfg(unix)]
    fn spawns(&self) -> usize {
        fs::read_to_string(self.root().join("spawns.log")).map_or(0, |log| log.lines().count())
    }

    fn load(&self) -> anyhow::Result<Plugins> {
        Plugins::load(&self.config(), self.root(), &self.root().join(".cache"))
    }

    fn rules(&self, options: &[(&str, serde_json::Value)]) -> PythonRules {
        let options: BTreeMap<_, _> = options
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
        self.load().unwrap().activate(|_| true, &options)
    }
}

fn check(rules: &PythonRules, path: &Path, source: &str) -> FileReport {
    check_with(rules, path, source, &|_| false)
}

fn check_with(
    rules: &PythonRules,
    path: &Path,
    source: &str,
    is_ignored: &dyn Fn(&str) -> bool,
) -> FileReport {
    let parsed = parse_module(source).unwrap();
    let comments = CommentRanges::new(
        parsed
            .tokens()
            .iter()
            .filter(|token| token.kind() == TokenKind::Comment)
            .map(Ranged::range)
            .collect(),
    );
    // Resolves `print` to the builtin unless the file imports something as `print`.
    let shadowed = source.contains("as print");
    let resolve = |expression: &Expr| match expression {
        Expr::Name(name) if name.id.as_str() == "print" && !shadowed => {
            Some("builtins.print".to_owned())
        }
        Expr::Name(name) if name.id.as_str() == "print" => Some("other.print".to_owned()),
        _ => None,
    };
    rules.check(&CheckedFile {
        path,
        source,
        module: parsed.syntax(),
        comments: &comments,
        qualified_name: &resolve,
        is_ignored,
    })
}

fn texts<'a>(report: &FileReport, source: &'a str) -> Vec<&'a str> {
    report
        .findings
        .iter()
        .map(|finding| &source[finding.range])
        .collect()
}

#[test]
fn findings_and_edits_use_rust_byte_ranges() {
    let project = Project::new(&[("rules/print_rule.py", PRINT_RULE)]);
    let rules = project.rules(&[("no-print", serde_json::json!({"to": "emit"}))]);
    let source = "\u{feff}name = 'é'\r\nprint(name)\r\n";
    let report = check(&rules, &project.root().join("module.py"), source);
    assert_eq!(report.failures, []);
    assert_eq!(texts(&report, source), ["print"]);
    let edit = &report.findings[0].edits[0];
    assert_eq!(&source[edit.range], "print");
    assert_eq!(edit.replacement, "emit");
    assert_eq!(report.findings[0].message, "print call");
}

#[test]
fn qualified_names_come_from_the_rust_side_table() {
    let project = Project::new(&[("rules/print_rule.py", PRINT_RULE)]);
    let rules = project.rules(&[]);
    let path = project.root().join("module.py");
    assert_eq!(check(&rules, &path, "print(1)\n").findings.len(), 1);
    assert_eq!(
        check(&rules, &path, "from other import emit as print\nprint(1)\n")
            .findings
            .len(),
        0
    );
}

#[test]
fn comments_are_passed_as_byte_spans() {
    let project = Project::new(&[(
        "rules/comments.py",
        r#"
from tuff_rules import TuffRule


class Todo(TuffRule):
    name = "no-todo"

    def check(self, tree, ctx):
        for span in ctx.comments:
            if "TODO" in ctx.text(span):
                ctx.report(span, "todo")
"#,
    )]);
    let rules = project.rules(&[]);
    let source = "value = 'ü'  # TODO: fix\n# done\n";
    let report = check(&rules, &project.root().join("module.py"), source);
    assert_eq!(texts(&report, source), ["# TODO: fix"]);
}

#[test]
fn ignored_and_filtered_rules_do_not_run() {
    let project = Project::new(&[(
        "rules/print_rule.py",
        &PRINT_RULE.replace(
            "node_kinds = (\"Call\",)",
            "node_kinds = (\"Call\",)\n    include = (\"src/**\",)\n    exclude = (\"src/generated/**\",)",
        ),
    )]);
    let rules = project.rules(&[]);
    let source = "print(1)\n";
    let root = project.root();
    assert_eq!(
        check(&rules, &root.join("src/app.py"), source)
            .findings
            .len(),
        1
    );
    assert_eq!(
        check(&rules, &root.join("app.py"), source).findings.len(),
        0
    );
    assert_eq!(
        check(&rules, &root.join("src/generated/app.py"), source)
            .findings
            .len(),
        0
    );
    assert_eq!(
        check_with(&rules, &root.join("src/app.py"), source, &|name| name
            == "no-print")
        .findings
        .len(),
        0
    );
}

#[test]
fn rule_exceptions_are_failures_and_other_rules_still_report() {
    let project = Project::new(&[
        ("rules/print_rule.py", PRINT_RULE),
        (
            "rules/broken.py",
            r#"
from tuff_rules import TuffRule


class Broken(TuffRule):
    name = "broken-rule"

    def check(self, tree, ctx):
        raise ValueError("boom")
"#,
        ),
    ]);
    let rules = project.rules(&[]);
    let report = check(&rules, &project.root().join("module.py"), "print(1)\n");
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].rule, Some("broken-rule"));
    assert!(
        report.failures[0].message.contains("ValueError: boom"),
        "{}",
        report.failures[0].message
    );
}

#[test]
fn invalid_spans_are_failures() {
    let project = Project::new(&[(
        "rules/span.py",
        r#"
from tuff_rules import TuffRule


class OutOfRange(TuffRule):
    name = "out-of-range"

    def check(self, tree, ctx):
        ctx.report((0, 10_000), "too far")
        ctx.report((1, 2), "inside a character")
"#,
    )]);
    let rules = project.rules(&[]);
    let report = check(&rules, &project.root().join("module.py"), "é = 1\n");
    assert_eq!(report.findings, []);
    assert_eq!(report.failures.len(), 2);
}

#[test]
fn import_errors_fail_loading() {
    let project = Project::new(&[("rules/bad.py", "import missing_module_for_test\n")]);
    let error = project.load().unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("failed to import"), "{message}");
    assert!(message.contains("missing_module_for_test"), "{message}");
}

#[test]
fn invalid_rule_metadata_fails_loading() {
    for (body, expected) in [
        ("name = 'Bad_Name'", "lower kebab-case"),
        (
            "name = 'kinds'\n    node_kinds = ('ExprCall',)",
            "unknown node kind",
        ),
        (
            "name = 'pattern'\n    include = 'src/**'",
            "sequence of strings",
        ),
    ] {
        let project = Project::new(&[(
            "rules/rule.py",
            &format!(
                "from tuff_rules import TuffRule\n\nclass Rule(TuffRule):\n    {body}\n    def check(self, tree, ctx):\n        pass\n"
            ),
        )]);
        let message = format!("{:#}", project.load().unwrap_err());
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn duplicate_rule_names_fail_loading() {
    let rule = "from tuff_rules import TuffRule\n\nclass Rule(TuffRule):\n    name = 'same'\n";
    let project = Project::new(&[("rules/a.py", rule), ("rules/b.py", rule)]);
    let message = format!("{:#}", project.load().unwrap_err());
    assert!(message.contains("already defined"), "{message}");
}

#[test]
fn decorated_functions_and_rule_lists_are_discovered() {
    let project = Project::new(&[
        (
            "rules/decorated.py",
            r#"
from tuff_rules import tuff_rule


@tuff_rule("decorated-rule", source_patterns=("x",))
def check(tree, ctx):
    ctx.report(tree.body[0], "decorated")
"#,
        ),
        (
            "rules/listed.py",
            r#"
from tuff_rules import TuffRule


class Base(TuffRule):
    def check(self, tree, ctx):
        ctx.report(tree.body[0], self.name)


class First(Base):
    name = "listed-first"


class Unlisted(Base):
    name = "unlisted"


RULES = [First]
"#,
        ),
    ]);
    let plugins = project.load().unwrap();
    assert_eq!(
        plugins.names().collect::<Vec<_>>(),
        ["decorated-rule", "listed-first"]
    );
    let rules = plugins.activate(|_| true, &BTreeMap::new());
    let report = check(&rules, &project.root().join("module.py"), "x = 1\n");
    let mut messages: Vec<_> = report
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect();
    messages.sort_unstable();
    assert_eq!(messages, ["decorated", "listed-first"]);
}

#[test]
fn worker_exit_is_reported_and_the_next_file_gets_a_new_worker() {
    let project = Project::new(&[(
        "rules/exit.py",
        r#"
import os
from tuff_rules import TuffRule


class Exit(TuffRule):
    name = "exit-rule"

    def check(self, tree, ctx):
        if ctx.relative_path == "crash.py":
            os._exit(3)
        ctx.report(tree.body[0], "ok")
"#,
    )]);
    let rules = project.rules(&[]);
    let crashed = check(&rules, &project.root().join("crash.py"), "x = 1\n");
    assert_eq!(crashed.findings, []);
    assert!(
        crashed.failures[0].message.contains("exited unexpectedly"),
        "{:?}",
        crashed.failures
    );
    let report = check(&rules, &project.root().join("fine.py"), "x = 1\n");
    assert_eq!(report.failures, []);
    assert_eq!(report.findings.len(), 1);
}

#[test]
fn disabled_rules_do_not_run() {
    let project = Project::new(&[("rules/print_rule.py", PRINT_RULE)]);
    let rules = project
        .load()
        .unwrap()
        .activate(|name| name != "no-print", &BTreeMap::new());
    assert!(!rules.has_active_rules());
    assert_eq!(
        check(&rules, &project.root().join("module.py"), "print(1)\n")
            .findings
            .len(),
        0
    );
}

#[cfg(unix)]
#[test]
fn python_starts_only_for_manifest_misses_and_candidate_files() {
    let project = Project::new(&[
        ("rules/print_rule.py", PRINT_RULE),
        ("rules/_shared.py", "VALUE = 1\n"),
    ]);
    drop(project.load().unwrap());
    assert_eq!(project.spawns(), 1);

    // The cached manifest is reused, and files the filters reject never reach Python.
    let rules = project.rules(&[]);
    assert_eq!(project.spawns(), 1);
    let path = project.root().join("module.py");
    assert_eq!(check(&rules, &path, "value = 1\n").findings.len(), 0);
    assert_eq!(check(&rules, &path, "text = 'print'\n").findings.len(), 0);
    assert_eq!(project.spawns(), 1);
    assert_eq!(check(&rules, &path, "print(1)\n").findings.len(), 1);
    assert_eq!(project.spawns(), 2);
    drop(rules);

    // Helper modules are part of the manifest key.
    project.write("rules/_shared.py", "VALUE = 2\n");
    drop(project.load().unwrap());
    assert_eq!(project.spawns(), 3);
    drop(project.load().unwrap());
    assert_eq!(project.spawns(), 3);
}

#[test]
fn fingerprint_tracks_watch_files_options_and_selection() {
    let project = Project::new(&[(
        "rules/watch.py",
        r#"
from tuff_rules import TuffRule


class Watch(TuffRule):
    name = "watch-rule"
    watch_files = ("schema.txt",)

    def check(self, tree, ctx):
        pass
"#,
    )]);
    let fingerprint =
        |options: &[(&str, serde_json::Value)]| project.rules(options).fingerprint().to_owned();
    let missing = fingerprint(&[]);
    project.write("schema.txt", "v1");
    let first = fingerprint(&[]);
    assert_ne!(missing, first);
    assert_eq!(first, fingerprint(&[]));
    project.write("schema.txt", "v2");
    let second = fingerprint(&[]);
    assert_ne!(first, second);
    assert_ne!(
        second,
        fingerprint(&[("watch-rule", serde_json::json!({"level": 1}))])
    );
    let disabled = project
        .load()
        .unwrap()
        .activate(|_| false, &BTreeMap::new());
    assert_ne!(second, disabled.fingerprint());
}

#[test]
fn uncacheable_rules_change_the_fingerprint_every_load() {
    let project = Project::new(&[(
        "rules/volatile.py",
        "from tuff_rules import TuffRule\n\nclass Volatile(TuffRule):\n    name = 'volatile'\n    cacheable = False\n",
    )]);
    assert_ne!(
        project.rules(&[]).fingerprint(),
        project.rules(&[]).fingerprint()
    );
}

#[test]
fn testing_helper_runs_rules_without_tuff() {
    let project = Project::new(&[("rules/print_rule.py", PRINT_RULE)]);
    let script = r#"
import sys
sys.path[:0] = sys.argv[1:3]
from print_rule import NoPrint
from tuff_rules.testing import apply_edits, check_source

source = """import builtins as b
from other import emit as print
b.print(1)

def run(log):
    print(2)

[print(x) for x in range(2)]
"""
findings = check_source(NoPrint, source, options={"to": "log"})
print(apply_edits(source, findings), end="")
"#;
    let output = std::process::Command::new(which::which("python3").unwrap())
        .args(["-I", "-c", script])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("python"))
        .arg(project.root().join("rules"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "import builtins as b\nfrom other import emit as print\nlog(1)\n\ndef run(log):\n    print(2)\n\n[print(x) for x in range(2)]\n"
    );
}
