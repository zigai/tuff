use ruff_formatter::LineWidth;
use ruff_text_size::{TextRange, TextSize};
use tuff_python_formatter::{
    AlignmentMode, AssignmentAlignmentScope, ClassFieldAlignmentScope, CollectionLayout,
    DictAlignmentMode, FunctionParamAlignmentScope, PyFormatOptions, TuffCustomOptions,
    format_module_source, format_range,
};

fn class_field_alignment_options() -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    PyFormatOptions::default().with_custom(custom)
}

fn function_param_alignment_options() -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.alignment.function_params = AlignmentMode::Enabled;
    PyFormatOptions::default().with_custom(custom)
}

fn function_param_alignment_options_with_scope(
    scope: FunctionParamAlignmentScope,
) -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.alignment.function_params = AlignmentMode::Enabled;
    custom.alignment.function_param_scope = scope;
    PyFormatOptions::default().with_custom(custom)
}

fn assignment_alignment_options_with_scope(scope: AssignmentAlignmentScope) -> PyFormatOptions {
    let mut custom = TuffCustomOptions::default();
    custom.alignment.assignments = AlignmentMode::Enabled;
    custom.alignment.assignment_scope = scope;
    PyFormatOptions::default().with_custom(custom)
}

fn format(source: &str, options: PyFormatOptions) -> String {
    format_module_source(source, options).unwrap().into_code()
}

fn format_class_fields(source: &str) -> String {
    format(source, class_field_alignment_options())
}

fn format_function_params(source: &str) -> String {
    format(source, function_param_alignment_options())
}

fn assert_idempotent(source: &str, options: PyFormatOptions) {
    let formatted = format(source, options.clone());
    assert_eq!(format(&formatted, options), formatted);
}

fn format_both(source: &str) -> String {
    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.function_params = AlignmentMode::Enabled;
    format_module_source(source, PyFormatOptions::default().with_custom(custom))
        .unwrap()
        .into_code()
}

#[test]
fn aligns_class_fields() {
    let source = r#"class User:
    id: int
    display_name: str
    active: bool = True
"#;

    let expected = r#"class User:
    id:           int
    display_name: str
    active:       bool = True
"#;

    let formatted = format_class_fields(source);

    assert_eq!(formatted, expected);
    assert_eq!(format_class_fields(&formatted), formatted);
}

#[test]
fn class_field_alignment_uses_display_width() {
    let source = r#"class User:
    x: str
    動画: str
"#;

    let expected = r#"class User:
    x:    str
    動画: str
"#;

    let formatted = format_class_fields(source);

    assert_eq!(formatted, expected);
    assert_eq!(format_class_fields(&formatted), formatted);
}

#[test]
fn blank_lines_and_leading_comments_split_groups() {
    let source = r#"class User:
    id: int
    display_name: str

    # account state
    active: bool
    last_seen_at: datetime | None
"#;

    let expected = r#"class User:
    id:           int
    display_name: str

    # account state
    active:       bool
    last_seen_at: datetime | None
"#;

    assert_eq!(format_class_fields(source), expected);
}

#[test]
fn fmt_off_regions_are_not_aligned() {
    let source = r#"class User:
    id: int
    display_name: str
    # fmt: off
    x: int
    very_long_name: str
    # fmt: on
    active: bool
    last_seen_at: datetime | None
"#;

    let expected = r#"class User:
    id:           int
    display_name: str
    # fmt: off
    x: int
    very_long_name: str
    # fmt: on
    active:       bool
    last_seen_at: datetime | None
"#;

    assert_eq!(format_class_fields(source), expected);
}

