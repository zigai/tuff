use anyhow::Result;
use insta::{assert_json_snapshot, assert_snapshot};
use lsp_types::{
    Diagnostic, DocumentDiagnosticReport, Position, PublishDiagnosticsNotification,
    TextDocumentContentChangeEvent, TextDocumentContentChangeWholeDocument,
};

use crate::{TestServer, TestServerBuilder};

const THROWS_PYPROJECT: &str = r#"
[project]
name = "sample"
version = "0.1.0"
requires-python = ">=3.12"

[tool.tuff.throws]
boundaries = ["app.api.*"]
"#;

const PLAIN_PYPROJECT: &str = r#"
[project]
name = "sample"
version = "0.1.0"
requires-python = ">=3.12"
"#;

const ERRORS: &str = r"
class DomainError(Exception):
    pass


class NotFound(DomainError):
    pass


class Invalid(DomainError):
    pass
";

const CONTRACTS: &str = r"
def throws(*exceptions):
    def decorate(function):
        return function

    return decorate
";

const CORE: &str = r"from app.errors import NotFound


def load(key):
    raise NotFound(key)
";

const CORE_EDITED: &str = r"from app.errors import Invalid


def load(key):
    raise Invalid(key)
";

const API: &str = r"from app.core import load
from app.errors import Invalid


def handler():  # noqa: F401
    return load('key')


def redundant():
    try:
        return 1
    except Invalid:
        return 2
";

const SERVICE: &str = r"from app.contracts import throws
from app.core import load
from app.errors import Invalid


@throws(Invalid)
def declared():
    return load('key')


class Base:
    @throws(Invalid)
    def run(self):
        raise Invalid()


class Child(Base):
    def run(self):
        return load('key')
";

fn project(pyproject: &str) -> Result<TestServerBuilder> {
    TestServerBuilder::new()?
        .with_workspace(".")?
        .with_file("pyproject.toml", pyproject)?
        .with_file("app/__init__.py", "")?
        .with_file("app/errors.py", ERRORS)?
        .with_file("app/contracts.py", CONTRACTS)?
        .with_file("app/core.py", CORE)?
        .with_file("app/api.py", API)?
        .with_file("app/service.py", SERVICE)
}

fn tuff_diagnostics(server: &mut TestServer, path: &str) -> Vec<Diagnostic> {
    let DocumentDiagnosticReport::RelatedFullDocumentDiagnosticReport(report) =
        server.document_diagnostic_request(path, None)
    else {
        panic!("Expected a full diagnostic report");
    };
    report
        .full_document_diagnostic_report
        .items
        .into_iter()
        .filter(|diagnostic| diagnostic.source.as_deref() == Some("Tuff"))
        .collect()
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.code {
            Some(lsp_types::Code::String(code)) => Some(code.clone()),
            _ => None,
        })
        .collect()
}

/// The position of the second character of the first occurrence of `needle`.
fn position_of(source: &str, needle: &str) -> Position {
    let offset = source.find(needle).expect("needle should be in the source");
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    Position {
        line: u32::try_from(source[..offset].matches('\n').count()).unwrap(),
        character: u32::try_from(offset - line_start + 1).unwrap(),
    }
}

fn hover_markdown(server: &mut TestServer, path: &str, position: Position) -> Option<String> {
    let hover = server.hover_request(path, position)?;
    let lsp_types::Contents::MarkupContent(markup) = hover.contents else {
        panic!("Expected Markdown hover contents");
    };
    Some(markup.value)
}

#[test]
fn diagnostics_for_configured_workspace() -> Result<()> {
    let mut server = project(THROWS_PYPROJECT)?
        .enable_diagnostic_related_information(true)
        .build();

    server.open_text_document("app/api.py", API, 1);
    server.open_text_document("app/service.py", SERVICE, 1);

    let api = tuff_diagnostics(&mut server, "app/api.py");
    let service = tuff_diagnostics(&mut server, "app/service.py");
    assert_eq!(codes(&api), ["THROWS001", "THROWS002"]);
    assert_eq!(codes(&service), ["THROWS003", "THROWS004"]);

    assert_json_snapshot!("throws_api_diagnostics", api);
    assert_json_snapshot!("throws_service_diagnostics", service);

    Ok(())
}

#[test]
fn diagnostics_without_related_information_support() -> Result<()> {
    let mut server = project(THROWS_PYPROJECT)?.build();

    server.open_text_document("app/api.py", API, 1);

    let diagnostics = tuff_diagnostics(&mut server, "app/api.py");
    assert_eq!(codes(&diagnostics), ["THROWS001", "THROWS002"]);
    assert!(diagnostics[0].related_information.is_none());
    assert_json_snapshot!(diagnostics[0].message, @r#""`NotFound` escapes boundary `handler` unhandled\n\n  handler  app/api.py:6:12  calls `load()`\n→ load     app/core.py:5:5  raises `NotFound`""#);

    Ok(())
}

#[test]
fn diagnostics_follow_unsaved_edits() -> Result<()> {
    let mut server = project(THROWS_PYPROJECT)?.build();

    server.open_text_document("app/api.py", API, 1);
    server.open_text_document("app/core.py", CORE, 1);
    let before = tuff_diagnostics(&mut server, "app/api.py");
    assert_eq!(
        before[0].message,
        "`NotFound` escapes boundary `handler` unhandled\n\n  handler  app/api.py:6:12  calls `load()`\n→ load     app/core.py:5:5  raises `NotFound`".into()
    );

    server.change_text_document(
        "app/core.py",
        vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: CORE_EDITED.to_string(),
                },
            ),
        ],
        2,
    );
    let edited = tuff_diagnostics(&mut server, "app/api.py");
    assert_eq!(codes(&edited), ["THROWS001", "THROWS002"]);
    assert_eq!(
        edited[0].message,
        "`Invalid` escapes boundary `handler` unhandled\n\n  handler  app/api.py:6:12  calls `load()`\n→ load     app/core.py:5:5  raises `Invalid`".into()
    );
    assert_eq!(
        std::fs::read_to_string(server.file_path("app/core.py"))?,
        CORE
    );

    // Closing the edited buffer discards its unsaved contents.
    server.close_text_document("app/core.py");
    let cleared = server.await_notification::<PublishDiagnosticsNotification>();
    assert!(cleared.diagnostics.is_empty());
    let closed = tuff_diagnostics(&mut server, "app/api.py");
    assert_eq!(closed, before);

    Ok(())
}

