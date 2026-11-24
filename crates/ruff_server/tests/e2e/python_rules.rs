//! Repository-local Python rules over LSP. The rules run in a CPython worker started through
//! a wrapper script that records each start, so these tests need a Unix shell and `python3`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use anyhow::{Context, Result};
use insta::assert_json_snapshot;
use lsp_types::{
    CodeActionKind, CodeActionResponse, DidChangeWatchedFilesClientCapabilities,
    DidSaveTextDocumentNotification, DidSaveTextDocumentParams, DocumentDiagnosticReport,
    FileChangeType, FileEvent, GlobPattern, Position, PublishDiagnosticsNotification,
    RegistrationParams, RegistrationRequest, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextEdit,
};

use crate::{TestServer, TestServerBuilder};

const RULES: &str = r#"
import ast
from tuff_rules import TuffRule


class NoPrint(TuffRule):
    name = "no-print"
    description = "Use the project logger instead of `print`."
    source_patterns = ("print",)
    node_kinds = ("Call",)

    def check(self, tree, ctx):
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and ctx.qualified_name(node.func) == "builtins.print":
                ctx.report(node.func, "Use the logger", [ctx.edit(node.func, "logger.info")])


class NoShellCall(TuffRule):
    name = "no-shell-call"
    description = "Run commands through the project's process helpers."
    source_patterns = ("subprocess",)

    def check(self, tree, ctx):
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and ctx.qualified_name(node.func) == "subprocess.call":
                ctx.report(node, "Avoid `subprocess.call`")
"#;

const SOURCE: &str = "import subprocess as sp\n\nsp.call([\"ls\"])\nprint(\"hi\")\n";

/// A project whose `rules/` directory holds [`RULES`] and whose `python` wrapper appends
/// to `spawns.log` each time a rule worker starts.
fn project(lint: &str) -> Result<TestServerBuilder> {
    let builder = TestServerBuilder::new()?
        .with_workspace("project")?
        .with_file("project/rules/rules.py", RULES)?
        .with_file(
            "project/pyproject.toml",
            format!("[tool.ruff.lint]\nselect = []\n\n[tool.tuff.lint]\n{lint}"),
        )?;
    let wrapper = builder.file_path("project/python");
    let log = builder.file_path("project/spawns.log");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho started >> '{}'\nexec python3 \"$@\"\n",
            log.display()
        ),
    )?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    Ok(builder)
}

const PLUGIN_CONFIG: &str = "plugin-dirs = [\"rules\"]\npython = \"./python\"\n";

fn watch_files(mut builder: TestServerBuilder) -> TestServerBuilder {
    builder
        .client_capabilities
        .workspace
        .get_or_insert_default()
        .did_change_watched_files = Some(DidChangeWatchedFilesClientCapabilities {
        dynamic_registration: Some(true),
        relative_pattern_support: Some(true),
    });
    builder
}

fn spawns(server: &TestServer) -> usize {
    fs::read_to_string(server.file_path("project/spawns.log")).map_or(0, |log| log.lines().count())
}

fn diagnostics(server: &mut TestServer, path: &str) -> Vec<lsp_types::Diagnostic> {
    match server.document_diagnostic_request(path, None) {
        DocumentDiagnosticReport::RelatedFullDocumentDiagnosticReport(report) => {
            report.full_document_diagnostic_report.items
        }
        DocumentDiagnosticReport::RelatedUnchangedDocumentDiagnosticReport(_) => {
            panic!("Expected a full diagnostic report")
        }
    }
}

fn codes_and_messages(diagnostics: &[lsp_types::Diagnostic]) -> Vec<(String, String)> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let code = match &diagnostic.code {
                Some(lsp_types::Code::String(code)) => code.clone(),
                code => format!("{code:?}"),
            };
            let message = match &diagnostic.message {
                lsp_types::Message::String(message) => message.clone(),
                lsp_types::Message::MarkupContent(markup) => markup.value.clone(),
            };
            (code, message)
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Applies non-overlapping edits on ASCII text.
fn apply(source: &str, edits: &[TextEdit]) -> String {
    let offset = |position: Position| {
        let line_start: usize = source
            .split_inclusive('\n')
            .take(position.line as usize)
            .map(str::len)
            .sum();
        line_start + position.character as usize
    };
    let mut edits: Vec<_> = edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(offset(edit.range.start)));
    let mut result = source.to_owned();
    for edit in edits {
        result.replace_range(
            offset(edit.range.start)..offset(edit.range.end),
            &edit.new_text,
        );
    }
    result
}

