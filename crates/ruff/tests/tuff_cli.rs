use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn tuff() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tuff"))
}

#[test]
fn formats_file_with_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
class-fields = "enabled"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(&path, "class User:\n    id: int\n    display_name: str\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "class User:\n    id:           int\n    display_name: str\n"
    );
}

#[test]
fn dedicated_tuff_config_wins_over_generic_pyproject() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[project]
name = "example"
"#,
    )
    .unwrap();
    fs::write(
        tempdir.path().join("tuff.toml"),
        r#"
[format.alignment]
class-fields = "enabled"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(&path, "class User:\n    id: int\n    display_name: str\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "class User:\n    id:           int\n    display_name: str\n"
    );
}

#[test]
fn reads_alignment_scope_and_default_alignment_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
function-params = "enabled"
function-param-scope = "class-init"
align-defaults = true
assignments = "enabled"
assignment-scope = "class"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(
        &path,
        "def f(short: int, longer_name: str): ...\n\nclass Upload:\n    def __init__(\n        self,\n        short: int = 1,\n        longer_name: str | None = None,\n    ) -> None: ...\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let formatted = fs::read_to_string(path).unwrap();
    assert!(formatted.contains("def f(short: int, longer_name: str): ..."));
    assert!(formatted.contains("short:       int        = 1"));
    assert!(formatted.contains("longer_name: str | None = None"));
}

#[test]
fn reads_assignment_scope_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
assignments = "enabled"
assignment-scope = "enum-class"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(
        &path,
        "class Plain:\n    SHORT = 'short'\n    MUCH_LONGER = 'long'\n\nclass Codes(StrEnum):\n    SHORT = 'short'\n    MUCH_LONGER = 'long'\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let formatted = fs::read_to_string(path).unwrap();
    assert!(formatted.contains("class Plain:\n    SHORT = \"short\"\n    MUCH_LONGER = \"long\""));
    assert!(formatted.contains("class Codes(StrEnum):\n    SHORT       = \"short\""));
    assert!(formatted.contains("    MUCH_LONGER = \"long\""));
}

#[test]
fn reads_assignment_class_scope_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
assignments = "enabled"
assignment-scope = "class"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(
        &path,
        "class UploadError:\n    STATUS_CODE = 502\n    ERROR_CODE = ServiceErrorCode.UPSTREAM_UPLOAD_FAILED\n    DEFAULT_DETAIL = 'YouTube upload failed.'\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "class UploadError:\n    STATUS_CODE    = 502\n    ERROR_CODE     = ServiceErrorCode.UPSTREAM_UPLOAD_FAILED\n    DEFAULT_DETAIL = \"YouTube upload failed.\"\n"
    );
}

#[test]
fn alignment_preserve_input_mode_is_accepted() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
assignments = "preserve-input"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(&path, "SHORT = 'short'\nMUCH_LONGER = 'long'\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "SHORT = \"short\"\nMUCH_LONGER = \"long\"\n"
    );
}

#[test]
fn reads_compact_format_config() {
    for (filename, prefix) in [("pyproject.toml", "tool.tuff."), ("tuff.toml", "")] {
        let tempdir = tempdir().unwrap();
        fs::write(
            tempdir.path().join(filename),
            format!(
                r#"
[{prefix}format.collections]
lists = {{ layout = "prefer-compact", ignore-magic-trailing-comma = true, compact-nested = true }}

[{prefix}format.arguments]
calls = "enabled"
parameters = "enabled"
mixed = "enabled"
ignore-magic-trailing-comma = true

[{prefix}format.imports]
mode = "enabled"
ignore-magic-trailing-comma = true

[{prefix}format.one-line-suites]
mode = "enabled"
clauses = ["function", "class"]
statements = ["pass", "ellipsis", "return"]
"#
            ),
        )
        .unwrap();
        let path = tempdir.path().join("example.py");
        fs::write(
            &path,
            "from package import (\n    Alpha,\n    Beta,\n)\n\nvalues = [\n    1,\n    2,\n]\n\nresult = call(\n    first,\n    second,\n)\n\ndef get(\n    first,\n    second,\n):\n    return first\n\nclass Empty:\n    pass\n",
        )
        .unwrap();

        let output = tuff().arg("format").arg(&path).output().unwrap();
        assert!(output.status.success(), "stderr: {}", stderr(&output));
        let formatted = fs::read_to_string(&path).unwrap();
        assert!(formatted.contains("from package import Alpha, Beta\n"));
        assert!(formatted.contains("values = [1, 2]\n"));
        assert!(formatted.contains("result = call(first, second)\n"));
        assert!(formatted.contains("def get(first, second): return first\n"));
        assert!(formatted.contains("class Empty: pass\n"));

        let output = tuff()
            .arg("format")
            .arg("--check")
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {}", stderr(&output));
    }
}

