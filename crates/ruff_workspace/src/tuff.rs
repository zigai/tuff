use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use toml::Value;
use toml::map::Map;
use tuff_python_formatter::{
    AlignmentMode, AssignmentAlignmentScope, ClassFieldAlignmentScope, CollectionLayout,
    DictAlignmentMode, FeatureMode, FunctionParamAlignmentScope, OneLineSuiteClause,
    OneLineSuiteStatement, TuffCustomOptions,
};

pub(crate) fn custom_options(
    options: &tuff_lint::config::Options,
    enabled: impl Fn(&str) -> bool,
) -> Result<TuffCustomOptions> {
    let mut custom = TuffCustomOptions::default();
    let path = options
        .lint
        .source
        .as_deref()
        .unwrap_or_else(|| Path::new("config"));
    if let Some(table) = &options.format {
        apply_format_table(table, path, &mut custom)?;
    }
    custom.blank_lines = options
        .lint
        .spacing_policy(&custom.blank_lines.rules, enabled)?;
    Ok(custom)
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
            "arguments" => apply_arguments_table(as_table(value, key, path)?, custom)?,
            "imports" => apply_imports_table(as_table(value, key, path)?, custom)?,
            "blank-lines" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct BlankLines {
                    rules: Vec<tuff_lint::builtin::spacing::SpacingRule>,
                }
                custom.blank_lines.rules = value.clone().try_into::<BlankLines>()?.rules;
            }
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
            "dict-alignment" => custom.alignment.dict_alignment = dict_alignment_mode(value, key)?,
            "dict-values" => {
                custom.alignment.dict_values = alignment_mode(value, key)?;
                if matches!(custom.alignment.dict_values, AlignmentMode::Enabled)
                    && matches!(custom.alignment.dict_alignment, DictAlignmentMode::None)
                {
                    custom.alignment.dict_alignment = DictAlignmentMode::Value;
                }
            }
            "call-keyword-args" => custom.alignment.call_keyword_args = alignment_mode(value, key)?,
            "import-aliases" => custom.alignment.import_aliases = alignment_mode(value, key)?,
            "collection-rows" => custom.alignment.collection_rows = alignment_mode(value, key)?,
            "repeated-call-args" => {
                custom.alignment.repeated_call_args = alignment_mode(value, key)?;
            }
            "with-items" => custom.alignment.with_items = alignment_mode(value, key)?,
            "trailing-comments" => {
                custom.alignment.trailing_comments = alignment_mode(value, key)?;
            }
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
            "docstrings" => custom.one_line_suites.docstrings = feature_mode(value, key)?,
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

fn apply_arguments_table(table: &Map<String, Value>, custom: &mut TuffCustomOptions) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "calls" => custom.arguments.calls = feature_mode(value, key)?,
            "parameters" => custom.arguments.parameters = feature_mode(value, key)?,
            "mixed" => custom.arguments.mixed = feature_mode(value, key)?,
            "ignore-magic-trailing-comma" => {
                custom.arguments.ignore_magic_trailing_comma = bool_value(value, key)?;
            }
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format.arguments]"),
        }
    }

    Ok(())
}

fn apply_imports_table(table: &Map<String, Value>, custom: &mut TuffCustomOptions) -> Result<()> {
    for (key, value) in table {
        match key.as_str() {
            "mode" => custom.imports.mode = feature_mode(value, key)?,
            "ignore-magic-trailing-comma" => {
                custom.imports.ignore_magic_trailing_comma = bool_value(value, key)?;
            }
            unknown => bail!("unknown field `{unknown}` in [tool.tuff.format.imports]"),
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
                "ignore-magic-trailing-comma" => {
                    policy.ignore_magic_trailing_comma = bool_value(value, policy_key)?;
                }
                "compact-nested" => policy.compact_nested = bool_value(value, policy_key)?,
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

fn dict_alignment_mode(value: &Value, key: &str) -> Result<DictAlignmentMode> {
    match value
        .as_str()
        .ok_or_else(|| anyhow!("expected string for `{key}`"))?
    {
        "none" => Ok(DictAlignmentMode::None),
        "value" => Ok(DictAlignmentMode::Value),
        "colon" => Ok(DictAlignmentMode::Colon),
        value => bail!("unsupported dict alignment mode `{value}`"),
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
        "function" => Ok(OneLineSuiteClause::Function),
        "class" => Ok(OneLineSuiteClause::Class),
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
        "fill" => Ok(CollectionLayout::Fill),
        "force-expanded" => Ok(CollectionLayout::ForceExpanded),
        "expand-if-more-than" => Ok(CollectionLayout::ExpandIfMoreThan { threshold: 0 }),
        "preserve-input" => Ok(CollectionLayout::PreserveInput),
        value => bail!("unsupported collection layout `{value}`"),
    }
}