fn quick_fixes(
    server: &mut TestServer,
    path: &str,
    diagnostics: Vec<lsp_types::Diagnostic>,
) -> Vec<(String, Vec<TextEdit>)> {
    let uri = server.file_uri(path);
    server
        .code_action_request(path, diagnostics)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionResponse::CodeAction(action)
                if action.kind == Some(CodeActionKind::QuickFix) =>
            {
                let mut changes = action.edit?.changes?;
                Some((action.title, changes.remove(&uri)?))
            }
            _ => None,
        })
        .collect()
}

fn replace_document(server: &mut TestServer, path: &str, text: &str, version: i32) {
    server.change_text_document(
        path,
        vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                lsp_types::TextDocumentContentChangeWholeDocument {
                    text: text.to_owned(),
                },
            ),
        ],
        version,
    );
}

#[test]
fn reports_python_rule_diagnostics() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    server.open_text_document("project/app.py", SOURCE, 1);

    assert_json_snapshot!(diagnostics(&mut server, "project/app.py"), @r#"
    [
      {
        "range": {
          "start": {
            "line": 3,
            "character": 0
          },
          "end": {
            "line": 3,
            "character": 5
          }
        },
        "severity": 2,
        "code": "no-print",
        "source": "Ruff",
        "message": "Use the logger",
        "tags": [],
        "data": {
          "code": "no-print",
          "edits": [
            {
              "newText": "logger.info",
              "range": {
                "end": {
                  "character": 5,
                  "line": 3
                },
                "start": {
                  "character": 0,
                  "line": 3
                }
              }
            }
          ],
          "is_preferred": true,
          "noqa_edit": {
            "newText": "  # tuff: ignore[no-print]",
            "range": {
              "end": {
                "character": 11,
                "line": 3
              },
              "start": {
                "character": 11,
                "line": 3
              }
            }
          },
          "title": "no-print"
        }
      },
      {
        "range": {
          "start": {
            "line": 2,
            "character": 0
          },
          "end": {
            "line": 2,
            "character": 15
          }
        },
        "severity": 2,
        "code": "no-shell-call",
        "source": "Ruff",
        "message": "Avoid `subprocess.call`",
        "tags": [],
        "data": {
          "code": "no-shell-call",
          "edits": [],
          "is_preferred": null,
          "noqa_edit": {
            "newText": "  # tuff: ignore[no-shell-call]",
            "range": {
              "end": {
                "character": 15,
                "line": 2
              },
              "start": {
                "character": 15,
                "line": 2
              }
            }
          },
          "title": "no-shell-call"
        }
      }
    ]
    "#);

    Ok(())
}

#[test]
fn quick_fixes_apply_rule_edits_and_tuff_ignore_comments() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    server.open_text_document("project/app.py", SOURCE, 1);

    let reported = diagnostics(&mut server, "project/app.py");
    let fixes = quick_fixes(&mut server, "project/app.py", reported);
    let titles: Vec<_> = fixes.iter().map(|(title, _)| title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Ruff (no-print): no-print",
            "Ruff (no-print): Disable for this line",
            "Ruff (no-shell-call): Disable for this line",
        ]
    );

    let fixed = apply(SOURCE, &fixes[0].1);
    assert_eq!(
        fixed,
        "import subprocess as sp\n\nsp.call([\"ls\"])\nlogger.info(\"hi\")\n"
    );

    // Applying every suppression leaves no Python rule diagnostics behind.
    let suppressed = apply(SOURCE, &[fixes[1].1.clone(), fixes[2].1.clone()].concat());
    assert_eq!(
        suppressed,
        "import subprocess as sp\n\nsp.call([\"ls\"])  # tuff: ignore[no-shell-call]\nprint(\"hi\")  # tuff: ignore[no-print]\n"
    );
    replace_document(&mut server, "project/app.py", &suppressed, 2);
    assert_eq!(diagnostics(&mut server, "project/app.py"), []);

    Ok(())
}

#[test]
fn suppression_extends_existing_comments() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    let source = "import subprocess\n\nsubprocess.call(print())  # tuff: ignore[no-shell-call]\nprint()  # noqa: E501\n";
    server.open_text_document("project/app.py", source, 1);

    let reported = diagnostics(&mut server, "project/app.py");
    let suppressions: Vec<_> = quick_fixes(&mut server, "project/app.py", reported)
        .into_iter()
        .filter(|(title, _)| title.ends_with("Disable for this line"))
        .flat_map(|(_, edits)| edits)
        .collect();
    let suppressed = apply(source, &suppressions);
    assert_eq!(
        suppressed,
        "import subprocess\n\nsubprocess.call(print())  # tuff: ignore[no-shell-call, no-print]\nprint()  # noqa: E501  # tuff: ignore[no-print]\n"
    );
    replace_document(&mut server, "project/app.py", &suppressed, 2);
    assert_eq!(diagnostics(&mut server, "project/app.py"), []);

    Ok(())
}