#[test]
fn compact_modes_are_opt_in_and_invalidate_cached_formatting() {
    let tempdir = tempdir().unwrap();
    let config = tempdir.path().join("tuff.toml");
    let path = tempdir.path().join("example.py");
    let source = "result = call(\n    first,\n    second,\n)\n";
    fs::write(&path, source).unwrap();
    for (mode, expected) in [
        ("disabled", source),
        ("enabled", "result = call(first, second)\n"),
    ] {
        fs::write(
            &config,
            format!("[format.arguments]\ncalls = '{mode}'\nignore-magic-trailing-comma = true\n"),
        )
        .unwrap();
        let output = tuff().arg("format").arg(&path).output().unwrap();
        assert!(output.status.success(), "stderr: {}", stderr(&output));
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    }
}

#[test]
fn compact_config_errors_do_not_write_files() {
    for (table, setting, expected) in [
        ("arguments", "calls = true", "expected string"),
        ("imports", "mode = 'fill'", "unsupported feature mode"),
        ("arguments", "call = 'enabled'", "unknown field `call`"),
        (
            "collections",
            "lists = { compact-nested = 'enabled' }",
            "expected boolean",
        ),
        ("one-line-suites", "docstrings = true", "expected string"),
    ] {
        let tempdir = tempdir().unwrap();
        fs::write(
            tempdir.path().join("tuff.toml"),
            format!("[format.{table}]\n{setting}\n"),
        )
        .unwrap();
        let path = tempdir.path().join("example.py");
        fs::write(&path, "value=[1,2]\n").unwrap();
        let output = tuff().arg("format").arg(&path).output().unwrap();
        assert!(!output.status.success());
        assert!(stderr(&output).contains(expected), "{}", stderr(&output));
        assert_eq!(fs::read_to_string(path).unwrap(), "value=[1,2]\n");
    }
}

#[test]
fn reads_one_line_suite_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.one-line-suites]
mode = "enabled"
clauses = ["if"]
statements = ["return"]
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(
        &path,
        "def f(on_progress):\n    if on_progress is None:\n        return\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "def f(on_progress):\n    if on_progress is None: return\n"
    );
}

#[test]
fn collection_threshold_config_is_not_order_dependent() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.collections]
lists = { threshold = 10, layout = "expand-if-more-than" }
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(&path, "values=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(fs::read_to_string(path).unwrap(), "values = [1, 2]\n");
}

#[test]
fn reads_docstring_args_section_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.docstrings]
args-sections = "enabled"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(
        &path,
        "def f(short, much_longer):\n    \"\"\"Do it.\n\n    Args:\n        short: Short value.\n        much_longer: Longer value.\n    \"\"\"\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(fs::read_to_string(path).unwrap().contains(
        "    Args:\n        short:       Short value.\n        much_longer: Longer value."
    ));
}

#[test]
fn reads_custom_format_config_from_tuff_toml() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("tuff.toml"),
        r#"
[format.alignment]
assignments = "enabled"
assignment-scope = "enum-class"

[format.docstrings]
args-sections = "enabled"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(
        &path,
        "class Codes(StrEnum):\n    SHORT = 'short'\n    MUCH_LONGER = 'long'\n\ndef f(short, much_longer):\n    \"\"\"Do it.\n\n    Args:\n        short: Short value.\n        much_longer: Longer value.\n    \"\"\"\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let formatted = fs::read_to_string(path).unwrap();
    assert!(formatted.contains("    SHORT       = \"short\""));
    assert!(formatted.contains("    MUCH_LONGER = \"long\""));
    assert!(formatted.contains(
        "    Args:\n        short:       Short value.\n        much_longer: Longer value."
    ));
}

