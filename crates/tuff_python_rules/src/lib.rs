//! Repository-local lint rules written in Python.
//!
//! Tuff does not link against Python. Rules run in CPython worker processes that are
//! started only when a checked file passes a rule's Rust-side filters, and they talk to
//! Tuff over length-prefixed JSON frames.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use ruff_python_ast::{Expr, ModModule};
use ruff_python_trivia::CommentRanges;
use ruff_text_size::{TextRange, TextSize};
use rustc_hash::FxHashSet;
use sha2::{Digest, Sha256};

use crate::filter::RuleFilter;
use crate::manifest::{Manifest, RuleManifest};
use crate::worker::{FilePayload, RawFinding, WorkerPool, WorkerSpec};

mod discovery;
mod filter;
mod manifest;
mod node_kinds;
mod worker;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginConfig {
    /// Absolute directories searched recursively for rule modules.
    pub plugin_dirs: Vec<PathBuf>,
    /// The interpreter to run rules with. A bare name is looked up on `PATH`.
    pub python: Option<PathBuf>,
    pub workers: Option<NonZeroUsize>,
}

/// Rules discovered in a project's plugin directories.
#[derive(Debug)]
pub struct Plugins {
    root: PathBuf,
    plugin_dirs: Vec<PathBuf>,
    rules: Vec<LoadedRule>,
    fingerprint: String,
    pool: Arc<WorkerPool>,
}

#[derive(Debug)]
struct LoadedRule {
    name: &'static str,
    manifest: RuleManifest,
    filter: RuleFilter,
}

impl Plugins {
    /// Discovers rules, reusing the manifest cached under `cache_dir` when neither the
    /// plugin files nor the interpreter changed. Python only starts on a cache miss.
    pub fn load(config: &PluginConfig, root: &Path, cache_dir: &Path) -> Result<Self> {
        let files = discovery::discover(&config.plugin_dirs)?;
        let python = discovery::find_python(config.python.as_deref(), root)?;
        let key = discovery::manifest_key(&python, &config.plugin_dirs, &files)?;
        let capacity = config.workers.map_or_else(
            || std::thread::available_parallelism().map_or(1, NonZeroUsize::get),
            NonZeroUsize::get,
        );
        let spec = WorkerSpec {
            python,
            root: root.to_path_buf(),
            plugin_dirs: config.plugin_dirs.clone(),
            rule_files: files.rules.clone(),
        };
        let pool = worker::shared_pool(spec.clone(), &key, capacity);
        let manifest = if files.rules.is_empty() {
            Manifest {
                python_version: String::new(),
                rules: Vec::new(),
            }
        } else if let Some(manifest) = manifest::read_cached(cache_dir, &key) {
            manifest
        } else {
            let manifest = pool.load_manifest()?;
            if let Err(error) = manifest::write_cached(cache_dir, &key, &manifest) {
                log::warn!("Failed to cache the Python rule manifest: {error:#}");
            }
            manifest
        };
        let watched = watched_key(root, &manifest)?;
        // A rule may read its watched files when its module is imported, so workers started
        // before a watched file changed must not serve the new contents.
        let pool = match &watched {
            Some(watched) => worker::shared_pool(spec, &format!("{key}:{watched}"), capacity),
            None => pool,
        };
        Self::from_manifest(
            root,
            config.plugin_dirs.clone(),
            manifest,
            &key,
            watched.as_deref(),
            pool,
        )
    }

    fn from_manifest(
        root: &Path,
        plugin_dirs: Vec<PathBuf>,
        manifest: Manifest,
        key: &str,
        watched: Option<&str>,
        pool: Arc<WorkerPool>,
    ) -> Result<Self> {
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, key.as_bytes());
        hash_field(&mut hasher, manifest.python_version.as_bytes());
        hash_field(&mut hasher, watched.unwrap_or_default().as_bytes());
        let mut rules = Vec::with_capacity(manifest.rules.len());
        let mut seen = FxHashSet::default();
        for rule in manifest.rules {
            manifest::validate_name(&rule)?;
            if !seen.insert(rule.name.clone()) {
                bail!("Python rule `{}` is defined more than once", rule.name);
            }
            let filter = RuleFilter::new(&rule).with_context(|| {
                format!(
                    "invalid metadata for Python rule `{}` in {}",
                    rule.name,
                    rule.path.display()
                )
            })?;
            rules.push(LoadedRule {
                name: intern(&rule.name),
                manifest: rule,
                filter,
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
            plugin_dirs,
            rules,
            fingerprint: hex(hasher.finalize().as_slice()),
            pool,
        })
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.rules.iter().map(|rule| rule.name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.rules.iter().any(|rule| rule.name == name)
    }

    /// Selects the rules that run. `options` maps rule names to their configured options.
    pub fn activate(
        self,
        enabled: impl Fn(&str) -> bool,
        options: &BTreeMap<String, serde_json::Value>,
    ) -> PythonRules {
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, self.fingerprint.as_bytes());
        let mut active = Vec::new();
        let mut cacheable = true;
        for (index, rule) in self.rules.iter().enumerate() {
            if !enabled(rule.name) {
                continue;
            }
            let options = options
                .get(rule.name)
                .cloned()
                .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
            hash_field(&mut hasher, rule.name.as_bytes());
            hash_field(&mut hasher, options.to_string().as_bytes());
            cacheable &= rule.manifest.cacheable;
            active.push(ActiveRule { index, options });
        }
        if !cacheable {
            // A run-specific value keeps files checked by uncacheable rules out of the
            // lint cache.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            hash_field(
                &mut hasher,
                format!("{}:{now}", std::process::id()).as_bytes(),
            );
        }
        PythonRules {
            fingerprint: hex(hasher.finalize().as_slice()),
            plugins: self,
            active,
        }
    }
}

