use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use toml::map::Map;
use toml::{Table, Value};
use tuff_python_formatter::{
    AlignmentMode, AssignmentAlignmentScope, ClassFieldAlignmentScope, CollectionLayout,
    FeatureMode, FunctionParamAlignmentScope, OneLineSuiteClause, OneLineSuiteStatement,
    TuffCustomOptions,
};

pub(crate) fn custom_options(path: Option<&Path>) -> Result<TuffCustomOptions> {
    let Some(config_path) = discover_config(path) else {
        return Ok(TuffCustomOptions::default());
    };

    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let value = contents
        .parse::<Table>()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    let mut custom = TuffCustomOptions::default();
    if let Some(table) = tuff_table(&value, &config_path) {
        apply_root_table(table, &config_path, &mut custom)?;
    }

    Ok(custom)
}

fn discover_config(path: Option<&Path>) -> Option<PathBuf> {
    let start = path.unwrap_or_else(|| Path::new("."));
    let start = if start.is_dir() {
        start
    } else {
        start.parent().unwrap_or_else(|| Path::new("."))
    };

    for directory in start.ancestors() {
        for candidate in ["tuff.toml", ".tuff.toml"] {
            let config = directory.join(candidate);
            if config.exists() {
                return Some(config);
            }
        }
        let pyproject = directory.join("pyproject.toml");
        if fs::read_to_string(&pyproject).is_ok_and(|contents| contents.contains("[tool.tuff")) {
            return Some(pyproject);
        }
    }

    None
}

fn tuff_table<'a>(table: &'a Table, path: &Path) -> Option<&'a Map<String, Value>> {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("tuff.toml" | ".tuff.toml") => Some(table),
        Some("pyproject.toml") => table
            .get("tool")
            .and_then(Value::as_table)
            .and_then(|tool| tool.get("tuff"))
            .and_then(Value::as_table),
        _ => None,
    }
}

fn apply_root_table(
    table: &Map<String, Value>,
    path: &Path,
    custom: &mut TuffCustomOptions,
) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "line-length" | "target-version" | "include" | "exclude" => {}
            "format" => apply_format_table(as_table(value, key, path)?, path, custom)?,
            unknown => bail!("unknown field `{unknown}` in [tool.tuff]"),
        }
    }

    Ok(())
}

fn apply_format_table(
    table: &Map<String, Value>,
    path: &Path,
    custom: &mut TuffCustomOptions,
) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "quote-style" | "indent-style" | "line-ending" => {}
            "alignment" => apply_alignment_table(as_table(value, key, path)?, custom)?,
            "one-line-suites" => {
                apply_one_line_suites_table(as_table(value, key, path)?, path, custom)?;
            }
            "docstrings" => apply_docstrings_table(as_table(value, key, path)?, custom)?,
            "collections" => apply_collections_table(as_table(value, key, path)?, custom)?,
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format]"),
        }
    }

    Ok(())
}

fn apply_alignment_table(table: &Map<String, Value>, custom: &mut TuffCustomOptions) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "class-fields" => custom.alignment.class_fields = alignment_mode(value, key)?,
            "class-field-scope" => {
                custom.alignment.class_field_scope = class_field_alignment_scope(value, key)?;
            }
            "function-params" => custom.alignment.function_params = alignment_mode(value, key)?,
            "function-param-scope" => {
                custom.alignment.function_param_scope = function_param_alignment_scope(value, key)?;
            }
            "align-defaults" => custom.alignment.align_defaults = bool_value(value, key)?,
            "assignments" => custom.alignment.assignments = alignment_mode(value, key)?,
            "assignment-scope" => {
                custom.alignment.assignment_scope = assignment_alignment_scope(value, key)?;
            }
            "dict-values" => custom.alignment.dict_values = alignment_mode(value, key)?,
            "call-keyword-args" => custom.alignment.call_keyword_args = alignment_mode(value, key)?,
            "import-aliases" => custom.alignment.import_aliases = alignment_mode(value, key)?,
            "min-group-size" => custom.alignment.min_group_size = value_as_u16(value, key)?,
            "break-on-blank-line" => {
                custom.alignment.break_on_blank_line = bool_value(value, key)?;
            }
            "break-on-leading-comment" => {
                custom.alignment.break_on_leading_comment = bool_value(value, key)?;
            }
            "break-on-trailing-comment" => {
                custom.alignment.break_on_trailing_comment = bool_value(value, key)?;
            }
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format.alignment]"),
        }
    }

    Ok(())
}