#[test]
fn reads_extended_alignment_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
call-keyword-args = "enabled"
dict-values = "enabled"
import-aliases = "enabled"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(
        &path,
        r#"from app.services import YoutubeUploadService as UploadService
from app.transport import YoutubeTransport as Transport

payload = {
    "video_path": video_path,
    "credentials_path": credentials_path,
}

create_job(
    name="sync",
    notify_on_failure=True,
)
"#,
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let formatted = fs::read_to_string(path).unwrap();
    assert!(formatted.contains("YoutubeUploadService as UploadService"));
    assert!(formatted.contains("YoutubeTransport     as Transport"));
    assert!(formatted.contains("\"video_path\":       video_path"));
    assert!(formatted.contains("name              = \"sync\""));
}

#[test]
fn reads_new_alignment_and_fill_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
dict-alignment = "colon"
collection-rows = "enabled"
repeated-call-args = "enabled"
with-items = "enabled"
trailing-comments = "enabled"

[tool.tuff.format.collections]
lists = { layout = "fill" }
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(
        &path,
        r#"payload = {
    "video_path": video_path,
    "credentials_path": credentials_path,
}

permissions = [
    "read",
    "write",
    "delete",
    "admin",
    "billing",
]

rows = [
    ("id", "name", "active"),
    (1, "Ana", True),
    (20, "Benedict", False),
]

router.add_route("GET", "/users", list_users)
router.add_route("DELETE", "/users/{id}", delete_user)

with (
    open(input_path) as input_file,
    open(output_path) as output_file,
):
    process()

HOST = "localhost"  # main API host
PORT = 443  # TLS
"#,
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let formatted = fs::read_to_string(path).unwrap();
    assert!(formatted.contains("\"video_path\"      : video_path"));
    assert!(formatted.contains("\"read\", \"write\", \"delete\", \"admin\", \"billing\","));
    assert!(formatted.contains("(1,    \"Ana\",      True)"));
    assert!(formatted.contains("router.add_route(\"GET\",    \"/users\",      list_users)"));
    assert!(formatted.contains("open(input_path)  as input_file"));
    assert!(formatted.contains("HOST = \"localhost\"  # main API host"));
    assert!(formatted.contains("PORT = 443          # TLS"));
}

#[test]
fn check_exits_nonzero_when_file_would_change() {
    let tempdir = tempdir().unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(&path, "x=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(!output.status.success());
}

#[test]
fn diff_prints_unified_diff_without_writing() {
    let tempdir = tempdir().unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(&path, "x=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg("--diff")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-x=[1,2]"));
    assert!(stdout.contains("+x = [1, 2]"));
    assert_eq!(fs::read_to_string(path).unwrap(), "x=[1,2]\n");
}

#[test]
fn formats_stdin() {
    let mut child = tuff()
        .arg("format")
        .arg("--stdin-filename")
        .arg("example.py")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run tuff");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"x=[1,2]\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "x = [1, 2]\n");
}

#[test]
fn stdin_check_diff_prints_diff_before_failing() {
    let mut child = tuff()
        .arg("format")
        .arg("--check")
        .arg("--diff")
        .arg("--stdin-filename")
        .arg("example.py")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run tuff");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"x=[1,2]\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    assert!(stdout.contains("example.py"));
    assert!(stdout.contains("+x = [1, 2]"));
}

#[test]
fn unknown_tuff_config_field_errors() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.alignment]
class-field = "enabled"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("model.py");
    fs::write(&path, "class User:\n    id: int\n    display_name: str\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown field `class-field`"));
}

#[test]
fn unknown_collection_policy_field_errors() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.tuff.format.collections]
lists = { layout = "ruff-default", extra = true }
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(&path, "x=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown field `extra`"));
}

#[test]
fn invalid_line_length_errors_before_formatting() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.ruff]
line-length = -1
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(&path, "x=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("line-length"));
    assert_eq!(fs::read_to_string(path).unwrap(), "x=[1,2]\n");
}

#[test]
fn parse_errors_do_not_write_files() {
    let tempdir = tempdir().unwrap();
    let path = tempdir.path().join("broken.py");
    fs::write(&path, "x = [\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "x = [\n");
}

#[test]
fn ruff_config_supplies_standard_format_options() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.ruff]
line-length = 18
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(
        &path,
        "value = function_call(first_argument, second_argument)\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("first_argument,\n")
    );
}