#[test]
fn aligns_class_assignments_for_enums() {
    let source = r#"class ServiceErrorCode(StrEnum):
    """Stable error codes returned by the ytup service API."""

    UNAUTHORIZED = "unauthorized"
    VALIDATION_FAILED = "validation_failed"
    REQUEST_VALIDATION_FAILED = "request_validation_failed"
    INPUT_NOT_FOUND = "input_not_found"
    CONFLICT = "conflict"
    UPSTREAM_UPLOAD_FAILED = "upstream_upload_failed"
    SERVICE_UNAVAILABLE = "service_unavailable"
    NOT_FOUND = "not_found"
    HTTP_ERROR = "http_error"
    INTERNAL_SERVER_ERROR = "internal_server_error"
"#;

    let expected = r#"class ServiceErrorCode(StrEnum):
    """Stable error codes returned by the ytup service API."""

    UNAUTHORIZED              = "unauthorized"
    VALIDATION_FAILED         = "validation_failed"
    REQUEST_VALIDATION_FAILED = "request_validation_failed"
    INPUT_NOT_FOUND           = "input_not_found"
    CONFLICT                  = "conflict"
    UPSTREAM_UPLOAD_FAILED    = "upstream_upload_failed"
    SERVICE_UNAVAILABLE       = "service_unavailable"
    NOT_FOUND                 = "not_found"
    HTTP_ERROR                = "http_error"
    INTERNAL_SERVER_ERROR     = "internal_server_error"
"#;

    assert_eq!(
        format(
            source,
            assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class)
        ),
        expected
    );
}

#[test]
fn assignment_alignment_uses_display_width() {
    let source = r#"class Codes:
    x = "latin"
    動画 = "wide"
"#;

    let expected = r#"class Codes:
    x    = "latin"
    動画 = "wide"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_scope_can_target_enum_classes_only() {
    let source = r#"class Plain:
    SHORT = "short"
    MUCH_LONGER = "long"

class Codes(StrEnum):
    SHORT = "short"
    MUCH_LONGER = "long"
"#;

    let expected = r#"class Plain:
    SHORT = "short"
    MUCH_LONGER = "long"


class Codes(StrEnum):
    SHORT       = "short"
    MUCH_LONGER = "long"
"#;

    assert_eq!(
        format(
            source,
            assignment_alignment_options_with_scope(AssignmentAlignmentScope::EnumClass)
        ),
        expected
    );
}

#[test]
fn assignment_alignment_scope_does_not_align_function_locals_when_class_only() {
    let source = r#"MODULE_SHORT = "short"
MODULE_MUCH_LONGER = "long"

class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"

def f():
    local = "short"
    much_longer_local = "long"
"#;

    let expected = r#"MODULE_SHORT = "short"
MODULE_MUCH_LONGER = "long"


class Codes:
    SHORT       = "short"
    MUCH_LONGER = "long"


def f():
    local = "short"
    much_longer_local = "long"
"#;

    assert_eq!(
        format(
            source,
            assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class)
        ),
        expected
    );
}

