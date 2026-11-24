//! Editor support for repository-local Python rules.
//!
//! Python rule findings reach the editor through Ruff's `check_path` like any other lint
//! diagnostic. This module covers what Ruff's own machinery does not know about Python
//! rules: they are suppressed only by `# tuff: ignore[...]` comments, their documentation
//! lives in the plugin manifest, and editing a rule module must reload the settings that
//! loaded it.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lsp_types::{
    BaseUri, ClientCapabilities, DidChangeWatchedFilesRegistrationOptions, FileSystemWatcher,
    GlobPattern, Notification as _, Registration, RegistrationParams, RegistrationRequest,
    RelativePattern, Save, Unregistration, UnregistrationParams, UnregistrationRequest, Uri,
};
use ruff_db::diagnostic::Diagnostic;
use ruff_diagnostics::Edit;
use ruff_linter::directives::Directives;
use ruff_linter::fs::normalize_path;
use ruff_linter::registry::Rule;
use ruff_linter::settings::LinterSettings;
use ruff_python_index::Indexer;
use ruff_source_file::LineRanges;
use ruff_text_size::{TextLen, TextRange, TextSize};
use ruff_workspace::Settings;
use tuff_lint::suppression::name_ranges;
use tuff_python_rules::RuleInfo;

use crate::session::{Client, Session};

const WATCHER_ID: &str = "tuff-python-rules-watch";

/// Replaces the suppression edits of Python rule diagnostics with `# tuff: ignore[...]`
/// comments. Ruff's `# noqa` and `# ruff: ignore` comments do not suppress Python rules.
///
/// `edits` holds one entry per diagnostic, in the same order.
pub(crate) fn replace_suppression_edits(
    diagnostics: &[Diagnostic],
    edits: &mut [Option<Edit>],
    settings: &LinterSettings,
    source: &str,
    indexer: &Indexer,
    directives: &Directives,
) {
    let Some(rules) = &settings.tuff.python_rules else {
        return;
    };
    for (diagnostic, edit) in diagnostics.iter().zip(edits) {
        if let Some(name) = diagnostic.id().as_lint()
            && diagnostic
                .secondary_code()
                .is_some_and(|code| code.as_str() == name.as_str())
            && rules.rule(name.as_str()).is_some()
            && let Some(range) = diagnostic.range()
        {
            *edit = Some(ignore_edit(
                name.as_str(),
                directives.noqa_line_for.resolve(range.start()),
                source,
                indexer,
            ));
        }
    }
}

/// Adds `name` to the `# tuff: ignore[...]` comment on the line of `offset`, or appends a
/// new comment to that line.
fn ignore_edit(name: &str, offset: TextSize, source: &str, indexer: &Indexer) -> Edit {
    let line_start = source.line_start(offset);
    let line_end = source.line_end(offset);
    let existing = indexer
        .comment_ranges()
        .comments_in_range(TextRange::new(line_start, line_end))
        .last()
        .and_then(|comment| {
            let names = name_ranges(&source[*comment]).ok().flatten()?;
            Some(comment.start() + names.last()?.end())
        });
    if let Some(end) = existing {
        return Edit::insertion(format!(", {name}"), end);
    }
    let content = source[TextRange::new(line_start, line_end)].trim_end();
    let separator = if content.trim_start().is_empty() {
        ""
    } else {
        "  "
    };
    Edit::range_replacement(
        format!("{separator}# tuff: ignore[{name}]"),
        TextRange::new(line_start + content.text_len(), line_end),
    )
}

/// Describes the rule named at `offset` in a `# tuff: ignore[...]` comment.
///
/// Native rules are described by `describe_native`; Python rules by their plugin manifest.
pub(crate) fn ignore_comment_hover(
    source: &str,
    comment: TextRange,
    offset: TextSize,
    settings: &Settings,
    describe_native: impl FnOnce(Rule) -> String,
) -> Option<(TextRange, String)> {
    let range = name_ranges(&source[comment])
        .ok()
        .flatten()?
        .into_iter()
        .map(|range| range + comment.start())
        .find(|range| range.contains(offset))?;
    let name = &source[range];
    let text = if let Ok(rule) = Rule::from_name(name) {
        describe_native(rule)
    } else if let Some(rule) = settings
        .linter
        .tuff
        .python_rules
        .as_ref()
        .and_then(|rules| rules.rule(name))
    {
        describe_python_rule(rule, &settings.linter.tuff.root)
    } else {
        format!("{name}: Rule not found")
    };
    Some((range, text))
}

fn describe_python_rule(rule: RuleInfo<'_>, root: &Path) -> String {
    let mut output = String::new();
    let path = rule.path.strip_prefix(root).unwrap_or(rule.path);
    let _ = write!(
        output,
        "# {}\n\nPython rule defined in `{}`.\n\n",
        rule.name,
        path.display()
    );
    if !rule.enabled {
        output.push_str("This rule is disabled in this configuration.\n\n");
    }
    output.push_str(rule.description.trim());
    output.trim_end().to_owned()
}

/// The Python rule sources that loaded settings depend on: plugin directories and the files
/// or directories rules list in `watch_files`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WatchedPaths {
    plugin_dirs: BTreeSet<PathBuf>,
    watch_files: BTreeSet<PathBuf>,
}