#[test]
fn reads_ruff_toml_as_compatible_fallback() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("ruff.toml"),
        r#"
line-length = 18
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(
        &path,
        "value = function_call(first_argument, second_argument)\n",
    )
    .unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("first_argument,\n")
    );
}

#[test]
fn reads_indent_style_and_line_ending_config() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.ruff.format]
indent-style = "tab"
line-ending = "cr-lf"
"#,
    )
    .unwrap();
    let path = tempdir.path().join("example.py");
    fs::write(&path, "if True:\n    x=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "if True:\r\n\tx = [1, 2]\r\n"
    );
}

#[test]
fn directory_formatting_respects_include() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.ruff]
include = ["*.pyi"]
"#,
    )
    .unwrap();
    let py = tempdir.path().join("example.py");
    let pyi = tempdir.path().join("example.pyi");
    fs::write(&py, "x=[1,2]\n").unwrap();
    fs::write(&pyi, "y=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(tempdir.path())
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(fs::read_to_string(py).unwrap(), "x=[1,2]\n");
    assert_eq!(fs::read_to_string(pyi).unwrap(), "y = [1, 2]\n");
}

#[test]
fn directory_formatting_respects_exclude() {
    let tempdir = tempdir().unwrap();
    fs::write(
        tempdir.path().join("pyproject.toml"),
        r#"
[tool.ruff]
exclude = ["generated"]
"#,
    )
    .unwrap();
    let generated = tempdir.path().join("generated");
    fs::create_dir(&generated).unwrap();
    let included = tempdir.path().join("included.py");
    let excluded = generated.join("excluded.py");
    fs::write(&included, "x=[1,2]\n").unwrap();
    fs::write(&excluded, "y=[1,2]\n").unwrap();

    let output = tuff()
        .arg("format")
        .arg(tempdir.path())
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(fs::read_to_string(included).unwrap(), "x = [1, 2]\n");
    assert_eq!(fs::read_to_string(excluded).unwrap(), "y=[1,2]\n");
}

