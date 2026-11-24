//! `[tool.tuff.throws]` configuration.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

/// Where a function's exceptions may come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Explicit `raise` statements, including propagation from callees.
    Raise,
    /// `@throws(...)` decorators.
    Decorator,
    /// `Raises` sections in docstrings.
    Docstring,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields, default)]
pub struct ThrowsConfig {
    /// Exception base classes to report on. Empty means every exception for `inspect` and every
    /// exception class defined in the project for `check`.
    pub track: Vec<String>,
    /// File globs (`src/app/api/**`) or symbol globs (`app.api.*`) of functions that must not
    /// let tracked exceptions escape.
    pub boundaries: Vec<String>,
    pub sources: Vec<SourceKind>,
    /// Maximum number of calls an exception may propagate through.
    pub max_depth: Option<usize>,
    /// File globs to analyze. Empty means every Python file in the project.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Decorators that preserve the wrapped function's exceptions, in addition to the
    /// built-in ones such as `functools.wraps` and `property`.
    pub transparent_decorators: Vec<String>,
    /// Decorators that handle every exception raised by the wrapped function.
    pub suppressing_decorators: Vec<String>,
}

impl Default for ThrowsConfig {
    fn default() -> Self {
        Self {
            track: Vec::new(),
            boundaries: Vec::new(),
            sources: vec![
                SourceKind::Raise,
                SourceKind::Decorator,
                SourceKind::Docstring,
            ],
            max_depth: None,
            include: Vec::new(),
            exclude: Vec::new(),
            transparent_decorators: Vec::new(),
            suppressing_decorators: Vec::new(),
        }
    }
}

impl ThrowsConfig {
    pub fn uses(&self, source: SourceKind) -> bool {
        self.sources.contains(&source)
    }
}

/// A loaded configuration and the directory paths in it are relative to.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: ThrowsConfig,
    pub root: PathBuf,
    pub source: Option<PathBuf>,
}

/// Finds the nearest `tuff.toml`, `.tuff.toml`, or `pyproject.toml` with a `[tool.tuff]` table
/// at or above `start`, and reads its throws section.
///
/// Without a configuration file, the defaults apply and `start` is the root.
pub fn discover(start: &Path) -> Result<LoadedConfig> {
    for directory in start.ancestors() {
        for name in ["tuff.toml", ".tuff.toml"] {
            let path = directory.join(name);
            if path.is_file() {
                let table = read_table(&path)?;
                return loaded(table.get("throws"), directory, path);
            }
        }
        let path = directory.join("pyproject.toml");
        if path.is_file() {
            let table = read_table(&path)?;
            if let Some(tuff) = table.get("tool").and_then(|tool| tool.get("tuff")) {
                return loaded(tuff.get("throws"), directory, path);
            }
        }
    }
    Ok(LoadedConfig {
        config: ThrowsConfig::default(),
        root: start.to_path_buf(),
        source: None,
    })
}

fn read_table(path: &Path) -> Result<toml::Table> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
}

fn loaded(value: Option<&toml::Value>, root: &Path, source: PathBuf) -> Result<LoadedConfig> {
    let config = match value {
        Some(value) => value
            .clone()
            .try_into()
            .with_context(|| format!("invalid throws configuration in {}", source.display()))?,
        None => ThrowsConfig::default(),
    };
    Ok(LoadedConfig {
        config,
        root: root.to_path_buf(),
        source: Some(source),
    })
}

/// Matches functions by project-relative path or qualified symbol name.
///
/// A pattern containing `/` or ending in `.py` matches paths; any other pattern matches
/// qualified names such as `app.api.handlers.create_order`.
#[derive(Debug)]
pub struct FunctionMatcher {
    paths: GlobSet,
    symbols: GlobSet,
}