#[test]
fn assignment_alignment_scope_does_not_align_method_locals_when_class_only() {
    let source = r#"class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"

    def method(self):
        local = "short"
        much_longer_local = "long"
"#;

    let expected = r#"class Codes:
    SHORT       = "short"
    MUCH_LONGER = "long"

    def method(self):
        local = "short"
        much_longer_local = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_enum_scope_does_not_align_method_locals() {
    let source = r#"class Codes(StrEnum):
    SHORT = "short"
    MUCH_LONGER = "long"

    def method(self):
        local = "short"
        much_longer_local = "long"
"#;

    let expected = r#"class Codes(StrEnum):
    SHORT       = "short"
    MUCH_LONGER = "long"

    def method(self):
        local = "short"
        much_longer_local = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::EnumClass);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_scope_can_target_module_only() {
    let source = r#"MODULE_SHORT = "short"
MODULE_MUCH_LONGER = "long"

class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"
"#;

    let expected = r#"MODULE_SHORT       = "short"
MODULE_MUCH_LONGER = "long"


class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::Module);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_scope_can_target_module_and_class() {
    let source = r#"MODULE_SHORT = "short"
MODULE_MUCH_LONGER = "long"

class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"

def f():
    local = "short"
    much_longer_local = "long"
"#;

    let expected = r#"MODULE_SHORT       = "short"
MODULE_MUCH_LONGER = "long"


class Codes:
    SHORT       = "short"
    MUCH_LONGER = "long"


def f():
    local = "short"
    much_longer_local = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::ModuleAndClass);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_scope_all_includes_function_locals() {
    let source = r#"def f():
    local = "short"
    much_longer_local = "long"
    if enabled:
        nested = "nested"
        much_longer_nested = "long"
"#;

    let expected = r#"def f():
    local             = "short"
    much_longer_local = "long"
    if enabled:
        nested             = "nested"
        much_longer_nested = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::All);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_min_group_size_is_respected() {
    let source = r#"class Codes:
    A = "a"
    BBB = "bbb"
    CCCCC = "ccccc"
"#;

    let expected = r#"class Codes:
    A = "a"
    BBB = "bbb"
    CCCCC = "ccccc"
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.assignments = AlignmentMode::Enabled;
    custom.alignment.assignment_scope = AssignmentAlignmentScope::Class;
    custom.alignment.min_group_size = 4;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_enum_scope_recognizes_attribute_bases() {
    let source = r#"class Codes(enum.StrEnum):
    SHORT = "short"
    MUCH_LONGER = "long"
"#;

    let expected = r#"class Codes(enum.StrEnum):
    SHORT       = "short"
    MUCH_LONGER = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::EnumClass);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_enum_scope_recognizes_classic_enum_mixins() {
    let source = r#"class Codes(str, Enum):
    SHORT = "short"
    MUCH_LONGER = "long"
"#;

    let expected = r#"class Codes(str, Enum):
    SHORT       = "short"
    MUCH_LONGER = "long"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::EnumClass);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_comments_and_blank_lines_split_groups() {
    let source = r#"class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"

    # second group
    A = "a"
    BBB = "bbb"
"#;

    let expected = r#"class Codes:
    SHORT       = "short"
    MUCH_LONGER = "long"

    # second group
    A   = "a"
    BBB = "bbb"
"#;

    assert_eq!(
        format(
            source,
            assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class)
        ),
        expected
    );
}

#[test]
fn assignment_alignment_skips_non_simple_or_multiline_assignments() {
    let source = r#"class Codes:
    SHORT = "short"
    self.attribute = "attribute"
    MUCH_LONGER = (
        "long"
    )
    OTHER = "other"
    EVEN_LONGER_OTHER = "even-longer-other"
"#;

    let expected = r#"class Codes:
    SHORT = "short"
    self.attribute = "attribute"
    MUCH_LONGER       = "long"
    OTHER             = "other"
    EVEN_LONGER_OTHER = "even-longer-other"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_is_stable_with_multiline_values() {
    let source = r#"class Codes:
    SHORT = [
        "short",
    ]
    MUCH_LONGER = [
        "long",
    ]
    OTHER = "other"
"#;

    let expected = r#"class Codes:
    SHORT       = [
        "short",
    ]
    MUCH_LONGER = [
        "long",
    ]
    OTHER       = "other"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_alignment_respects_fmt_off_regions() {
    let source = r#"class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"
    # fmt: off
    X = "x"
    VERY_LONG_NAME = "very-long-name"
    # fmt: on
    A = "a"
    BBB = "bbb"
"#;

    let expected = r#"class Codes:
    SHORT       = "short"
    MUCH_LONGER = "long"
    # fmt: off
    X = "x"
    VERY_LONG_NAME = "very-long-name"
    # fmt: on
    A   = "a"
    BBB = "bbb"
"#;

    let options = assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn assignment_trailing_comments_can_split_groups() {
    let source = r#"class Codes:
    SHORT = "short"  # public
    MUCH_LONGER = "long"
    A = "a"
    BBB = "bbb"
"#;

    let expected = r#"class Codes:
    SHORT = "short"  # public
    MUCH_LONGER = "long"
    A           = "a"
    BBB         = "bbb"
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.assignments = AlignmentMode::Enabled;
    custom.alignment.assignment_scope = AssignmentAlignmentScope::Class;
    custom.alignment.break_on_trailing_comment = true;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn range_formatting_does_not_align_partial_assignment_group() {
    let source = r#"class Codes:
    SHORT = "short"
    MUCH_LONGER = "long"
    OTHER = "other"
"#;
    let start = source.find("MUCH_LONGER").unwrap();
    let end = source[start..].find('\n').unwrap() + start + 1;

    let printed = format_range(
        source,
        TextRange::new(
            TextSize::try_from(start).unwrap(),
            TextSize::try_from(end).unwrap(),
        ),
        assignment_alignment_options_with_scope(AssignmentAlignmentScope::Class),
    )
    .unwrap();

    assert!(printed.as_code().contains("MUCH_LONGER = \"long\""));
    assert!(!printed.as_code().contains("MUCH_LONGER =  \"long\""));
}

#[test]
fn class_field_scope_can_target_dataclasses_and_pydantic() {
    let source = r#"class Plain:
    id: int
    display_name: str

@dataclass
class Data:
    id: int
    display_name: str

class Model(BaseModel):
    id: int
    display_name: str
"#;

    let expected = r#"class Plain:
    id: int
    display_name: str


@dataclass
class Data:
    id:           int
    display_name: str


class Model(BaseModel):
    id:           int
    display_name: str
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.class_field_scope = ClassFieldAlignmentScope::DataclassesAndPydantic;

    assert_eq!(
        format(source, PyFormatOptions::default().with_custom(custom)),
        expected
    );
}

#[test]
fn class_field_scope_recognizes_pydantic_attribute_bases() {
    let source = r#"class Settings(pydantic.BaseSettings):
    token: str
    upload_timeout_s: float
"#;

    let expected = r#"class Settings(pydantic.BaseSettings):
    token:            str
    upload_timeout_s: float
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.class_field_scope = ClassFieldAlignmentScope::DataclassesAndPydantic;

    assert_eq!(
        format(source, PyFormatOptions::default().with_custom(custom)),
        expected
    );
}

#[test]
fn class_field_scope_recognizes_dataclass_attribute_decorators_and_generic_pydantic_bases() {
    let source = r#"@dataclasses.dataclass(frozen=True)
class Data:
    id: int
    display_name: str

class Model(BaseModel[User]):
    id: int
    display_name: str
"#;

    let expected = r#"@dataclasses.dataclass(frozen=True)
class Data:
    id:           int
    display_name: str


class Model(BaseModel[User]):
    id:           int
    display_name: str
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.class_field_scope = ClassFieldAlignmentScope::DataclassesAndPydantic;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_class_field_defaults() {
    let source = r#"class YoutubeApiUploadRequest(BaseModel):
    """Upload payload expected by the YouTube transport layer."""

    model_config = ConfigDict(extra="forbid")

    video_path: str = Field(min_length=1)
    credentials_path: str = Field(min_length=1)
    snippet: dict[str, Any] = Field(default_factory=dict)
    status: dict[str, Any] = Field(default_factory=dict)
    notify_subscribers: bool = False
"#;

    let expected = r#"class YoutubeApiUploadRequest(BaseModel):
    """Upload payload expected by the YouTube transport layer."""

    model_config = ConfigDict(extra="forbid")

    video_path:         str            = Field(min_length=1)
    credentials_path:   str            = Field(min_length=1)
    snippet:            dict[str, Any] = Field(default_factory=dict)
    status:             dict[str, Any] = Field(default_factory=dict)
    notify_subscribers: bool           = False
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.class_field_scope = ClassFieldAlignmentScope::DataclassesAndPydantic;

    assert_eq!(
        format(source, PyFormatOptions::default().with_custom(custom)),
        expected
    );
}

#[test]
fn class_field_default_alignment_can_be_disabled() {
    let source = r#"class Settings:
    video_path: str = Field(min_length=1)
    snippet: dict[str, Any] = Field(default_factory=dict)
"#;

    let expected = r#"class Settings:
    video_path: str = Field(min_length=1)
    snippet:    dict[str, Any] = Field(default_factory=dict)
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.align_defaults = false;

    assert_eq!(
        format(source, PyFormatOptions::default().with_custom(custom)),
        expected
    );
}

#[test]
fn class_field_trailing_comments_can_split_groups() {
    let source = r#"class Settings:
    video_path: str = Field(min_length=1)  # required
    credentials_path: str = Field(min_length=1)
    snippet: dict[str, Any] = Field(default_factory=dict)
    status: dict[str, Any] = Field(default_factory=dict)
"#;

    let expected = r#"class Settings:
    video_path: str = Field(min_length=1)  # required
    credentials_path: str            = Field(min_length=1)
    snippet:          dict[str, Any] = Field(default_factory=dict)
    status:           dict[str, Any] = Field(default_factory=dict)
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.break_on_trailing_comment = true;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn multiline_annotations_are_group_boundaries() {
    let source = r#"class User:
    id: int
    display_name: (
        str
    )
    active: bool
    last_seen_at: datetime | None
"#;

    let expected = r#"class User:
    id: int
    display_name: str
    active:       bool
    last_seen_at: datetime | None
"#;

    assert_eq!(format_class_fields(source), expected);
}

#[test]
fn aligns_function_parameters() {
    let source = r#"def create_user(
    id: int,
    display_name: str,
    active: bool = True,
) -> User:
    ...
"#;

    let expected = r#"def create_user(
    id:           int,
    display_name: str,
    active:       bool = True,
) -> User: ...
"#;

    let formatted = format_function_params(source);

    assert_eq!(formatted, expected);
    assert_idempotent(source, function_param_alignment_options());
}

#[test]
fn aligns_function_parameter_defaults() {
    let source = r#"class Upload:
    def __init__(
        self,
        *,
        resumable: bool = True,
        chunksize: int = -1,
        proxy: str | None = None,
        upload_timeout_s: float | None = 1800.0,
        upstream_http_timeout_s: float | None = 60.0,
    ) -> None:
        ...
"#;

    let expected = r#"class Upload:
    def __init__(
        self,
        *,
        resumable:               bool         = True,
        chunksize:               int          = -1,
        proxy:                   str | None   = None,
        upload_timeout_s:        float | None = 1800.0,
        upstream_http_timeout_s: float | None = 60.0,
    ) -> None: ...
"#;

    assert_eq!(format_function_params(source), expected);
}

#[test]
fn wide_function_parameter_groups_are_not_aligned_past_line_width() {
    let source = r#"def download(
    url: str,
    opts: dict[str, Any] | None = None,
    task: progresslib.TaskContext | progresslib.TaskClient | None = None,
    use_cache: bool = True,
    refresh_cache: bool = False,
) -> dict[str, Any]:
    ...
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.function_params = AlignmentMode::Enabled;
    let options = PyFormatOptions::default()
        .with_line_width(LineWidth::try_from(80).unwrap())
        .with_custom(custom);
    let formatted = format(source, options.clone());

    let expected = r#"def download(
    url: str,
    opts: dict[str, Any] | None = None,
    task: progresslib.TaskContext | progresslib.TaskClient | None = None,
    use_cache: bool = True,
    refresh_cache: bool = False,
) -> dict[str, Any]: ...
"#;

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn function_parameter_default_alignment_can_be_disabled() {
    let source = r#"def f(
    short: int = 1,
    longer_name: str | None = None,
) -> None:
    ...
"#;
    let mut custom = TuffCustomOptions::default();
    custom.alignment.function_params = AlignmentMode::Enabled;
    custom.alignment.align_defaults = false;

    let expected = r#"def f(
    short:       int = 1,
    longer_name: str | None = None,
) -> None: ...
"#;

    assert_eq!(
        format(source, PyFormatOptions::default().with_custom(custom)),
        expected
    );
}

#[test]
fn parameter_separators_split_groups() {
    let source = r#"def f(
    id: int,
    display_name: str,
    /,
    a: int,
    longer_name: str,
    *,
    x: int,
    keyword_only: str,
) -> None:
    ...
"#;

    let expected = r#"def f(
    id:           int,
    display_name: str,
    /,
    a:           int,
    longer_name: str,
    *,
    x:            int,
    keyword_only: str,
) -> None: ...
"#;

    assert_eq!(format_function_params(source), expected);
}