#[cfg(unix)]
#[test]
fn file_formatting_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tempdir = tempdir().unwrap();
    let path = tempdir.path().join("script.py");
    fs::write(&path, "x=[1,2]\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

    let output = tuff()
        .arg("format")
        .arg(&path)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[cfg(unix)]
#[test]
fn directory_formatting_does_not_follow_symlinked_directories() {
    use std::os::unix::fs::symlink;

    let tempdir = tempdir().unwrap();
    let outside = tempdir.path().join("outside");
    let root = tempdir.path().join("root");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(&root).unwrap();
    let outside_file = outside.join("outside.py");
    fs::write(&outside_file, "x=[1,2]\n").unwrap();
    symlink(&outside, root.join("linked")).unwrap();

    let output = tuff()
        .arg("format")
        .arg(&root)
        .output()
        .expect("failed to run tuff");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(fs::read_to_string(outside_file).unwrap(), "x=[1,2]\n");
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn lint_selected_spacing_formats_with_shared_options() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(directory.path().join("pyproject.toml"), "[tool.ruff.lint]\nselect = []\n[tool.tuff.lint]\nenable = ['blank-line-before-branch']\n[tool.tuff.lint.rules.blank-line-before-branch]\nmax_suite_non_empty_lines = 1\nallow_related_return_tails = false\n").unwrap();
    let path = directory.path().join("input.py");
    fs::write(&path, "def f():\n    value = 1\n    return value\n").unwrap();
    let output = tuff().arg("format").arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "def f():\n    value = 1\n\n    return value\n"
    );
    let output = tuff()
        .args(["check", "--no-cache", "--output-format", "json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn formatter_only_spacing_does_not_enable_lint() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(directory.path().join("pyproject.toml"), "[tool.ruff.lint]\nselect = []\n[tool.tuff.format.blank-lines]\nrules = ['no-suite-leading-trailing-blank-lines']\n").unwrap();
    let path = directory.path().join("input.py");
    fs::write(&path, "def f():\n\n    work()\n").unwrap();
    let output = tuff()
        .args(["check", "--no-cache", "--output-format", "json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
    let output = tuff().arg("format").arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(fs::read_to_string(path).unwrap(), "def f():\n    work()\n");
}

#[test]
fn explicit_and_isolated_configs_control_custom_formatting() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(
        directory.path().join("tuff.toml"),
        "[format.blank-lines]\nrules = ['no-suite-leading-trailing-blank-lines']\n",
    )
    .unwrap();
    let explicit = directory.path().join("other.toml");
    fs::write(&explicit, "[format]\nquote-style = 'single'\n").unwrap();
    let path = directory.path().join("input.py");
    for args in [
        vec!["--isolated"],
        vec!["--config", explicit.to_str().unwrap()],
    ] {
        fs::write(&path, "def f():\n\n    work()\n").unwrap();
        let output = tuff().arg("format").args(args).arg(&path).output().unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "def f():\n\n    work()\n"
        );
    }
}

#[test]
fn invalid_native_spacing_config_fails_before_writing() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let path = directory.path().join("input.py");
    let source = "def f():\n\n    work( )\n";
    for (config, message) in [
        (
            "[tool.tuff.lint]\nenable = ['missing-rule']\n",
            "missing-rule",
        ),
        (
            "[tool.tuff.format.blank-lines]\nrules = ['forbidden-call']\n",
            "forbidden-call",
        ),
        (
            "[tool.tuff.format.blank-lines]\nrules = []\nunknown = true\n",
            "unknown",
        ),
        (
            "[tool.tuff.lint]\nenable = ['blank-line-before-branch']\n[tool.tuff.lint.rules.blank-line-before-branch]\nmax_suite_non_empty_lines = -1\n",
            "invalid value",
        ),
        ("[tool.tuff.lint]\nenable = ['all']\n", "mutually exclusive"),
        (
            "[tool.tuff.format.blank-lines]\nrules = ['blank-line-before-unrelated-block', 'block-header-cuddle-strict']\n",
            "mutually exclusive",
        ),
    ] {
        fs::write(directory.path().join("pyproject.toml"), config).unwrap();
        for command in ["check", "format"] {
            fs::write(&path, source).unwrap();
            let output = tuff().arg(command).arg(&path).output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{config}: {}",
                stderr(&output)
            );
            assert!(
                stderr(&output).contains(message),
                "{config}: {}",
                stderr(&output)
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }
}

#[test]
fn native_disables_override_formatter_selection_and_conflicts() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(directory.path().join("tuff.toml"), "[lint]\ndisable = ['no-suite-leading-trailing-blank-lines', 'blank-line-before-unrelated-block']\n[format.blank-lines]\nrules = ['no-suite-leading-trailing-blank-lines', 'blank-line-before-unrelated-block', 'block-header-cuddle-strict']\n").unwrap();
    let path = directory.path().join("input.py");
    fs::write(&path, "def f():\n\n    work()\n").unwrap();
    let output = tuff().arg("format").arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "def f():\n\n    work()\n"
    );
}

#[test]
fn inherited_custom_tables_share_rule_options() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(directory.path().join("pyproject.toml"), "[tool.tuff.lint]\nenable = ['blank-line-before-branch']\n[tool.tuff.lint.rules.blank-line-before-branch]\nmax_suite_non_empty_lines = 1\nallow_related_return_tails = false\n[tool.tuff.format.alignment]\nassignments = 'enabled'\n").unwrap();
    let child = directory.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("tuff.toml"), "extend = '../pyproject.toml'\n[format.blank-lines]\nrules = ['no-suite-leading-trailing-blank-lines']\n[format.alignment]\nmin-group-size = 2\n").unwrap();
    let path = child.join("input.py");
    fs::write(
        &path,
        "def f():\n\n    a = 1\n    longer = 2\n    return longer\n",
    )
    .unwrap();
    let output = tuff().arg("format").arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "def f():\n    a      = 1\n    longer = 2\n\n    return longer\n"
    );
}

#[test]
fn shared_tuff_pyproject_options_override_ruff_values() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(
        directory.path().join("pyproject.toml"),
        "[tool.ruff.format]\nquote-style = 'double'\n[tool.tuff.format]\nquote-style = 'single'\n",
    )
    .unwrap();
    let path = directory.path().join("input.py");
    fs::write(&path, "value = \"text\"\n").unwrap();
    let output = tuff().arg("format").arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(fs::read_to_string(path).unwrap(), "value = 'text'\n");
}