#[test]
fn published_diagnostics_use_unsaved_buffers() -> Result<()> {
    let mut server = project(THROWS_PYPROJECT)?
        .enable_pull_diagnostics(false)
        .enable_diagnostic_related_information(true)
        .build();

    server.open_text_document("app/core.py", CORE_EDITED, 1);
    let core = server.await_notification::<PublishDiagnosticsNotification>();
    assert_eq!(core.uri, server.file_uri("app/core.py"));

    server.open_text_document("app/api.py", API, 1);
    let api = server.await_notification::<PublishDiagnosticsNotification>();
    let tuff: Vec<_> = api
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.source.as_deref() == Some("Tuff"))
        .collect();
    assert_eq!(codes(&tuff), ["THROWS001", "THROWS002"]);
    assert_eq!(
        tuff[0].message,
        "`Invalid` escapes boundary `handler` unhandled".into()
    );
    let related = tuff[0]
        .related_information
        .as_ref()
        .expect("Expected related information");
    assert_eq!(related.len(), 2);
    assert_eq!(related[1].location.uri, server.file_uri("app/core.py"));
    assert_eq!(related[1].message, "`load` raises `Invalid`");

    Ok(())
}

#[test]
fn hover_on_definition_and_call_site() -> Result<()> {
    let mut server = project(THROWS_PYPROJECT)?.build();

    server.open_text_document("app/api.py", API, 1);
    server.open_text_document("app/service.py", SERVICE, 1);

    let definition = server
        .hover_request("app/api.py", position_of(API, "handler"))
        .expect("Expected hover information for a function definition");
    assert_json_snapshot!(definition.range, @r#"
    {
      "start": {
        "line": 4,
        "character": 4
      },
      "end": {
        "line": 4,
        "character": 11
      }
    }
    "#);
    let lsp_types::Contents::MarkupContent(markup) = definition.contents else {
        panic!("Expected Markdown hover contents");
    };
    assert_snapshot!(markup.value, @"
    **`app.api.handler`**

    May raise:

    - `NotFound` (`app.errors.NotFound`)

      ```text
        handler  app/api.py:6:12  calls `load()`
      → load     app/core.py:5:5  raises `NotFound`
      ```
    ");

    let call = hover_markdown(&mut server, "app/api.py", position_of(API, "load('key')"))
        .expect("Expected hover information for a call site");
    assert_snapshot!(call, @"
    **`app.core.load`**

    May raise:

    - `NotFound` (`app.errors.NotFound`)

      ```text
        load  app/core.py:5:5  raises `NotFound`
      ```
    ");

    let declared = hover_markdown(
        &mut server,
        "app/service.py",
        position_of(SERVICE, "declared"),
    )
    .expect("Expected hover information for a function with a contract");
    assert_snapshot!(declared, @"
    **`app.service.declared`**

    Declares: `Invalid`

    May raise:

    - `Invalid` (`app.errors.Invalid`)

      ```text
        declared  app/service.py:7:5  declares `Invalid` with @throws
      ```

    - `NotFound` (`app.errors.NotFound`)

      ```text
        declared  app/service.py:8:12  calls `load()`
      → load      app/core.py:5:5      raises `NotFound`
      ```
    ");

    assert!(hover_markdown(&mut server, "app/api.py", position_of(API, "try")).is_none());

    Ok(())
}

#[test]
fn noqa_hover_takes_precedence_on_comments() -> Result<()> {
    let mut server = project(THROWS_PYPROJECT)?.build();

    server.open_text_document("app/api.py", API, 1);

    let noqa = hover_markdown(&mut server, "app/api.py", position_of(API, "F401"))
        .expect("Expected hover information for a `noqa` comment");
    assert!(noqa.starts_with("# unused-import (F401)"));

    let throws = hover_markdown(&mut server, "app/api.py", position_of(API, "handler"))
        .expect("Expected hover information for a function on the same line");
    assert!(throws.starts_with("**`app.api.handler`**"));

    Ok(())
}

#[test]
fn unconfigured_workspace_has_no_throws_diagnostics_or_hover() -> Result<()> {
    let mut server = project(PLAIN_PYPROJECT)?.build();

    server.open_text_document("app/api.py", API, 1);
    server.open_text_document("app/service.py", SERVICE, 1);

    assert!(tuff_diagnostics(&mut server, "app/api.py").is_empty());
    assert!(tuff_diagnostics(&mut server, "app/service.py").is_empty());
    assert!(hover_markdown(&mut server, "app/api.py", position_of(API, "handler")).is_none());
    assert!(hover_markdown(&mut server, "app/api.py", position_of(API, "load('key')")).is_none());

    let noqa = hover_markdown(&mut server, "app/api.py", position_of(API, "F401"))
        .expect("Expected hover information for a `noqa` comment");
    assert!(noqa.starts_with("# unused-import (F401)"));

    Ok(())
}