#[test]
fn function_parameter_scope_can_target_class_init_only() {
    let source = r#"def make_user(
    id: int,
    display_name: str,
) -> None:
    ...

class User:
    def __init__(
        self,
        id: int,
        display_name: str,
    ) -> None:
        ...

    def update(
        self,
        id: int,
        display_name: str,
    ) -> None:
        ...
"#;

    let expected = r#"def make_user(
    id: int,
    display_name: str,
) -> None: ...


class User:
    def __init__(
        self,
        id:           int,
        display_name: str,
    ) -> None: ...

    def update(
        self,
        id: int,
        display_name: str,
    ) -> None: ...
"#;

    assert_eq!(
        format(
            source,
            function_param_alignment_options_with_scope(FunctionParamAlignmentScope::ClassInit),
        ),
        expected
    );
}

#[test]
fn function_parameter_scope_can_target_functions_only() {
    let source = r#"def make_user(
    id: int,
    display_name: str,
) -> None:
    ...

class User:
    def update(
        self,
        id: int,
        display_name: str,
    ) -> None:
        ...
"#;

    let expected = r#"def make_user(
    id:           int,
    display_name: str,
) -> None: ...


class User:
    def update(
        self,
        id: int,
        display_name: str,
    ) -> None: ...
"#;

    assert_eq!(
        format(
            source,
            function_param_alignment_options_with_scope(FunctionParamAlignmentScope::Functions),
        ),
        expected
    );
}

