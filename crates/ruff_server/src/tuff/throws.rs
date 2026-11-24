//! Exception-flow diagnostics and hovers from `tuff_throws`.
//!
//! Only workspaces whose nearest Tuff configuration file has a `[tool.tuff.throws]` or
//! `[throws]` section are analyzed. Each configured project is analyzed lazily, on the first
//! diagnostic or hover request after a change, with the contents of its open documents in
//! place of the files on disk, and the analysis is reused until a document in the project
//! changes or the configuration is reloaded.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use lsp_types::{self as types, Uri};
use ruff_python_ast::PySourceType;
use rustc_hash::FxHashMap;
use tuff_throws::check::{Code, Diagnostic};
use tuff_throws::config::{LoadedConfig, discover_configured};
use tuff_throws::inspect::render_chain;
use tuff_throws::snapshot::{HoverTarget, ThrowsSnapshot};
use tuff_throws::ty_adapter::Overrides;

use crate::PositionEncoding;
use crate::edit::{RangeExt, ToRangeExt};
use crate::session::{DocumentQuery, DocumentSnapshot};

const DIAGNOSTIC_SOURCE: &str = "Tuff";

/// Throws configuration, open document contents, and cached analyses for every project the
/// server has seen a Python document from.
#[derive(Debug, Default)]
pub(crate) struct ThrowsIndex {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// The throws configuration that applies to each directory containing a document, or
    /// `None` if the directory's project does not configure throws.
    configs: FxHashMap<PathBuf, Option<Arc<LoadedConfig>>>,
    /// Contents of the open documents in configured projects, keyed by canonical path.
    overlays: FxHashMap<PathBuf, String>,
    /// The current analysis of each configured project, keyed by its canonical root.
    projects: FxHashMap<PathBuf, Arc<Project>>,
}

#[derive(Debug)]
struct Project {
    loaded: Arc<LoadedConfig>,
    overrides: Overrides,
    snapshot: OnceLock<Option<Arc<ThrowsSnapshot>>>,
}

impl Project {
    /// Analyzes the project on first use. Concurrent callers wait for the same analysis.
    fn snapshot(&self) -> Option<Arc<ThrowsSnapshot>> {
        self.snapshot
            .get_or_init(
                || match ThrowsSnapshot::build(&self.loaded, &self.overrides) {
                    Ok(snapshot) => Some(Arc::new(snapshot)),
                    Err(error) => {
                        tracing::error!(
                            "Failed to analyze exception flow in `{}`: {error:#}",
                            self.loaded.root.display()
                        );
                        None
                    }
                },
            )
            .clone()
    }
}

impl ThrowsIndex {
    /// Records the contents of an opened or edited document. `contents` is only called when the
    /// document belongs to a project that configures throws.
    pub(crate) fn document_changed(&self, path: &Path, contents: impl FnOnce() -> Option<String>) {
        if !is_python(path) {
            return;
        }
        let mut state = self.lock();
        if state.config(path).is_none() {
            return;
        }
        let Some(contents) = contents() else {
            return;
        };
        let path = canonical(path);
        state.invalidate(&path);
        state.overlays.insert(path, contents);
    }

    pub(crate) fn document_closed(&self, path: &Path) {
        if !is_python(path) {
            return;
        }
        let mut state = self.lock();
        if state.config(path).is_none() {
            return;
        }
        let path = canonical(path);
        if state.overlays.remove(&path).is_some() {
            state.invalidate(&path);
        }
    }

    /// Forgets every configuration, document, and analysis, for example after a configuration
    /// file changed. The caller re-registers the open documents.
    pub(crate) fn clear(&self) {
        *self.lock() = State::default();
    }

