use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::{hash_field, hex, worker};

#[derive(Debug, Default)]
pub(crate) struct PluginFiles {
    /// Modules loaded for their rules, in a stable order.
    pub(crate) rules: Vec<PathBuf>,
    /// Every Python file that can affect the rules, including `_*.py` helpers.
    pub(crate) all: Vec<PathBuf>,
}

pub(crate) fn discover(directories: &[PathBuf]) -> Result<PluginFiles> {
    let mut files = PluginFiles::default();
    for directory in directories {
        if !directory.is_dir() {
            bail!("plugin directory {} does not exist", directory.display());
        }
        let walker = WalkDir::new(directory)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !entry.file_type().is_dir()
                    || !is_skipped_directory(entry.file_name())
            });
        for entry in walker {
            let entry = entry.with_context(|| format!("failed to read {}", directory.display()))?;
            let path = entry.path();
            if !entry.file_type().is_file() || path.extension() != Some(OsStr::new("py")) {
                continue;
            }
            if !entry.file_name().to_string_lossy().starts_with('_') {
                files.rules.push(path.to_path_buf());
            }
            files.all.push(path.to_path_buf());
        }
    }
    Ok(files)
}

fn is_skipped_directory(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name == "tests" || name == "__pycache__" || name.starts_with('.')
}

/// Picks the interpreter: the configured one, then the active virtual environment, then
/// the project's `.venv`, then `python3` or `python` on `PATH`.
pub(crate) fn find_python(explicit: Option<&Path>, root: &Path) -> Result<PathBuf> {
    if let Some(python) = explicit {
        if python.components().count() == 1 && !python.is_absolute() {
            return which::which(python).with_context(|| {
                format!("Python interpreter `{}` is not on PATH", python.display())
            });
        }
        if !python.is_file() {
            bail!("Python interpreter {} does not exist", python.display());
        }
        return Ok(python.to_path_buf());
    }
    let environments = std::env::var_os("VIRTUAL_ENV")
        .map(PathBuf::from)
        .into_iter()
        .chain(std::iter::once(root.join(".venv")));
    for environment in environments {
        let python = if cfg!(windows) {
            environment.join("Scripts").join("python.exe")
        } else {
            environment.join("bin").join("python")
        };
        if python.is_file() {
            return Ok(python);
        }
    }
    which::which("python3")
        .or_else(|_| which::which("python"))
        .context("no Python interpreter found; set `python` under [tool.tuff.lint]")
}

/// Identifies a manifest without starting Python: the worker sources, the interpreter's
/// resolved file and modification time, and every plugin file's path and contents.
pub(crate) fn manifest_key(
    python: &Path,
    directories: &[PathBuf],
    files: &PluginFiles,
) -> Result<String> {
    let mut hasher = Sha256::new();
    for (name, source) in worker::MODULES {
        hash_field(&mut hasher, name.as_bytes());
        hash_field(&mut hasher, source.as_bytes());
    }
    let interpreter = fs::canonicalize(python)
        .with_context(|| format!("failed to resolve {}", python.display()))?;
    let metadata = fs::metadata(&interpreter)
        .with_context(|| format!("failed to read {}", interpreter.display()))?;
    hash_field(&mut hasher, python.as_os_str().as_encoded_bytes());
    hash_field(&mut hasher, interpreter.as_os_str().as_encoded_bytes());
    hash_field(&mut hasher, &metadata.len().to_le_bytes());
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    hash_field(&mut hasher, &modified.to_le_bytes());
    for directory in directories {
        hash_field(&mut hasher, directory.as_os_str().as_encoded_bytes());
    }
    for file in &files.all {
        hash_field(&mut hasher, file.as_os_str().as_encoded_bytes());
        hash_field(
            &mut hasher,
            &fs::read(file).with_context(|| format!("failed to read {}", file.display()))?,
        );
    }
    Ok(hex(hasher.finalize().as_slice()))
}

/// Hashes a watched file or every file below a watched directory. A missing path hashes
/// differently from an empty file.
pub(crate) fn hash_path(hasher: &mut Sha256, path: &Path) -> Result<()> {
    if path.is_dir() {
        for entry in WalkDir::new(path).sort_by_file_name() {
            let entry = entry.with_context(|| format!("failed to read {}", path.display()))?;
            if entry.file_type().is_file() {
                hash_field(hasher, entry.path().as_os_str().as_encoded_bytes());
                hash_field(
                    hasher,
                    &fs::read(entry.path())
                        .with_context(|| format!("failed to read {}", entry.path().display()))?,
                );
            }
        }
    } else if path.is_file() {
        hash_field(
            hasher,
            &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
        );
    } else {
        hash_field(hasher, b"missing");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_separates_rules_from_helpers_and_skips_tests() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for file in [
            "a_rule.py",
            "_helpers.py",
            "__init__.py",
            "nested/b_rule.py",
            "tests/test_rule.py",
            "__pycache__/cached.py",
            "notes.txt",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let files = discover(&[root.to_path_buf()]).unwrap();
        let relative = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|path| {
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(relative(&files.rules), ["a_rule.py", "nested/b_rule.py"]);
        assert_eq!(
            relative(&files.all),
            [
                "__init__.py",
                "_helpers.py",
                "a_rule.py",
                "nested/b_rule.py"
            ]
        );
    }

    #[test]
    fn missing_plugin_directory_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = discover(&[directory.path().join("missing")]).unwrap_err();
        assert!(error.to_string().contains("does not exist"), "{error}");
    }

    #[test]
    fn helper_changes_change_the_manifest_key() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("rule.py"), "from _shared import value\n").unwrap();
        fs::write(root.join("_shared.py"), "value = 1\n").unwrap();
        let python = root.join("python");
        fs::write(&python, "").unwrap();
        let key = || {
            let files = discover(&[root.to_path_buf()]).unwrap();
            manifest_key(&python, &[root.to_path_buf()], &files).unwrap()
        };
        let before = key();
        assert_eq!(before, key());
        fs::write(root.join("_shared.py"), "value = 2\n").unwrap();
        assert_ne!(before, key());
    }
}