#[test]
fn hover_describes_rules_in_tuff_ignore_comments() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    server.open_text_document(
        "project/app.py",
        "print()  # tuff: ignore[no-print, unused-import, missing-rule]\n",
        1,
    );

    let hover_text = |server: &mut TestServer, character| {
        let hover = server
            .hover_request("project/app.py", Position { line: 0, character })
            .expect("Expected hover information");
        let lsp_types::Contents::MarkupContent(content) = hover.contents else {
            panic!("Expected markup hover contents");
        };
        (content.value, hover.range)
    };

    assert_json_snapshot!(hover_text(&mut server, 27), @r##"
    [
      "# no-print\n\nPython rule defined in `rules/rules.py`.\n\nUse the project logger instead of `print`.",
      {
        "start": {
          "line": 0,
          "character": 24
        },
        "end": {
          "line": 0,
          "character": 32
        }
      }
    ]
    "##);

    let (native, _) = hover_text(&mut server, 36);
    assert!(
        native.starts_with("# unused-import (F401)\n"),
        "Expected the native rule documentation, got: {native}"
    );

    let (missing, _) = hover_text(&mut server, 52);
    assert_eq!(missing, "missing-rule: Rule not found");

    Ok(())
}

#[test]
fn hover_marks_disabled_python_rules() -> Result<()> {
    let mut server = project(&format!("{PLUGIN_CONFIG}disable = [\"no-print\"]\n"))?.build();
    server.open_text_document("project/app.py", "x = 1  # tuff: ignore[no-print]\n", 1);

    let hover = server
        .hover_request(
            "project/app.py",
            Position {
                line: 0,
                character: 25,
            },
        )
        .context("Expected hover information")?;
    assert_json_snapshot!(hover.contents, @r##"
    {
      "kind": "markdown",
      "value": "# no-print\n\nPython rule defined in `rules/rules.py`.\n\nThis rule is disabled in this configuration.\n\nUse the project logger instead of `print`."
    }
    "##);

    Ok(())
}

#[test]
fn edits_reuse_the_warm_worker() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    server.open_text_document("project/app.py", SOURCE, 1);
    assert_eq!(diagnostics(&mut server, "project/app.py").len(), 2);

    for version in 2..5 {
        replace_document(
            &mut server,
            "project/app.py",
            &format!("{SOURCE}print({version})\n"),
            version,
        );
        assert_eq!(diagnostics(&mut server, "project/app.py").len(), 3);
    }

    assert_eq!(spawns(&server), 1);

    Ok(())
}

#[test]
fn changed_rule_module_reloads_rules() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    server.open_text_document("project/app.py", SOURCE, 1);
    assert_eq!(diagnostics(&mut server, "project/app.py").len(), 2);

    server.write_file(
        "project/rules/rules.py",
        RULES.replace("\"Use the logger\"", "\"Print is not allowed\""),
    )?;
    let uri = server.file_uri("project/rules/rules.py");
    server.did_change_watched_files(vec![FileEvent {
        uri,
        kind: FileChangeType::Changed,
    }]);
    server.await_notification::<PublishDiagnosticsNotification>();

    assert_eq!(
        codes_and_messages(&diagnostics(&mut server, "project/app.py")),
        [
            ("no-print".to_owned(), "Print is not allowed".to_owned()),
            (
                "no-shell-call".to_owned(),
                "Avoid `subprocess.call`".to_owned()
            ),
        ]
    );
    // The old worker belongs to the replaced rules; one new worker serves the new ones.
    assert_eq!(spawns(&server), 2);

    Ok(())
}

#[test]
fn changed_watch_file_reloads_rules() -> Result<()> {
    let rule = r#"
import ast
from pathlib import Path
from tuff_rules import TuffRule

BANNED = Path("banned.txt").read_text().split()


class BannedCall(TuffRule):
    name = "banned-call"
    watch_files = ("banned.txt",)

    def check(self, tree, ctx):
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and ctx.qualified_name(node.func) in BANNED:
                ctx.report(node.func, f"`{ctx.qualified_name(node.func)}` is banned")
"#;
    let mut server = project(PLUGIN_CONFIG)?
        .with_file("project/rules/rules.py", rule)?
        .with_file("project/banned.txt", "builtins.print\n")?
        .build();
    server.open_text_document("project/app.py", SOURCE, 1);
    assert_eq!(
        codes_and_messages(&diagnostics(&mut server, "project/app.py")),
        [(
            "banned-call".to_owned(),
            "`builtins.print` is banned".to_owned()
        )]
    );

    server.write_file("project/banned.txt", "subprocess.call\n")?;
    let uri = server.file_uri("project/banned.txt");
    server.did_change_watched_files(vec![FileEvent {
        uri,
        kind: FileChangeType::Changed,
    }]);
    server.await_notification::<PublishDiagnosticsNotification>();

    assert_eq!(
        codes_and_messages(&diagnostics(&mut server, "project/app.py")),
        [(
            "banned-call".to_owned(),
            "`subprocess.call` is banned".to_owned()
        )]
    );

    Ok(())
}

