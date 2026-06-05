use ruff_macros::CacheKey;

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct TuffCustomOptions {
    pub alignment: AlignmentOptions,
    pub collections: CollectionOptions,
    pub blank_lines: BlankLineOptions,
    pub spacing: SpacingOptions,
    pub commas: CommaOptions,
    pub one_line_suites: OneLineSuiteOptions,
    pub docstrings: DocstringOptions,
}

impl TuffCustomOptions {
    pub const fn is_default_disabled(&self) -> bool {
        self.alignment.is_disabled()
            && self.collections.is_ruff_default()
            && self.blank_lines.is_disabled()
            && self.spacing.is_disabled()
            && self.commas.is_disabled()
            && self.one_line_suites.is_disabled()
            && self.docstrings.is_disabled()
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum FeatureMode {
    #[default]
    Disabled,
    Enabled,
}

impl FeatureMode {
    pub const fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum AlignmentMode {
    #[default]
    Disabled,
    Enabled,
    PreserveInput,
}

impl AlignmentMode {
    pub const fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled | Self::PreserveInput)
    }
}

#[derive(CacheKey, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields, rename_all = "kebab-case")
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "formatter configuration mirrors boolean TOML settings"
)]
pub struct AlignmentOptions {
    pub class_fields: AlignmentMode,
    pub class_field_scope: ClassFieldAlignmentScope,
    pub function_params: AlignmentMode,
    pub function_param_scope: FunctionParamAlignmentScope,
    pub align_defaults: bool,
    pub assignments: AlignmentMode,
    pub assignment_scope: AssignmentAlignmentScope,
    pub dict_values: AlignmentMode,
    pub call_keyword_args: AlignmentMode,
    pub import_aliases: AlignmentMode,
    pub min_group_size: u16,
    pub break_on_blank_line: bool,
    pub break_on_leading_comment: bool,
    pub break_on_trailing_comment: bool,
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum ClassFieldAlignmentScope {
    #[default]
    All,
    DataclassesAndPydantic,
    DataclassesPydanticAndTypedDict,
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum FunctionParamAlignmentScope {
    #[default]
    All,
    Functions,
    Methods,
    ClassInit,
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum AssignmentAlignmentScope {
    #[default]
    All,
    Module,
    Class,
    ModuleAndClass,
    EnumClass,
}

#[derive(CacheKey, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields, rename_all = "kebab-case")
)]
pub struct OneLineSuiteOptions {
    pub mode: FeatureMode,
    pub clauses: Vec<OneLineSuiteClause>,
    pub statements: Vec<OneLineSuiteStatement>,
}

impl OneLineSuiteOptions {
    pub const fn is_disabled(&self) -> bool {
        self.mode.is_disabled()
    }
}

impl Default for OneLineSuiteOptions {
    fn default() -> Self {
        Self {
            mode: FeatureMode::Disabled,
            clauses: vec![OneLineSuiteClause::If],
            statements: vec![OneLineSuiteStatement::Return, OneLineSuiteStatement::Call],
        }
    }
}

#[derive(CacheKey, Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum OneLineSuiteClause {
    If,
    Elif,
    Else,
    For,
    While,
    With,
    Try,
    Except,
    Finally,
    MatchCase,
}

#[derive(CacheKey, Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum OneLineSuiteStatement {
    Return,
    Call,
    Raise,
    Assert,
    Pass,
    Break,
    Continue,
    Ellipsis,
    Expression,
    Assignment,
    AnnotatedAssignment,
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields, rename_all = "kebab-case")
)]
pub struct DocstringOptions {
    pub args_sections: FeatureMode,
}

impl DocstringOptions {
    pub const fn is_disabled(&self) -> bool {
        self.args_sections.is_disabled()
    }
}

impl AlignmentOptions {
    pub const fn is_disabled(&self) -> bool {
        self.class_fields.is_disabled()
            && self.function_params.is_disabled()
            && self.assignments.is_disabled()
            && self.dict_values.is_disabled()
            && self.call_keyword_args.is_disabled()
            && self.import_aliases.is_disabled()
    }
}

impl Default for AlignmentOptions {
    fn default() -> Self {
        Self {
            class_fields: AlignmentMode::Disabled,
            class_field_scope: ClassFieldAlignmentScope::All,
            function_params: AlignmentMode::Disabled,
            function_param_scope: FunctionParamAlignmentScope::All,
            align_defaults: true,
            assignments: AlignmentMode::Disabled,
            assignment_scope: AssignmentAlignmentScope::All,
            dict_values: AlignmentMode::Disabled,
            call_keyword_args: AlignmentMode::Disabled,
            import_aliases: AlignmentMode::Disabled,
            min_group_size: 2,
            break_on_blank_line: true,
            break_on_leading_comment: true,
            break_on_trailing_comment: false,
        }
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum CollectionLayout {
    #[default]
    RuffDefault,
    PreferCompact,
    ForceExpanded,
    ExpandIfMoreThan {
        threshold: u16,
    },
    PreserveInput,
}

#[derive(CacheKey, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields, rename_all = "kebab-case")
)]
pub struct CollectionPolicy {
    pub layout: CollectionLayout,
}

impl CollectionPolicy {
    pub const fn is_ruff_default(&self) -> bool {
        matches!(self.layout, CollectionLayout::RuffDefault)
    }
}

impl Default for CollectionPolicy {
    fn default() -> Self {
        Self {
            layout: CollectionLayout::RuffDefault,
        }
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct CollectionOptions {
    pub lists: CollectionPolicy,
    pub dicts: CollectionPolicy,
    pub tuples: CollectionPolicy,
    pub sets: CollectionPolicy,
}

impl CollectionOptions {
    pub const fn is_ruff_default(&self) -> bool {
        self.lists.is_ruff_default()
            && self.dicts.is_ruff_default()
            && self.tuples.is_ruff_default()
            && self.sets.is_ruff_default()
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct BlankLineOptions {
    pub mode: FeatureMode,
}

impl BlankLineOptions {
    pub const fn is_disabled(&self) -> bool {
        self.mode.is_disabled()
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct SpacingOptions {
    pub mode: FeatureMode,
}

impl SpacingOptions {
    pub const fn is_disabled(&self) -> bool {
        self.mode.is_disabled()
    }
}

#[derive(CacheKey, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct CommaOptions {
    pub mode: FeatureMode,
}

impl CommaOptions {
    pub const fn is_disabled(&self) -> bool {
        self.mode.is_disabled()
    }
}