fn apply_one_line_suites_table(
    table: &Map<String, Value>,
    path: &Path,
    custom: &mut TuffCustomOptions,
) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "mode" => custom.one_line_suites.mode = feature_mode(value, key)?,
            "clauses" => {
                custom.one_line_suites.clauses = string_array(value, key, path)?
                    .into_iter()
                    .map(|value| one_line_suite_clause(&value))
                    .collect::<Result<_>>()?;
            }
            "statements" => {
                custom.one_line_suites.statements = string_array(value, key, path)?
                    .into_iter()
                    .map(|value| one_line_suite_statement(&value))
                    .collect::<Result<_>>()?;
            }
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format.one-line-suites]"),
        }
    }

    Ok(())
}

fn apply_docstrings_table(
    table: &Map<String, Value>,
    custom: &mut TuffCustomOptions,
) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "args-sections" => custom.docstrings.args_sections = feature_mode(value, key)?,
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format.docstrings]"),
        }
    }

    Ok(())
}

fn apply_collections_table(
    table: &Map<String, Value>,
    custom: &mut TuffCustomOptions,
) -> Result<()> {
    for (key, value) in table {
        let policy = match key.as_str() {
            "lists" => &mut custom.collections.lists,
            "dicts" => &mut custom.collections.dicts,
            "tuples" => &mut custom.collections.tuples,
            "sets" => &mut custom.collections.sets,
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format.collections]"),
        };
        let table = as_table(value, key, Path::new("config"))?;
        let mut threshold = None;
        for (policy_key, value) in table {
            match policy_key.as_str() {
                "layout" => {
                    policy.layout =
                        collection_layout(string(value, policy_key, Path::new("config"))?)?;
                }
                "threshold" => threshold = Some(value_as_u16(value, policy_key)?),
                unknown => {
                    bail!("unknown field `{unknown}` in [tool.tuff.format.collections.{key}]")
                }
            }
        }
        if let CollectionLayout::ExpandIfMoreThan { threshold: current } = policy.layout {
            policy.layout = CollectionLayout::ExpandIfMoreThan {
                threshold: threshold.unwrap_or(current),
            };
        } else if let Some(threshold) = threshold {
            policy.layout = CollectionLayout::ExpandIfMoreThan { threshold };
        }
    }

    Ok(())
}

fn as_table<'a>(value: &'a Value, key: &str, path: &Path) -> Result<&'a Map<String, Value>> {
    value
        .as_table()
        .ok_or_else(|| anyhow!("expected table for `{key}` in {}", path.display()))
}

fn string<'a>(value: &'a Value, key: &str, path: &Path) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}` in {}", path.display()))
}

fn string_array(value: &Value, key: &str, path: &Path) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or_else(|| anyhow!("expected array for `{key}` in {}", path.display()))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .ok_or_else(|| anyhow!("expected string entries for `{key}` in {}", path.display()))
        })
        .collect()
}

fn bool_value(value: &Value, key: &str) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| anyhow!("expected boolean for `{key}`"))
}

fn value_as_u16(value: &Value, key: &str) -> Result<u16> {
    let integer = value
        .as_integer()
        .ok_or_else(|| anyhow!("expected integer for `{key}`"))?;
    u16::try_from(integer).with_context(|| format!("invalid value for `{key}`"))
}

fn feature_mode(value: &Value, key: &str) -> Result<FeatureMode> {
    match value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}`"))?
    {
        "disabled" => Ok(FeatureMode::Disabled),
        "enabled" => Ok(FeatureMode::Enabled),
        value => bail!("unsupported feature mode `{value}`"),
    }
}