#[test]
fn watches_plugin_directories_outside_the_workspace() -> Result<()> {
    let builder = watch_files(project(
        "plugin-dirs = [\"../shared-rules\"]\npython = \"./python\"\n",
    )?)
    .with_file("shared-rules/rules.py", RULES)?;
    let mut server = builder.build();

    let registrations = [
        server.await_request::<RegistrationRequest>(),
        server.await_request::<RegistrationRequest>(),
    ];
    let mut python_rule_watchers = None;
    for (id, RegistrationParams { registrations }) in registrations {
        for registration in registrations {
            if registration.id == "tuff-python-rules-watch" {
                python_rule_watchers = registration.register_options;
            }
        }
        server.respond(id, ());
    }
    let options: lsp_types::DidChangeWatchedFilesRegistrationOptions =
        serde_json::from_value(python_rule_watchers.context("Expected Python rule watchers")?)?;
    let [watcher] = options.watchers.as_slice() else {
        panic!("Expected one watcher, got {:?}", options.watchers);
    };
    let GlobPattern::RelativePattern(pattern) = &watcher.glob_pattern else {
        panic!(
            "Expected a relative pattern, got {:?}",
            watcher.glob_pattern
        );
    };
    assert_eq!(
        pattern.base_uri,
        lsp_types::BaseUri::Uri(server.file_uri("shared-rules"))
    );
    assert_eq!(pattern.pattern, "**/*.py");

    server.open_text_document("project/app.py", SOURCE, 1);
    assert_eq!(diagnostics(&mut server, "project/app.py").len(), 2);

    server.write_file(
        "shared-rules/rules.py",
        RULES.replace("\"Use the logger\"", "\"Print is not allowed\""),
    )?;
    let uri = server.file_uri("shared-rules/rules.py");
    server.did_change_watched_files(vec![FileEvent {
        uri,
        kind: FileChangeType::Changed,
    }]);
    server.await_notification::<PublishDiagnosticsNotification>();

    let messages: Vec<_> = codes_and_messages(&diagnostics(&mut server, "project/app.py"))
        .into_iter()
        .map(|(_, message)| message)
        .collect();
    assert_eq!(
        messages,
        ["Print is not allowed", "Avoid `subprocess.call`"]
    );

    Ok(())
}

#[test]
fn saving_a_rule_module_reloads_rules_without_file_watchers() -> Result<()> {
    let mut server = project(PLUGIN_CONFIG)?.build();
    server.open_text_document("project/app.py", SOURCE, 1);
    assert_eq!(diagnostics(&mut server, "project/app.py").len(), 2);

    server.write_file("project/rules/rules.py", RULES.replace("print", "input"))?;
    let uri = server.file_uri("project/rules/rules.py");
    server.send_notification::<DidSaveTextDocumentNotification>(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        text: None,
    });
    server.await_notification::<PublishDiagnosticsNotification>();

    assert_eq!(
        codes_and_messages(&diagnostics(&mut server, "project/app.py")),
        [(
            "no-shell-call".to_owned(),
            "Avoid `subprocess.call`".to_owned()
        )]
    );

    // Saving an ordinary source file does not reload settings.
    let uri = server.file_uri("project/app.py");
    server.send_notification::<DidSaveTextDocumentNotification>(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        text: None,
    });
    assert!(
        server
            .try_await_notification::<PublishDiagnosticsNotification>(Some(Duration::from_millis(
                200
            )))
            .is_err()
    );

    Ok(())
}

#[test]
fn no_plugin_dirs_starts_no_worker_and_registers_no_watchers() -> Result<()> {
    let mut server = watch_files(project("python = \"./python\"\n")?).build();

    let (id, params) = server.await_request::<RegistrationRequest>();
    assert!(
        params
            .registrations
            .iter()
            .all(|registration| registration.id != "tuff-python-rules-watch")
    );
    server.respond(id, ());
    assert!(
        server
            .try_await_request::<RegistrationRequest>(Some(Duration::from_millis(200)))
            .is_err()
    );

    server.open_text_document("project/app.py", SOURCE, 1);
    assert_eq!(diagnostics(&mut server, "project/app.py"), []);
    assert_eq!(spawns(&server), 0);

    Ok(())
}