    /// Returns the analysis of the project containing `path`, analyzing it if needed, or
    /// `None` if the project does not configure throws.
    fn snapshot(&self, path: &Path) -> Option<Arc<ThrowsSnapshot>> {
        if !is_python(path) {
            return None;
        }
        let project = {
            let mut state = self.lock();
            let loaded = state.config(path)?;
            match state.projects.get(&loaded.root) {
                Some(project) => project.clone(),
                None => {
                    let overrides = state
                        .overlays
                        .iter()
                        .filter(|(path, _)| path.starts_with(&loaded.root))
                        .map(|(path, contents)| (path.clone(), contents.clone()))
                        .collect();
                    let project = Arc::new(Project {
                        loaded: loaded.clone(),
                        overrides,
                        snapshot: OnceLock::new(),
                    });
                    state.projects.insert(loaded.root.clone(), project.clone());
                    project
                }
            }
        };
        // Analyze outside the lock so that document notifications are never blocked on it.
        project.snapshot()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    fn config(&mut self, path: &Path) -> Option<Arc<LoadedConfig>> {
        let directory = path.parent()?;
        if let Some(config) = self.configs.get(directory) {
            return config.clone();
        }
        let config = match discover_configured(directory) {
            Ok(loaded) => loaded.map(|mut loaded| {
                loaded.root = canonical(&loaded.root);
                Arc::new(loaded)
            }),
            Err(error) => {
                tracing::warn!(
                    "Failed to read the throws configuration for `{}`: {error:#}",
                    directory.display()
                );
                None
            }
        };
        self.configs.insert(directory.to_path_buf(), config.clone());
        config
    }

    /// Drops the analysis of every project containing `path`, including enclosing projects
    /// that analyze a nested project's files too.
    fn invalidate(&mut self, path: &Path) {
        self.projects.retain(|root, _| !path.starts_with(root));
    }
}

fn is_python(path: &Path) -> bool {
    matches!(
        PySourceType::from(path),
        PySourceType::Python | PySourceType::Stub
    )
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Exception-flow diagnostics for a Python text document.
pub(crate) fn diagnostics(
    index: &ThrowsIndex,
    query: &DocumentQuery,
    encoding: PositionEncoding,
    supports_related_information: bool,
) -> Vec<types::Diagnostic> {
    if query.as_notebook().is_some() {
        return Vec::new();
    }
    let Some(path) = query.file_path() else {
        return Vec::new();
    };
    let Some(snapshot) = index.snapshot(&path) else {
        return Vec::new();
    };
    let Some(file) = snapshot.file(&path) else {
        return Vec::new();
    };
    snapshot
        .diagnostics_for_file(&path)
        .iter()
        .map(|diagnostic| {
            let related_information = supports_related_information
                .then(|| related_information(&snapshot, diagnostic, encoding));
            let mut message = diagnostic.message.clone();
            if !supports_related_information && !diagnostic.chain.is_empty() {
                message.push_str("\n\n");
                render_chain(&mut message, &diagnostic.chain, "");
                message.truncate(message.trim_end().len());
            }
            types::Diagnostic {
                range: diagnostic
                    .range
                    .to_range(file.source(), file.line_index(), encoding),
                severity: Some(types::DiagnosticSeverity::Warning),
                code: Some(types::Code::String(diagnostic.code.code().to_string())),
                code_description: None,
                source: Some(DIAGNOSTIC_SOURCE.to_string()),
                message: message.into(),
                related_information,
                tags: (diagnostic.code == Code::RedundantExcept)
                    .then(|| vec![types::DiagnosticTag::Unnecessary]),
                data: None,
            }
        })
        .collect()
}

/// One location per step of the diagnostic's call chain, from the reported function to where
/// the exception is raised or declared.
fn related_information(
    snapshot: &ThrowsSnapshot,
    diagnostic: &Diagnostic,
    encoding: PositionEncoding,
) -> Vec<types::DiagnosticRelatedInformation> {
    diagnostic
        .chain
        .iter()
        .filter_map(|step| {
            let file = snapshot.file(&step.path)?;
            Some(types::DiagnosticRelatedInformation {
                location: types::Location {
                    uri: Uri::from_file_path(&step.path).ok()?,
                    range: step
                        .range
                        .to_range(file.source(), file.line_index(), encoding),
                },
                message: format!("`{}` {}", step.function, step.action),
            })
        })
        .collect()
}

/// Describes the exceptions of the function defined or called at the cursor.
pub(crate) fn hover(
    snapshot: &DocumentSnapshot,
    position: &types::TextDocumentPositionParams,
) -> Option<types::Hover> {
    let query = snapshot.query();
    if query.as_notebook().is_some() {
        return None;
    }
    let path = query.file_path()?;
    let throws = snapshot.throws().snapshot(&path)?;
    let file = throws.file(&path)?;
    // The analysis has the document's current contents, so positions map onto its text.
    let offset = types::Range::new(position.position, position.position)
        .to_text_range(file.source(), file.line_index(), snapshot.encoding())
        .start();
    let hover = throws.hover_at_offset(&path, offset)?;
    let value = hover
        .targets
        .iter()
        .map(hover_markdown)
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    Some(types::Hover {
        contents: types::MarkupContent {
            kind: types::MarkupKind::Markdown,
            value,
        }
        .into(),
        range: Some(
            hover
                .range
                .to_range(file.source(), file.line_index(), snapshot.encoding()),
        ),
    })
}

fn hover_markdown(target: &HoverTarget) -> String {
    let function = &target.function;
    let mut output = format!("**`{}`**", function.symbol);
    if let Some(contract) = &target.contract {
        let declared = if contract.is_empty() {
            "nothing".to_string()
        } else {
            contract
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = write!(output, "\n\nDeclares: {declared}");
    }
    if function.exceptions.is_empty() {
        output.push_str("\n\nNo exceptions inferred.");
    } else {
        output.push_str("\n\nMay raise:\n");
        for exception in &function.exceptions {
            if exception.qualified_name == format!("builtins.{}", exception.name) {
                let _ = write!(output, "\n- `{}`\n", exception.name);
            } else {
                let _ = write!(
                    output,
                    "\n- `{}` (`{}`)\n",
                    exception.name, exception.qualified_name
                );
            }
            let mut chain = String::new();
            render_chain(&mut chain, &exception.chain, "");
            output.push_str("\n  ```text\n");
            for line in chain.lines() {
                let _ = writeln!(output, "  {}", line.trim_end());
            }
            output.push_str("  ```\n");
        }
    }
    if !function.complete {
        output.push_str(
            "\n\n_Other exceptions may escape through unresolved calls or calls beyond the depth limit._",
        );
    }
    output.truncate(output.trim_end().len());
    output
}