#[test]
fn parameter_comments_and_multiline_defaults_split_groups() {
    let source = r#"def f(
    id: int,
    display_name: str,
    # account state
    active: bool = True,
    last_seen_at: datetime | None = None,
    complex_default: str = (
        "value"
    ),
    email: str = "",
    phone_number: str = "",
) -> None:
    ...
"#;

    let expected = r#"def f(
    id:           int,
    display_name: str,
    # account state
    active:       bool            = True,
    last_seen_at: datetime | None = None,
    complex_default: str = ("value"),
    email:        str = "",
    phone_number: str = "",
) -> None: ...
"#;

    assert_eq!(format_function_params(source), expected);
}

#[test]
fn collection_defaults_that_expand_split_parameter_groups() {
    let source = r#"def f(
    short: int = 1,
    longer_name: str | None = None,
    items: tuple[int, str] = (1, "x"),
    another: bool = False,
) -> None:
    ...
"#;
    let mut custom = TuffCustomOptions::default();
    custom.alignment.function_params = AlignmentMode::Enabled;
    custom.collections.tuples.layout = CollectionLayout::ForceExpanded;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    let expected = r#"def f(
    short:       int        = 1,
    longer_name: str | None = None,
    items: tuple[int, str] = (
        1,
        "x",
    ),
    another: bool = False,
) -> None: ...
"#;

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn parameter_trailing_comments_can_split_groups() {
    let source = r#"def f(
    video_path: str = Field(min_length=1),  # required
    credentials_path: str = Field(min_length=1),
    snippet: dict[str, Any] = Field(default_factory=dict),
    status: dict[str, Any] = Field(default_factory=dict),
) -> None:
    ...