impl WatchedPaths {
    pub(crate) fn extend<'a>(&mut self, settings: impl IntoIterator<Item = &'a Settings>) {
        for rules in settings
            .into_iter()
            .filter_map(|settings| settings.linter.tuff.python_rules.as_deref())
        {
            self.plugin_dirs
                .extend(rules.plugin_dirs().iter().map(normalize_path));
            self.watch_files
                .extend(rules.watched_files().map(normalize_path));
        }
    }

    pub(crate) fn union(&mut self, other: &Self) {
        self.plugin_dirs.extend(other.plugin_dirs.iter().cloned());
        self.watch_files.extend(other.watch_files.iter().cloned());
    }

    pub(crate) fn contains(&self, path: &Path) -> bool {
        self.watch_files
            .iter()
            .any(|watched| path.starts_with(watched))
            || (path.extension() == Some(OsStr::new("py"))
                && self
                    .plugin_dirs
                    .iter()
                    .any(|directory| path.starts_with(directory)))
    }

    fn is_empty(&self) -> bool {
        self.plugin_dirs.is_empty() && self.watch_files.is_empty()
    }
}

/// Client-side file watchers for Python rule sources.
///
/// Ruff's own watchers only cover configuration files. Plugin directories can live outside
/// the workspace, so they get a separate registration that follows the loaded settings.
#[derive(Debug)]
pub(crate) struct Watchers {
    dynamic_registration: bool,
    relative_patterns: bool,
    registered: WatchedPaths,
}

impl Watchers {
    pub(crate) fn new(capabilities: &ClientCapabilities) -> Self {
        let watched_files = capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.did_change_watched_files.as_ref());
        Self {
            dynamic_registration: watched_files
                .and_then(|watched_files| watched_files.dynamic_registration)
                .unwrap_or_default(),
            relative_patterns: watched_files
                .and_then(|watched_files| watched_files.relative_pattern_support)
                .unwrap_or_default(),
            registered: WatchedPaths::default(),
        }
    }

    /// Without file watchers, saving a rule module in the editor is the only signal that
    /// it changed.
    pub(crate) fn reload_on_save(&self) -> bool {
        !self.dynamic_registration
    }

    /// Records `paths` as watched and returns the requests that replace the previous
    /// registration.
    pub(crate) fn update(&mut self, paths: WatchedPaths) -> WatcherUpdate {
        if !self.dynamic_registration || paths == self.registered {
            return WatcherUpdate::default();
        }
        let method = lsp_types::DidChangeWatchedFilesNotification::METHOD.to_string();
        let unregister = (!self.registered.is_empty()).then(|| UnregistrationParams {
            unregisterations: vec![Unregistration {
                id: WATCHER_ID.to_owned(),
                method: method.clone(),
            }],
        });
        let register = (!paths.is_empty()).then(|| RegistrationParams {
            registrations: vec![Registration {
                id: WATCHER_ID.to_owned(),
                method,
                register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: self.file_system_watchers(&paths),
                })
                .ok(),
            }],
        });
        self.registered = paths;
        WatcherUpdate {
            unregister,
            register,
        }
    }

    fn file_system_watchers(&self, paths: &WatchedPaths) -> Vec<FileSystemWatcher> {
        let plugin_dirs = paths
            .plugin_dirs
            .iter()
            .filter_map(|directory| self.glob(directory, "**/*.py"));
        let watch_files = paths.watch_files.iter().filter_map(|watched| {
            if watched.is_dir() {
                self.glob(watched, "**/*")
            } else {
                self.glob(watched.parent()?, watched.file_name()?.to_str()?)
            }
        });
        plugin_dirs
            .chain(watch_files)
            .map(|glob_pattern| FileSystemWatcher {
                glob_pattern,
                kind: None,
            })
            .collect()
    }

    fn glob(&self, base: &Path, pattern: &str) -> Option<GlobPattern> {
        if self.relative_patterns {
            Some(GlobPattern::RelativePattern(RelativePattern {
                base_uri: BaseUri::Uri(Uri::from_file_path(base).ok()?),
                pattern: pattern.to_owned(),
            }))
        } else {
            let base = base.to_str()?.replace('\\', "/");
            Some(GlobPattern::Pattern(format!(
                "{}/{pattern}",
                base.trim_end_matches('/')
            )))
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct WatcherUpdate {
    unregister: Option<UnregistrationParams>,
    register: Option<RegistrationParams>,
}

/// Registers client file watchers for the Python rule sources of every workspace.
pub(crate) fn sync_watchers(session: &mut Session, client: &Client) {
    let update = session.update_python_rule_watchers();
    if let Some(params) = update.unregister
        && let Err(error) =
            client.send_request::<UnregistrationRequest>(session, params, |_, ()| {})
    {
        tracing::error!("Failed to unregister Python rule file watchers: {error}");
    }
    if let Some(params) = update.register
        && let Err(error) = client.send_request::<RegistrationRequest>(session, params, |_, ()| {
            tracing::debug!("Registered Python rule file watchers");
        })
    {
        tracing::error!("Failed to register Python rule file watchers: {error}");
    }
}

/// Whether the server asks for `textDocument/didSave`, which reloads Python rules when the
/// client cannot watch their files.
pub(crate) fn save_notifications(capabilities: &ClientCapabilities) -> Option<Save> {
    Watchers::new(capabilities)
        .reload_on_save()
        .then_some(Save::Bool(true))
}