impl FunctionMatcher {
    pub fn new(patterns: &[String]) -> Result<Self> {
        let mut paths = GlobSetBuilder::new();
        let mut symbols = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = Glob::new(pattern).with_context(|| format!("invalid glob `{pattern}`"))?;
            if pattern.contains('/') || is_python_path(pattern) {
                paths.add(glob);
            } else {
                symbols.add(glob);
            }
        }
        Ok(Self {
            paths: paths.build()?,
            symbols: symbols.build()?,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.symbols.is_empty()
    }

    pub fn matches(&self, relative_path: &str, symbol: &str) -> bool {
        self.paths.is_match(relative_path) || self.symbols.is_match(symbol)
    }
}

pub(crate) fn is_python_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("py"))
}

/// Matches project-relative file paths against include and exclude globs.
#[derive(Debug)]
pub struct FileFilter {
    include: GlobSet,
    exclude: GlobSet,
}

impl FileFilter {
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self> {
        Ok(Self {
            include: glob_set(include)?,
            exclude: glob_set(exclude)?,
        })
    }

    pub fn matches(&self, relative_path: &str) -> bool {
        (self.include.is_empty() || self.include.is_match(relative_path))
            && !self.exclude.is_match(relative_path)
    }
}

fn glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid glob `{pattern}`"))?);
    }
    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[test]
    fn pyproject_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            indoc! {r#"
                [tool.tuff.throws]
                track = ["app.errors.DomainError"]
                boundaries = ["app.api.*"]
                sources = ["raise", "decorator"]
                max-depth = 3
                transparent-decorators = ["app.retry"]
            "#},
        )
        .unwrap();
        let nested = dir.path().join("src/app");
        std::fs::create_dir_all(&nested).unwrap();

        let loaded = discover(&nested).unwrap();
        assert_eq!(loaded.root, dir.path());
        assert_eq!(loaded.config.track, ["app.errors.DomainError"]);
        assert_eq!(loaded.config.boundaries, ["app.api.*"]);
        assert_eq!(
            loaded.config.sources,
            [SourceKind::Raise, SourceKind::Decorator]
        );
        assert_eq!(loaded.config.max_depth, Some(3));
        assert_eq!(loaded.config.transparent_decorators, ["app.retry"]);
    }

    #[test]
    fn tuff_toml_section_takes_precedence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.tuff.throws]\ntrack = [\"A\"]\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("tuff.toml"), "[throws]\ntrack = [\"B\"]\n").unwrap();
        assert_eq!(discover(dir.path()).unwrap().config.track, ["B"]);
    }

    #[test]
    fn pyproject_without_tuff_table_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.tuff.throws]\ntrack = [\"Outer\"]\n",
        )
        .unwrap();
        let inner = dir.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(inner.join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        let loaded = discover(&inner).unwrap();
        assert_eq!(loaded.config.track, ["Outer"]);
        assert_eq!(loaded.root, dir.path());
    }

    #[test]
    fn defaults_without_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tuff.toml"), "line-length = 100\n").unwrap();
        let loaded = discover(dir.path()).unwrap();
        assert_eq!(loaded.config, ThrowsConfig::default());
    }

    #[test]
    fn unknown_field_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tuff.toml"),
            "[throws]\nmode = \"strict\"\n",
        )
        .unwrap();
        let error = discover(dir.path()).unwrap_err();
        assert!(format!("{error:#}").contains("unknown field `mode`"));
    }

    #[test]
    fn function_matcher_paths_and_symbols() {
        let matcher =
            FunctionMatcher::new(&["app/api/**".to_string(), "app.jobs.*".to_string()]).unwrap();
        assert!(matcher.matches("app/api/orders.py", "app.api.orders.create"));
        assert!(matcher.matches("app/jobs.py", "app.jobs.nightly"));
        assert!(!matcher.matches("app/core.py", "app.core.run"));
    }

    #[test]
    fn file_filter() {
        let filter = FileFilter::new(&["src/**".to_string()], &["src/gen/**".to_string()]).unwrap();
        assert!(filter.matches("src/app/core.py"));
        assert!(!filter.matches("src/gen/models.py"));
        assert!(!filter.matches("tests/test_core.py"));
    }
}