"#;

    let expected = r#"def f(
    video_path: str = Field(min_length=1),  # required
    credentials_path: str            = Field(min_length=1),
    snippet:          dict[str, Any] = Field(default_factory=dict),
    status:           dict[str, Any] = Field(default_factory=dict),
) -> None: ...
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.function_params = AlignmentMode::Enabled;
    custom.alignment.break_on_trailing_comment = true;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn class_and_parameter_alignment_can_be_enabled_together() {
    let source = r#"class User:
    id: int
    display_name: str

def create_user(
    id: int,
    display_name: str,
) -> User:
    ...
"#;

    let expected = r#"class User:
    id:           int
    display_name: str


def create_user(
    id:           int,
    display_name: str,
) -> User: ...
"#;

    assert_eq!(format_both(source), expected);
}

#[test]
fn collection_defaults_that_expand_split_class_field_groups() {
    let source = r#"class Config:
    short: int = 1
    longer_name: str | None = None
    items: tuple[int, str] = (1, "x")
    another: bool = False
"#;
    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.collections.tuples.layout = CollectionLayout::ForceExpanded;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    let expected = r#"class Config:
    short:       int        = 1
    longer_name: str | None = None
    items: tuple[int, str] = (
        1,
        "x",
    )
    another: bool = False
"#;

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn range_formatting_does_not_align_partial_class_field_group() {
    let source = r#"class User:
    id: int
    display_name: str
    active: bool
"#;
    let start = source.find("display_name").unwrap();
    let end = source[start..].find('\n').unwrap() + start + 1;

    let printed = format_range(
        source,
        TextRange::new(
            TextSize::try_from(start).unwrap(),
            TextSize::try_from(end).unwrap(),
        ),
        class_field_alignment_options(),
    )
    .unwrap();

    assert!(printed.as_code().contains("display_name: str"));
    assert!(!printed.as_code().contains("display_name:  str"));
}

#[test]
fn range_formatting_does_not_align_partial_class_field_default_group() {
    let source = r#"class Settings:
    video_path: str = Field(min_length=1)
    snippet: dict[str, Any] = Field(default_factory=dict)
    notify_subscribers: bool = False
"#;
    let start = source.find("snippet").unwrap();
    let end = source[start..].find('\n').unwrap() + start + 1;

    let printed = format_range(
        source,
        TextRange::new(
            TextSize::try_from(start).unwrap(),
            TextSize::try_from(end).unwrap(),
        ),
        class_field_alignment_options(),
    )
    .unwrap();

    assert!(printed.as_code().contains("snippet: dict[str, Any] ="));
    assert!(!printed.as_code().contains("snippet: dict[str, Any]  ="));
}

#[test]
fn range_formatting_aligns_whole_parameter_group() {
    let source = r#"def create_user(
    id: int,
    display_name: str,
) -> User: ...
"#;
    let start = source.find("def create_user").unwrap();
    let end = source.find(") -> User").unwrap() + 1;

    let printed = format_range(
        source,
        TextRange::new(
            TextSize::try_from(start).unwrap(),
            TextSize::try_from(end).unwrap(),
        ),
        function_param_alignment_options(),
    )
    .unwrap();

    assert!(printed.as_code().contains("id:           int"));
    assert!(printed.as_code().contains("display_name: str"));
}

