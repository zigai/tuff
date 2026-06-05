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