fn alignment_mode(value: &Value, key: &str) -> Result<AlignmentMode> {
    match value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}`"))?
    {
        "disabled" => Ok(AlignmentMode::Disabled),
        "enabled" => Ok(AlignmentMode::Enabled),
        "preserve-input" => Ok(AlignmentMode::PreserveInput),
        value => bail!("unsupported alignment mode `{value}`"),
    }
}

fn class_field_alignment_scope(value: &Value, key: &str) -> Result<ClassFieldAlignmentScope> {
    match value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}`"))?
    {
        "all" => Ok(ClassFieldAlignmentScope::All),
        "dataclasses-and-pydantic" => Ok(ClassFieldAlignmentScope::DataclassesAndPydantic),
        "dataclasses-pydantic-and-typeddict" | "schemas" => {
            Ok(ClassFieldAlignmentScope::DataclassesPydanticAndTypedDict)
        }
        value => bail!("unsupported class field alignment scope `{value}`"),
    }
}

fn function_param_alignment_scope(value: &Value, key: &str) -> Result<FunctionParamAlignmentScope> {
    match value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}`"))?
    {
        "all" => Ok(FunctionParamAlignmentScope::All),
        "functions" => Ok(FunctionParamAlignmentScope::Functions),
        "methods" => Ok(FunctionParamAlignmentScope::Methods),
        "class-init" => Ok(FunctionParamAlignmentScope::ClassInit),
        value => bail!("unsupported function parameter alignment scope `{value}`"),
    }
}

fn assignment_alignment_scope(value: &Value, key: &str) -> Result<AssignmentAlignmentScope> {
    match value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}`"))?
    {
        "all" => Ok(AssignmentAlignmentScope::All),
        "module" => Ok(AssignmentAlignmentScope::Module),
        "class" => Ok(AssignmentAlignmentScope::Class),
        "module-and-class" => Ok(AssignmentAlignmentScope::ModuleAndClass),
        "enum-class" => Ok(AssignmentAlignmentScope::EnumClass),
        value => bail!("unsupported assignment alignment scope `{value}`"),
    }
}

fn one_line_suite_clause(value: &str) -> Result<OneLineSuiteClause> {
    match value {
        "if" => Ok(OneLineSuiteClause::If),
        "elif" => Ok(OneLineSuiteClause::Elif),
        "else" => Ok(OneLineSuiteClause::Else),
        "for" => Ok(OneLineSuiteClause::For),
        "while" => Ok(OneLineSuiteClause::While),
        "with" => Ok(OneLineSuiteClause::With),
        "try" => Ok(OneLineSuiteClause::Try),
        "except" => Ok(OneLineSuiteClause::Except),
        "finally" => Ok(OneLineSuiteClause::Finally),
        "match-case" => Ok(OneLineSuiteClause::MatchCase),
        value => bail!("unsupported one-line suite clause `{value}`"),
    }
}

fn one_line_suite_statement(value: &str) -> Result<OneLineSuiteStatement> {
    match value {
        "return" => Ok(OneLineSuiteStatement::Return),
        "call" => Ok(OneLineSuiteStatement::Call),
        "raise" => Ok(OneLineSuiteStatement::Raise),
        "assert" => Ok(OneLineSuiteStatement::Assert),
        "pass" => Ok(OneLineSuiteStatement::Pass),
        "break" => Ok(OneLineSuiteStatement::Break),
        "continue" => Ok(OneLineSuiteStatement::Continue),
        "ellipsis" => Ok(OneLineSuiteStatement::Ellipsis),
        "expression" => Ok(OneLineSuiteStatement::Expression),
        "assignment" => Ok(OneLineSuiteStatement::Assignment),
        "annotated-assignment" => Ok(OneLineSuiteStatement::AnnotatedAssignment),
        value => bail!("unsupported one-line suite statement `{value}`"),
    }
}

fn collection_layout(value: &str) -> Result<CollectionLayout> {
    match value {
        "ruff-default" => Ok(CollectionLayout::RuffDefault),
        "prefer-compact" => Ok(CollectionLayout::PreferCompact),
        "force-expanded" => Ok(CollectionLayout::ForceExpanded),
        "expand-if-more-than" => Ok(CollectionLayout::ExpandIfMoreThan { threshold: 0 }),
        "preserve-input" => Ok(CollectionLayout::PreserveInput),
        value => bail!("unsupported collection layout `{value}`"),
    }
}