#[test]
fn aligns_call_keyword_arguments() {
    let source = r#"def f(client):
    client.create_job(
        name="sync",
        retries=3,
        timeout_s=60,
        notify_on_failure=True,
    )
"#;

    let expected = r#"def f(client):
    client.create_job(
        name              = "sync",
        retries           = 3,
        timeout_s         = 60,
        notify_on_failure = True,
    )
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.call_keyword_args = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn call_keyword_alignment_skips_single_line_calls() {
    let source = r#"def f(client):
    client.create_job(name="sync", notify_on_failure=True)
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.call_keyword_args = AlignmentMode::Enabled;

    assert_eq!(
        format(source, PyFormatOptions::default().with_custom(custom)),
        source
    );
}

#[test]
fn call_keyword_alignment_skips_complex_values() {
    let source = r#"def f(segment, chunk):
    emit(
        confidence=segment.confidence if segment.confidence is not None else None,
        text=chunk.text,
    )
"#;

    let expected = r#"def f(segment, chunk):
    emit(
        confidence=segment.confidence if segment.confidence is not None else None,
        text=chunk.text,
    )
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.call_keyword_args = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_dict_values() {
    let source = r#"payload = {
    "video_path": video_path,
    "credentials_path": credentials_path,
    "notify_subscribers": False,
}
"#;

    let expected = r#"payload = {
    "video_path":         video_path,
    "credentials_path":   credentials_path,
    "notify_subscribers": False,
}
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.dict_values = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_dict_colons() {
    let source = r#"payload = {
    "video_path": video_path,
    "credentials_path": credentials_path,
    "notify_subscribers": False,
}
"#;

    let expected = r#"payload = {
    "video_path"        : video_path,
    "credentials_path"  : credentials_path,
    "notify_subscribers": False,
}
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.dict_alignment = DictAlignmentMode::Colon;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_collection_rows() {
    let source = r#"rows = [
    ("id", "name", "active"),
    (1, "Ana", True),
    (20, "Benedict", False),
]
"#;

    let expected = r#"rows = [
    ("id", "name",     "active"),
    (1,    "Ana",      True),
    (20,   "Benedict", False),
]
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.collection_rows = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_repeated_call_arguments() {
    let source = r#"router.add_route("GET", "/users", list_users)
router.add_route("POST", "/users", create_user)
router.add_route("DELETE", "/users/{id}", delete_user)
"#;

    let expected = r#"router.add_route("GET",    "/users",      list_users)
router.add_route("POST",   "/users",      create_user)
router.add_route("DELETE", "/users/{id}", delete_user)
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.repeated_call_args = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_with_items() {
    let source = r#"with (
    open(input_path) as input_file,
    open(output_path) as output_file,
    lock as acquired_lock,
):
    process()
"#;

    let expected = r#"with (
    open(input_path)  as input_file,
    open(output_path) as output_file,
    lock              as acquired_lock,
):
    process()
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.with_items = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_trailing_comments() {
    let source = r#"HOST = "localhost"  # main API host
PORT = 443  # TLS
TIMEOUT_SECONDS = 30  # request timeout
"#;

    let expected = r#"HOST = "localhost"    # main API host
PORT = 443            # TLS
TIMEOUT_SECONDS = 30  # request timeout
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.trailing_comments = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn dict_value_alignment_skips_complex_values() {
    let source = r#"payload = {
    "confidence": segment.confidence if segment.confidence is not None else None,
    "text": chunk.text,
}
"#;

    let expected = r#"payload = {
    "confidence": segment.confidence if segment.confidence is not None else None,
    "text": chunk.text,
}
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.dict_values = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn class_field_scope_can_target_typeddicts_with_schema_scope() {
    let source = r#"class Plain:
    video_path: str
    notify_subscribers: bool

class Payload(TypedDict):
    video_path: str
    notify_subscribers: bool
"#;

    let expected = r#"class Plain:
    video_path: str
    notify_subscribers: bool


class Payload(TypedDict):
    video_path:         str
    notify_subscribers: bool
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.class_fields = AlignmentMode::Enabled;
    custom.alignment.class_field_scope = ClassFieldAlignmentScope::DataclassesPydanticAndTypedDict;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}

#[test]
fn aligns_consecutive_import_aliases() {
    let source = r#"from app.services import YoutubeUploadService as UploadService
from app.transport import YoutubeTransport as Transport
from app.errors import YtupUpstreamUploadError as UploadError
"#;

    let expected = r#"from app.services import YoutubeUploadService    as UploadService
from app.transport import YoutubeTransport        as Transport
from app.errors import YtupUpstreamUploadError as UploadError
"#;

    let mut custom = TuffCustomOptions::default();
    custom.alignment.import_aliases = AlignmentMode::Enabled;
    let options = PyFormatOptions::default().with_custom(custom);
    let formatted = format(source, options.clone());

    assert_eq!(formatted, expected);
    assert_eq!(format(&formatted, options), formatted);
}
