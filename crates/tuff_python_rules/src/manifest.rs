use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) python_version: String,
    pub(crate) rules: Vec<RuleManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuleManifest {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) source_patterns: Vec<String>,
    pub(crate) node_kinds: Vec<String>,
    pub(crate) watch_files: Vec<String>,
    pub(crate) cacheable: bool,
    pub(crate) path: PathBuf,
}

const DIRECTORY: &str = "tuff-python-rules";

fn cache_file(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(DIRECTORY).join(format!("{key}.json"))
}

pub(crate) fn read_cached(cache_dir: &Path, key: &str) -> Option<Manifest> {
    let path = cache_file(cache_dir, key);
    let contents = fs::read(&path).ok()?;
    match serde_json::from_slice(&contents) {
        Ok(manifest) => Some(manifest),
        Err(error) => {
            log::debug!("Ignoring unreadable manifest {}: {error}", path.display());
            None
        }
    }
}

pub(crate) fn write_cached(cache_dir: &Path, key: &str, manifest: &Manifest) -> Result<()> {
    let path = cache_file(cache_dir, key);
    let directory = cache_dir.join(DIRECTORY);
    fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    // Ruff writes the same file when it initializes the cache directory.
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(cache_dir.join(".gitignore"))
    {
        Ok(mut file) => file.write_all(b"# Automatically created by ruff.\n*\n")?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = directory.join(format!("{key}.{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec(manifest)?)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, &path).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Rule names share the grammar of native rule names so `# tuff: ignore[...]` can name them.
pub(crate) fn validate_name(rule: &RuleManifest) -> Result<()> {
    let valid = rule.name.split('-').enumerate().all(|(index, segment)| {
        let mut bytes = segment.bytes();
        bytes.next().is_some_and(|first| {
            first.is_ascii_lowercase() || (index > 0 && first.is_ascii_digit())
        }) && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    });
    if !valid {
        bail!(
            "Python rule name `{}` in {} must be lower kebab-case, such as `no-print-call`",
            rule.name,
            rule.path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str) -> RuleManifest {
        RuleManifest {
            name: name.to_owned(),
            description: String::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            source_patterns: Vec::new(),
            node_kinds: Vec::new(),
            watch_files: Vec::new(),
            cacheable: true,
            path: PathBuf::from("rule.py"),
        }
    }

    #[test]
    fn rule_names_follow_the_ignore_comment_grammar() {
        for name in ["no-print", "rule2", "a-1b"] {
            assert!(validate_name(&rule(name)).is_ok(), "{name}");
        }
        for name in [
            "",
            "NoPrint",
            "no_print",
            "-no",
            "no-",
            "no--print",
            "1rule",
        ] {
            assert!(validate_name(&rule(name)).is_err(), "{name}");
        }
    }

    #[test]
    fn cached_manifest_round_trips_by_key() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            python_version: "3.12.1".to_owned(),
            rules: vec![rule("no-print")],
        };
        assert_eq!(read_cached(directory.path(), "key"), None);
        write_cached(directory.path(), "key", &manifest).unwrap();
        assert_eq!(read_cached(directory.path(), "key"), Some(manifest));
        assert_eq!(read_cached(directory.path(), "other"), None);
        assert!(directory.path().join(".gitignore").is_file());
    }
}
