use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Table;
use tuff_python_rules::PluginConfig;

use crate::builtin::spacing::{self, SpacingPolicy, SpacingRule};
use crate::{RuleDefinition, RuleOptions, builtin};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintOptions {
    #[serde(skip)]
    inherited: Vec<Selection>,
    #[serde(skip)]
    pub source: Option<PathBuf>,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub rules: BTreeMap<String, RuleOptions>,
    #[serde(default, rename = "plugin-dirs")]
    pub plugin_dirs: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub python: Option<PathBuf>,
    #[serde(default, rename = "plugin-workers")]
    pub plugin_workers: Option<NonZeroUsize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Selection {
    enable: Vec<String>,
    disable: Vec<String>,
}

impl LintOptions {
    #[must_use]
    pub fn combine(mut self, parent: Self) -> Self {
        self.source = self.source.or(parent.source);
        self.plugin_dirs = self.plugin_dirs.or(parent.plugin_dirs);
        self.python = self.python.or(parent.python);
        self.plugin_workers = self.plugin_workers.or(parent.plugin_workers);
        let mut inherited = parent.inherited;
        inherited.push(Selection {
            enable: parent.enable,
            disable: parent.disable,
        });
        inherited.append(&mut self.inherited);
        self.inherited = inherited;
        let child_rules = self.rules;
        self.rules = parent.rules;
        for (name, options) in child_rules {
            self.rules.entry(name).or_default().extend(options);
        }
        self
    }

    pub fn selected_codes(&self) -> Vec<&'static str> {
        self.selections()
            .into_iter()
            .filter_map(|(code, enabled)| enabled.then_some(code))
            .collect()
    }

    pub fn ignored_codes(&self) -> Vec<&'static str> {
        self.selections()
            .into_iter()
            .filter_map(|(code, enabled)| (!enabled).then_some(code))
            .collect()
    }

    /// Native rule selections. Names that are not native rules are validated later by
    /// [`LintOptions::python_rule_selection`], once Python rules are discovered.
    fn selections(&self) -> BTreeMap<&'static str, bool> {
        let mut result = BTreeMap::new();
        for (selectors, enabled) in self.layers() {
            result.extend(
                expand_selectors(selectors)
                    .into_iter()
                    .map(|code| (code, enabled)),
            );
        }
        result
    }

    fn layers(&self) -> impl Iterator<Item = (&[String], bool)> {
        self.inherited
            .iter()
            .map(|selection| (&selection.enable, &selection.disable))
            .chain(std::iter::once((&self.enable, &self.disable)))
            .flat_map(|(enable, disable)| [(enable.as_slice(), true), (disable.as_slice(), false)])
    }

    /// Resolves which Python rules run. Discovered Python rules are enabled unless
    /// disabled by name or by `all`. Fails on names that are neither native nor discovered.
    pub fn python_rule_selection(
        &self,
        is_python_rule: impl Fn(&str) -> bool,
    ) -> Result<PythonRuleSelection> {
        let native = builtin::definitions();
        let mut selection = PythonRuleSelection::default();
        for (selectors, enabled) in self.layers() {
            for selector in selectors {
                match selector.as_str() {
                    "all" => {
                        selection.names.clear();
                        selection.all = enabled;
                    }
                    "builtin" | "legacy" => {}
                    name if native.iter().any(|definition| definition.name == name) => {}
                    name if is_python_rule(name) => {
                        selection.names.insert(name.to_owned(), enabled);
                    }
                    name => bail!("unknown Tuff rule `{name}`"),
                }
            }
        }
        Ok(selection)
    }

    /// The plugin settings, when `plugin-dirs` is configured.
    pub fn plugin_config(&self) -> Option<PluginConfig> {
        let plugin_dirs = self.plugin_dirs.clone()?;
        Some(PluginConfig {
            plugin_dirs,
            python: self.python.clone(),
            workers: self.plugin_workers,
        })
    }

    fn resolve_paths(&mut self, directory: &Path) {
        if let Some(plugin_dirs) = &mut self.plugin_dirs {
            for plugin_dir in plugin_dirs {
                *plugin_dir = directory.join(&*plugin_dir);
            }
        }
        // A bare interpreter name is looked up on `PATH` instead.
        if let Some(python) = &mut self.python
            && python.components().count() > 1
        {
            *python = directory.join(&*python);
        }
    }

    pub fn spacing_policy(
        &self,
        formatter_rules: &[SpacingRule],
        enabled: impl Fn(&str) -> bool,
    ) -> Result<SpacingPolicy> {
        let ignored = self.ignored_codes();
        let mut policy = SpacingPolicy::default();
        for rule in SpacingRule::ALL {
            let Some(definition) = builtin::spacing::RULES
                .iter()
                .find(|definition| definition.name == rule.name())
            else {
                continue;
            };
            if enabled(definition.code)
                || (formatter_rules.contains(&rule) && !ignored.contains(&definition.code))
            {
                if let Some(options) = self.rules.get(rule.name()) {
                    policy.options.apply_rule(rule, options)?;
                }
                policy.rules.push(rule);
            }
        }
        spacing::validate(&policy.rules)?;
        Ok(policy)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonRuleSelection {
    all: bool,
    names: BTreeMap<String, bool>,
}

impl Default for PythonRuleSelection {
    fn default() -> Self {
        Self {
            all: true,
            names: BTreeMap::new(),
        }
    }
}

impl PythonRuleSelection {
    pub fn is_enabled(&self, name: &str) -> bool {
        self.names.get(name).copied().unwrap_or(self.all)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Options {
    extend: Option<String>,
    line_length: Option<u16>,
    target_version: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    pub format: Option<Table>,
    #[serde(default)]
    pub lint: LintOptions,
    #[serde(default)]
    pub throws: Option<Table>,
}

impl Options {
    #[must_use]
    pub fn combine(mut self, parent: Self) -> Self {
        self.lint = self.lint.combine(parent.lint);
        self.format = match (self.format, parent.format) {
            (Some(child), Some(mut parent)) => {
                merge_tables(&mut parent, child);
                Some(parent)
            }
            (child, parent) => child.or(parent),
        };
        self.throws = match (self.throws, parent.throws) {
            (Some(child), Some(mut parent)) => {
                merge_tables(&mut parent, child);
                Some(parent)
            }
            (child, parent) => child.or(parent),
        };
        self
    }
}

fn merge_tables(parent: &mut Table, child: Table) {
    for (key, value) in child {
        if let Some(toml::Value::Table(parent)) = parent.get_mut(&key)
            && let toml::Value::Table(child) = value
        {
            merge_tables(parent, child);
        } else {
            parent.insert(key, value);
        }
    }
}

#[derive(Deserialize)]
struct Pyproject {
    tool: Option<Tools>,
}

#[derive(Deserialize)]
struct Tools {
    tuff: Option<Options>,
}

fn read_options(path: &Path) -> Result<Option<Options>> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let options = match path.file_name().and_then(|name| name.to_str()) {
        Some("pyproject.toml") => toml::from_str::<Pyproject>(&contents)?
            .tool
            .and_then(|tool| tool.tuff),
        Some("tuff.toml" | ".tuff.toml") => Some(toml::from_str(&contents)?),
        _ => None,
    };
    Ok(options)
}

pub fn from_path(path: Option<&Path>) -> Result<Options> {
    let Some(path) = path else {
        return Ok(Options::default());
    };
    let mut options = read_options(path)?.unwrap_or_default();
    options.lint.source = Some(path.to_path_buf());
    if let Some(directory) = path.parent() {
        options.lint.resolve_paths(directory);
    }
    Ok(options)
}

pub fn standard_options(path: &Path) -> Result<Table> {
    let Some(options) = read_options(path)? else {
        return Ok(Table::new());
    };
    let mut table = Table::new();
    if let Some(value) = options.extend {
        table.insert("extend".to_owned(), value.into());
    }
    if let Some(value) = options.line_length {
        table.insert("line-length".to_owned(), toml::Value::Integer(value.into()));
    }
    if let Some(value) = options.target_version {
        table.insert("target-version".to_owned(), value.into());
    }
    if let Some(value) = options.include {
        table.insert(
            "include".to_owned(),
            toml::Value::Array(value.into_iter().map(Into::into).collect()),
        );
    }
    if let Some(value) = options.exclude {
        table.insert(
            "exclude".to_owned(),
            toml::Value::Array(value.into_iter().map(Into::into).collect()),
        );
    }
    if let Some(format) = options.format {
        let format: Table = format
            .into_iter()
            .filter(|(name, _)| {
                matches!(
                    name.as_str(),
                    "quote-style" | "indent-style" | "line-ending"
                )
            })
            .collect();
        if !format.is_empty() {
            table.insert("format".to_owned(), toml::Value::Table(format));
        }
    }
    Ok(table)
}

fn is_legacy(definition: &RuleDefinition) -> bool {
    builtin::legacy::RULES
        .iter()
        .any(|legacy| std::ptr::eq(legacy, definition))
}

fn expand_selectors(selectors: &[String]) -> Vec<&'static str> {
    let definitions = builtin::definitions();
    let mut codes = Vec::new();
    for selector in selectors {
        codes.extend(
            definitions
                .iter()
                .filter(|definition| match selector.as_str() {
                    "all" => true,
                    "builtin" => !is_legacy(definition),
                    "legacy" => is_legacy(definition),
                    name => definition.name == name,
                })
                .map(|definition| definition.code),
        );
    }
    codes.sort_unstable();
    codes.dedup();
    codes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint(enable: &[&str], disable: &[&str]) -> LintOptions {
        LintOptions {
            enable: enable.iter().map(ToString::to_string).collect(),
            disable: disable.iter().map(ToString::to_string).collect(),
            ..LintOptions::default()
        }
    }

    fn enabled(options: &LintOptions, name: &str) -> bool {
        options
            .python_rule_selection(|name| name.starts_with("python-"))
            .unwrap()
            .is_enabled(name)
    }

    #[test]
    fn python_rules_are_enabled_unless_disabled() {
        let options = lint(&[], &["python-off"]);
        assert!(enabled(&options, "python-on"));
        assert!(!enabled(&options, "python-off"));
        let options = lint(&["python-kept"], &["all"]);
        assert!(!enabled(&options, "python-kept"));
        assert!(!enabled(&options, "python-other"));
    }

    #[test]
    fn child_selections_override_inherited_ones() {
        let options = lint(&["python-rule"], &[]).combine(lint(&[], &["all"]));
        assert!(enabled(&options, "python-rule"));
        assert!(!enabled(&options, "python-other"));
    }

    #[test]
    fn unknown_names_fail_but_native_names_and_groups_pass() {
        let native = builtin::definitions()[0].name;
        let options = lint(&[native, "builtin"], &["legacy", "python-rule"]);
        assert!(
            options
                .python_rule_selection(|name| name == "python-rule")
                .is_ok()
        );
        let error = lint(&["python-missing"], &[])
            .python_rule_selection(|name| name == "python-rule")
            .unwrap_err();
        assert_eq!(error.to_string(), "unknown Tuff rule `python-missing`");
    }

    #[test]
    fn plugin_paths_resolve_against_the_config_directory() {
        let mut options = LintOptions {
            plugin_dirs: Some(vec![PathBuf::from("tooling/rules")]),
            python: Some(PathBuf::from(".venv/bin/python")),
            ..LintOptions::default()
        };
        options.resolve_paths(Path::new("/project"));
        let config = options.plugin_config().unwrap();
        assert_eq!(
            config.plugin_dirs,
            [PathBuf::from("/project/tooling/rules")]
        );
        assert_eq!(
            config.python,
            Some(PathBuf::from("/project/.venv/bin/python"))
        );

        let mut options = LintOptions {
            python: Some(PathBuf::from("python3.12")),
            ..LintOptions::default()
        };
        options.resolve_paths(Path::new("/project"));
        assert_eq!(options.python, Some(PathBuf::from("python3.12")));
        assert_eq!(options.plugin_config(), None);
    }

    #[test]
    fn options_accept_throws_section() {
        let parsed: Pyproject = toml::from_str(
            r#"
            [tool.tuff.lint]
            enable = ["forbidden-call"]

            [tool.tuff.throws]
            boundaries = ["app.api.*"]
            "#,
        )
        .unwrap();
        let tuff = parsed.tool.and_then(|tool| tool.tuff).unwrap();
        assert_eq!(tuff.lint.enable, ["forbidden-call"]);
        assert!(tuff.throws.is_some());
    }
}
