use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Table;

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
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Selection {
    enable: Vec<String>,
    disable: Vec<String>,
}

impl LintOptions {
    pub fn combine(mut self, parent: Self) -> Self {
        self.source = self.source.or(parent.source);
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

    pub fn selected_codes(&self) -> Result<Vec<&'static str>> {
        Ok(self
            .selections()?
            .into_iter()
            .filter_map(|(code, enabled)| enabled.then_some(code))
            .collect())
    }

    pub fn ignored_codes(&self) -> Result<Vec<&'static str>> {
        Ok(self
            .selections()?
            .into_iter()
            .filter_map(|(code, enabled)| (!enabled).then_some(code))
            .collect())
    }

    fn selections(&self) -> Result<BTreeMap<&'static str, bool>> {
        let mut result = BTreeMap::new();
        for (enable, disable) in self
            .inherited
            .iter()
            .map(|selection| (&selection.enable, &selection.disable))
            .chain(std::iter::once((&self.enable, &self.disable)))
        {
            result.extend(
                expand_selectors(enable)?
                    .into_iter()
                    .map(|code| (code, true)),
            );
            result.extend(
                expand_selectors(disable)?
                    .into_iter()
                    .map(|code| (code, false)),
            );
        }
        Ok(result)
    }

    pub fn spacing_policy(
        &self,
        formatter_rules: &[SpacingRule],
        enabled: impl Fn(&str) -> bool,
    ) -> Result<SpacingPolicy> {
        let ignored = self.ignored_codes()?;
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
}

impl Options {
    pub fn combine(mut self, parent: Self) -> Self {
        self.lint = self.lint.combine(parent.lint);
        self.format = match (self.format, parent.format) {
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

fn expand_selectors(selectors: &[String]) -> Result<Vec<&'static str>> {
    let definitions = builtin::definitions();
    let mut codes = Vec::new();
    for selector in selectors {
        let selected: Vec<_> = definitions
            .iter()
            .copied()
            .filter(|definition| match selector.as_str() {
                "all" => true,
                "builtin" => !is_legacy(definition),
                "legacy" => is_legacy(definition),
                name => definition.name == name,
            })
            .collect();
        if selected.is_empty() {
            bail!("unknown Tuff rule `{selector}`");
        }
        for definition in selected {
            codes.push(definition.code);
        }
    }
    codes.sort_unstable();
    codes.dedup();
    Ok(codes)
}