#[test]
fn broad_ruff_selectors_keep_native_rules_opt_in() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let path = directory.path().join("input.py");
    fs::write(&path, "class _Hidden:\n    pass\n").unwrap();
    for selector in ["ALL", "restriction"] {
        let output = tuff()
            .args([
                "check",
                "--isolated",
                "--preview",
                "--no-cache",
                "--select",
                selector,
                "--output-format",
                "json",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "{}",
            stderr(&output)
        );
        let reports: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(reports.as_array().unwrap().iter().all(|report| {
            !report["code"]
                .as_str()
                .is_some_and(|code| code.starts_with("TUF"))
        }));
    }
}

#[test]
fn inherited_native_reenable_is_shared_by_lint_and_format() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(directory.path().join("pyproject.toml"), "[tool.ruff.lint]\nselect = []\n[tool.tuff.lint]\ndisable = ['no-suite-leading-trailing-blank-lines']\n").unwrap();
    let child = directory.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("tuff.toml"), "extend = '../pyproject.toml'\n[lint]\nenable = ['no-suite-leading-trailing-blank-lines']\n").unwrap();
    let path = child.join("input.py");
    fs::write(&path, "def run():\n\n    pass\n").unwrap();
    let lint = tuff()
        .args(["check", "--no-cache", "--output-format", "json"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(lint.status.code(), Some(1), "{}", stderr(&lint));
    let formatted = tuff()
        .args(["format", "--no-cache"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(formatted.status.success(), "{}", stderr(&formatted));
    assert_eq!(fs::read_to_string(&path).unwrap(), "def run():\n    pass\n");
    let lint = tuff()
        .args(["check", "--no-cache", "--output-format", "json"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(lint.status.code(), Some(0), "{}", stderr(&lint));
}

const PYTHON_RULES: &str = r#"
import ast
from tuff_rules import TuffRule


class NoPrint(TuffRule):
    name = "no-print"
    source_patterns = ("print",)
    node_kinds = ("Call",)

    def check(self, tree, ctx):
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and ctx.qualified_name(node.func) == "builtins.print":
                ctx.report(node.func, "Use logging", [ctx.edit(node.func, ctx.options.get("to", "log"))])


class CallNames(TuffRule):
    name = "call-names"
    include = ("names/**",)

    def check(self, tree, ctx):
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                ctx.report(node.func, str(ctx.qualified_name(node.func)))
"#;

/// A project whose `rules/` directory holds [`PYTHON_RULES`] and whose interpreter
/// records each start in `spawns.log`.
fn python_rule_project(config: &str) -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let root = directory.path();
    fs::create_dir(root.join("rules")).unwrap();
    fs::write(root.join("rules/rules.py"), PYTHON_RULES).unwrap();
    let wrapper = root.join("python");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho started >> '{}'\nexec python3 \"$@\"\n",
            root.join("spawns.log").display(),
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        root.join("pyproject.toml"),
        format!(
            "[tool.ruff.lint]\nselect = []\n[tool.tuff.lint]\nplugin-dirs = ['rules']\npython = './python'\n{config}"
        ),
    )
    .unwrap();
    directory
}

fn spawns(root: &std::path::Path) -> usize {
    fs::read_to_string(root.join("spawns.log")).map_or(0, |log| log.lines().count())
}

fn json_check(root: &std::path::Path, args: &[&str]) -> (Option<i32>, serde_json::Value, String) {
    let output = tuff()
        .current_dir(root)
        .args(["check", "--output-format", "json"])
        .args(args)
        .output()
        .unwrap();
    let reports = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    (output.status.code(), reports, stderr(&output))
}

fn codes_and_rows(reports: &serde_json::Value) -> Vec<(String, u64)> {
    let mut rows: Vec<_> = reports
        .as_array()
        .unwrap()
        .iter()
        .map(|report| {
            (
                report["code"].as_str().unwrap_or("<none>").to_owned(),
                report["location"]["row"].as_u64().unwrap(),
            )
        })
        .collect();
    rows.sort();
    rows
}

#[cfg(unix)]
#[test]
fn python_rules_report_and_fix_with_tuff_ignore_comments() {
    let directory = python_rule_project("[tool.tuff.lint.rules.no-print]\nto = 'emit'\n");
    let root = directory.path();
    fs::write(
        root.join("app.py"),
        "import builtins as b\nprint(1)\nb.print(2)\nprint(3)  # tuff: ignore[no-print]\n",
    )
    .unwrap();
    let (status, reports, stderr) = json_check(root, &["--no-cache", "app.py"]);
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(
        codes_and_rows(&reports),
        [("no-print".to_owned(), 2), ("no-print".to_owned(), 3)]
    );
    let (status, _, stderr) = json_check(root, &["--no-cache", "--fix", "app.py"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        fs::read_to_string(root.join("app.py")).unwrap(),
        "import builtins as b\nemit(1)\nemit(2)\nprint(3)  # tuff: ignore[no-print]\n"
    );
}

#[cfg(unix)]
#[test]
fn python_rules_resolve_aliased_imports() {
    let directory = python_rule_project("");
    let root = directory.path();
    fs::create_dir(root.join("names")).unwrap();
    fs::write(
        root.join("names/calls.py"),
        "import foo as bar\nimport os.path as osp\nfrom pkg import tool as t\n\nbar.baz()\nosp.join('a')\nt.run()\nlocal()\n",
    )
    .unwrap();
    let (status, reports, stderr) = json_check(root, &["--no-cache", "names/calls.py"]);
    assert_eq!(status, Some(1), "{stderr}");
    let messages: Vec<_> = reports
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        ["foo.baz", "os.path.join", "pkg.tool.run", "None"]
    );
}

#[cfg(unix)]
#[test]
fn python_rules_follow_selection_and_per_file_ignores() {
    let directory = python_rule_project(
        "[tool.ruff.lint.per-file-ignores]\n'ignored.py' = ['no-print', 'F401']\n",
    );
    let root = directory.path();
    for name in ["app.py", "ignored.py"] {
        fs::write(root.join(name), "print(1)\n").unwrap();
    }
    let (status, reports, stderr) = json_check(root, &["--no-cache", "."]);
    assert_eq!(status, Some(1), "{stderr}");
    let files: Vec<_> = reports
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["filename"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with("app.py"), "{files:?}");

    let pyproject = root.join("pyproject.toml");
    let config = fs::read_to_string(&pyproject).unwrap();
    fs::write(
        &pyproject,
        config.replace(
            "python = './python'\n",
            "python = './python'\ndisable = ['no-print']\n",
        ),
    )
    .unwrap();
    let (status, reports, stderr) = json_check(root, &["--no-cache", "."]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(reports, serde_json::json!([]));
}

#[cfg(unix)]
#[test]
fn unknown_python_rule_names_are_rejected() {
    let directory = python_rule_project("enable = ['no-such-rule']\n");
    let root = directory.path();
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    let (status, _, stderr) = json_check(root, &["--no-cache", "app.py"]);
    assert_eq!(status, Some(2));
    assert!(
        stderr.contains("unknown Tuff rule `no-such-rule`"),
        "{stderr}"
    );

    let directory = python_rule_project("[tool.tuff.lint.rules.no-such-rule]\nvalue = 1\n");
    let (status, _, stderr) = json_check(directory.path(), &["--no-cache", "."]);
    assert_eq!(status, Some(2));
    assert!(
        stderr.contains("unknown Tuff rule `no-such-rule`"),
        "{stderr}"
    );

    let directory = python_rule_project("");
    let root = directory.path();
    fs::write(
        root.join("app.py"),
        "x = 1  # tuff: ignore[no-print]\ny = 2  # tuff: ignore[no-such-rule]\n",
    )
    .unwrap();
    let (status, reports, stderr) = json_check(root, &["--no-cache", "app.py"]);
    assert_eq!(status, Some(1), "{stderr}");
    let reports = reports.as_array().unwrap();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["location"]["row"], 2);
    assert!(
        reports[0]["message"]
            .as_str()
            .unwrap()
            .contains("no-such-rule")
    );
}

#[test]
fn unknown_rule_names_without_plugins_are_rejected() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/config-tests");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    fs::write(
        directory.path().join("pyproject.toml"),
        "[tool.tuff.lint]\ndisable = ['no-such-rule']\n",
    )
    .unwrap();
    fs::write(directory.path().join("app.py"), "x = 1\n").unwrap();
    let (status, _, stderr) = json_check(directory.path(), &["--no-cache", "app.py"]);
    assert_eq!(status, Some(2));
    assert!(
        stderr.contains("unknown Tuff rule `no-such-rule`"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn cached_python_rule_runs_do_not_start_python() {
    let directory = python_rule_project("");
    let root = directory.path();
    fs::write(root.join("clean.py"), "value = 1\n").unwrap();
    fs::write(root.join("mentions.py"), "label = 'print'\n").unwrap();

    // The first run starts Python only to read the manifest: neither file passes the
    // `no-print` filters, and `call-names` only includes `names/`.
    let (status, _, stderr) = json_check(root, &["."]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(spawns(root), 1);

    let (status, _, stderr) = json_check(root, &["."]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(spawns(root), 1);

    // Editing a rule invalidates the manifest and the cached results of unchanged files.
    fs::write(
        root.join("rules/rules.py"),
        format!(
            "{PYTHON_RULES}\n\nclass NoLabel(TuffRule):\n    name = 'no-label'\n    include = ('mentions.py',)\n\n    def check(self, tree, ctx):\n        ctx.report(tree.body[0], 'label')\n"
        ),
    )
    .unwrap();
    let (status, reports, stderr) = json_check(root, &["."]);
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(codes_and_rows(&reports), [("no-label".to_owned(), 1)]);
    assert!(spawns(root) > 1);
}

#[cfg(unix)]
#[test]
fn watched_files_invalidate_cached_python_rule_results() {
    let directory = python_rule_project("");
    let root = directory.path();
    fs::write(
        root.join("rules/watch.py"),
        r#"
from tuff_rules import TuffRule


class Watch(TuffRule):
    name = "allowed-names"
    watch_files = ("allowed.txt",)

    def check(self, tree, ctx):
        allowed = (ctx.root / "allowed.txt").read_text().split()
        for statement in tree.body:
            for target in getattr(statement, "targets", []):
                if target.id not in allowed:
                    ctx.report(target, f"`{target.id}` is not allowed")
"#,
    )
    .unwrap();
    fs::write(root.join("allowed.txt"), "value\n").unwrap();
    fs::write(root.join("app.py"), "value = 1\n").unwrap();
    let (status, _, stderr) = json_check(root, &["app.py"]);
    assert_eq!(status, Some(0), "{stderr}");
    let before = spawns(root);
    let (status, _, stderr) = json_check(root, &["app.py"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(spawns(root), before);

    fs::write(root.join("allowed.txt"), "other\n").unwrap();
    let (status, reports, stderr) = json_check(root, &["app.py"]);
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(codes_and_rows(&reports), [("allowed-names".to_owned(), 1)]);
}

#[cfg(unix)]
#[test]
fn python_rule_failures_are_reported() {
    let directory = python_rule_project("");
    let root = directory.path();
    fs::write(
        root.join("rules/broken.py"),
        "from tuff_rules import TuffRule\n\nclass Broken(TuffRule):\n    name = 'broken-rule'\n\n    def check(self, tree, ctx):\n        raise RuntimeError('rule bug')\n",
    )
    .unwrap();
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    let output = tuff()
        .current_dir(root)
        .args(["check", "--no-cache", "app.py"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Python rule `broken-rule` failed")
            && stdout.contains("RuntimeError: rule bug"),
        "{stdout}"
    );

    fs::write(
        root.join("rules/broken.py"),
        "import missing_module_for_test\n",
    )
    .unwrap();
    let output = tuff()
        .current_dir(root)
        .args(["check", "--no-cache", "app.py"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr(&output);
    assert!(
        stderr.contains("failed to import") && stderr.contains("missing_module_for_test"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn python_rule_names_must_not_shadow_other_rules() {
    for (name, expected) in [
        ("unused-import", "same name as a Ruff rule"),
        ("forbidden-call", "same name as a native Tuff rule"),
    ] {
        let directory = python_rule_project("");
        let root = directory.path();
        fs::write(
            root.join("rules/clash.py"),
            format!(
                "from tuff_rules import TuffRule\n\nclass Clash(TuffRule):\n    name = '{name}'\n"
            ),
        )
        .unwrap();
        fs::write(root.join("app.py"), "x = 1\n").unwrap();
        let (status, _, stderr) = json_check(root, &["--no-cache", "app.py"]);
        assert_eq!(status, Some(2));
        assert!(stderr.contains(expected), "{stderr}");
    }
}