#[derive(Debug)]
struct ActiveRule {
    index: usize,
    options: serde_json::Value,
}

/// The enabled Python rules of one configuration.
#[derive(Debug)]
pub struct PythonRules {
    plugins: Plugins,
    active: Vec<ActiveRule>,
    fingerprint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleInfo<'a> {
    pub name: &'static str,
    pub description: &'a str,
    /// The module that defines the rule.
    pub path: &'a Path,
    pub enabled: bool,
}

/// A file Ruff has parsed and analyzed.
pub struct CheckedFile<'a> {
    pub path: &'a Path,
    pub source: &'a str,
    pub module: &'a ModModule,
    pub comments: &'a CommentRanges,
    pub qualified_name: &'a dyn Fn(&Expr) -> Option<String>,
    /// Returns `true` for rules ignored in this file, such as through `per-file-ignores`.
    pub is_ignored: &'a dyn Fn(&str) -> bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub range: TextRange,
    pub message: String,
    pub edits: Vec<Edit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: TextRange,
    pub replacement: String,
}

/// A rule or worker failure that prevented a complete check of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub rule: Option<&'static str>,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct FileReport {
    pub findings: Vec<Finding>,
    pub failures: Vec<Failure>,
}

impl PythonRules {
    /// Changes whenever rule code, interpreter version, watched files, selection, or
    /// options change.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The names of all discovered rules, whether or not they are enabled.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.plugins.names()
    }

    /// Returns the interned name of a discovered rule, whether or not it is enabled.
    pub fn rule_name(&self, name: &str) -> Option<&'static str> {
        self.plugins
            .rules
            .iter()
            .find(|rule| rule.name == name)
            .map(|rule| rule.name)
    }

    /// Describes a discovered rule, whether or not it is enabled.
    pub fn rule(&self, name: &str) -> Option<RuleInfo<'_>> {
        let index = self
            .plugins
            .rules
            .iter()
            .position(|rule| rule.name == name)?;
        let rule = &self.plugins.rules[index];
        Some(RuleInfo {
            name: rule.name,
            description: &rule.manifest.description,
            path: &rule.manifest.path,
            enabled: self.active.iter().any(|active| active.index == index),
        })
    }

    pub fn plugin_dirs(&self) -> &[PathBuf] {
        &self.plugins.plugin_dirs
    }

    /// The files and directories that rules list in `watch_files`.
    pub fn watched_files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.plugins.rules.iter().flat_map(|rule| {
            rule.manifest
                .watch_files
                .iter()
                .map(|watched| self.plugins.root.join(watched))
        })
    }

    pub fn has_active_rules(&self) -> bool {
        !self.active.is_empty()
    }

    pub fn check(&self, file: &CheckedFile<'_>) -> FileReport {
        let relative = relative_path(&self.plugins.root, file.path);
        let source = file.source.as_bytes();
        let mut candidates: Vec<&ActiveRule> = self
            .active
            .iter()
            .filter(|active| {
                let rule = &self.plugins.rules[active.index];
                !(file.is_ignored)(rule.name)
                    && rule.filter.matches_path(&relative)
                    && rule.filter.matches_source(source)
            })
            .collect();
        if candidates
            .iter()
            .any(|active| self.plugins.rules[active.index].filter.uses_node_kinds())
        {
            let present = node_kinds::present(file.module);
            candidates.retain(|active| {
                self.plugins.rules[active.index]
                    .filter
                    .matches_node_kinds(present)
            });
        }
        if candidates.is_empty() {
            return FileReport::default();
        }

        // CPython rejects a byte order mark in `str` input, so it is stripped and spans are
        // shifted back afterwards.
        let bom = if file.source.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        let offset = TextSize::try_from(bom).unwrap_or_default();
        let side_table = filter::side_table(file.module, file.comments, file.qualified_name);
        let shift = |range: TextRange| {
            (
                u32::from(range.start().checked_sub(offset).unwrap_or_default()),
                u32::from(range.end().checked_sub(offset).unwrap_or_default()),
            )
        };
        let payload = FilePayload {
            path: file.path.to_string_lossy().into_owned(),
            rel_path: relative,
            source: &file.source[bom..],
            qualified_names: side_table
                .qualified_names
                .into_iter()
                .map(|(range, name)| {
                    let (start, end) = shift(range);
                    (start, end, name)
                })
                .collect(),
            comments: side_table.comments.into_iter().map(shift).collect(),
            active_rules: candidates
                .iter()
                .map(|active| self.plugins.rules[active.index].name)
                .collect(),
            options_by_rule: candidates
                .iter()
                .map(|active| (self.plugins.rules[active.index].name, &active.options))
                .collect(),
        };
        let result = match self.plugins.pool.check(&payload) {
            Ok(result) => result,
            Err(error) => {
                return FileReport {
                    findings: Vec::new(),
                    failures: vec![Failure {
                        rule: None,
                        message: format!("{error:#}"),
                    }],
                };
            }
        };
        let mut report = FileReport::default();
        for failure in result.errors {
            report.failures.push(Failure {
                rule: failure
                    .rule
                    .as_deref()
                    .and_then(|name| self.rule_name(name)),
                message: failure.message,
            });
        }
        let length = file.source.len();
        for finding in result.findings {
            match self.convert(finding, file.source, bom, length) {
                Ok(finding) => report.findings.push(finding),
                Err(failure) => report.failures.push(failure),
            }
        }
        report
    }

    fn convert(
        &self,
        finding: RawFinding,
        source: &str,
        bom: usize,
        length: usize,
    ) -> std::result::Result<Finding, Failure> {
        let Some(rule) = self.rule_name(&finding.rule) else {
            return Err(Failure {
                rule: None,
                message: format!("Worker reported unknown rule `{}`", finding.rule),
            });
        };
        let range = |start: u64, end: u64| {
            let start = usize::try_from(start).ok()?.checked_add(bom)?;
            let end = usize::try_from(end).ok()?.checked_add(bom)?;
            (start <= end
                && end <= length
                && source.is_char_boundary(start)
                && source.is_char_boundary(end))
            .then(|| {
                TextRange::new(
                    TextSize::try_from(start).unwrap_or_default(),
                    TextSize::try_from(end).unwrap_or_default(),
                )
            })
        };
        let invalid = |start: u64, end: u64| Failure {
            rule: Some(rule),
            message: format!(
                "Rule reported the byte range {start}..{end}, which is not a valid span of the file"
            ),
        };
        let Some(target) = range(finding.start, finding.end) else {
            return Err(invalid(finding.start, finding.end));
        };
        let mut edits = Vec::with_capacity(finding.edits.len());
        for edit in finding.edits {
            let Some(edit_range) = range(edit.start, edit.end) else {
                return Err(invalid(edit.start, edit.end));
            };
            edits.push(Edit {
                range: edit_range,
                replacement: edit.replacement,
            });
        }
        Ok(Finding {
            rule,
            range: target,
            message: finding.message,
            edits,
        })
    }
}

/// Hashes the names and contents of every rule's `watch_files`, or returns `None` when no
/// rule watches files.
fn watched_key(root: &Path, manifest: &Manifest) -> Result<Option<String>> {
    let mut hasher = Sha256::new();
    let mut any = false;
    for rule in &manifest.rules {
        for watched in &rule.watch_files {
            any = true;
            hash_field(&mut hasher, watched.as_bytes());
            discovery::hash_path(&mut hasher, &root.join(watched))?;
        }
    }
    Ok(any.then(|| hex(hasher.finalize().as_slice())))
}

fn relative_path(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut text = String::new();
    for (index, component) in relative.components().enumerate() {
        if index > 0 {
            text.push('/');
        }
        text.push_str(&component.as_os_str().to_string_lossy());
    }
    text
}

/// Rule names live for the whole process because Ruff diagnostics name lints with
/// `&'static str`. Each distinct name is leaked once.
fn intern(name: &str) -> &'static str {
    static NAMES: LazyLock<Mutex<FxHashSet<&'static str>>> = LazyLock::new(Mutex::default);
    let mut names = NAMES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(existing) = names.get(name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
