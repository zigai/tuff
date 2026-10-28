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
