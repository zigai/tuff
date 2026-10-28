#[path = "tuff/mod.rs"]
mod common;

#[test]
fn formatting_expanded_suites_is_stable_and_lint_clean() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lint-tests");
    std::fs::create_dir_all(&base).unwrap();
    for (rule, source, expected) in [
        (
            "blank-line-after-control-block",
            "def run(flag):\n    if flag: work()\n    done()\n",
            "def run(flag):\n    if flag:\n        work()\n\n    done()\n",
        ),
        (
            "blank-line-after-control-block",
            "def run():\n    with manager(): work()\n    done()\n",
            "def run():\n    with manager():\n        work()\n\n    done()\n",
        ),
        (
            "blank-line-after-control-block",
            "def run():\n    for item in items: work()\n    done()\n",
            "def run():\n    for item in items:\n        work()\n\n    done()\n",
        ),
        (
            "blank-line-before-assignment",
            "def run(): log(); value = 1\n",
            "def run():\n    log()\n\n    value = 1\n",
        ),
        (
            "blank-line-after-terminal-control-block",
            "def run(flag):\n    if flag: return 1\n    else: return 2\n    done()\n",
            "def run(flag):\n    if flag:\n        return 1\n    else:\n        return 2\n\n    done()\n",
        ),
        (
            "blank-line-before-assignment",
            "def run():\n    log(); value = 1\n",
            "def run():\n    log()\n\n    value = 1\n",
        ),
    ] {
        let directory = tempfile::tempdir_in(&base).unwrap();
        std::fs::write(
            directory.path().join("pyproject.toml"),
            format!("[tool.ruff.lint]\nselect = []\n[tool.tuff.lint]\nenable = ['{rule}']\n"),
        )
        .unwrap();
        let path = directory.path().join("input.py");
        std::fs::write(&path, source).unwrap();
        let first = std::process::Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["format", "--no-cache"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            first.status.success(),
            "{source}: {}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            expected,
            "{rule}: {source}"
        );
        let checked = std::process::Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["check", "--no-cache", "--output-format", "json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(
            checked.status.code(),
            Some(0),
            "{rule}: {}",
            String::from_utf8_lossy(&checked.stdout)
        );
        let second = std::process::Command::new(env!("CARGO_BIN_EXE_tuff"))
            .args(["format", "--no-cache"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(second.status.success());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            expected,
            "second format: {source}"
        );
    }
}

#[test]
fn footer_cleanup_preserves_comment_line_endings() {
    for ending in ["\n", "\r\n", "\r"] {
        let clean = [
            "def run(flag):",
            "    if flag:",
            "        work()",
            "        # footer",
            "    done()",
            "",
        ]
        .join(ending);
        common::assert_clean("no-suite-leading-trailing-blank-lines", &clean, "");
        let source = [
            "def run(flag):",
            "    if flag:",
            "        work()",
            "        # footer",
            "        ",
            "    done()",
            "",
        ]
        .join(ending);
        common::assert_fix("no-suite-leading-trailing-blank-lines", &source, "", &clean);
    }
}

#[test]
fn semicolon_relationships_follow_rendered_lines() {
    common::assert_fix(
        "blank-line-after-control-block",
        "if flag:\n    x = 1\nlog(); consume(x)\n",
        "",
        "if flag:\n    x = 1\n\nlog(); consume(x)\n",
    );
    common::assert_fix(
        "blank-line-after-control-block",
        "with manager() as value:\n    pass\nlog(); consume(value)\n",
        "",
        "with manager() as value:\n    pass\n\nlog(); consume(value)\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "if flag:\n    log(\n    ); noop()\n    x = 1\n    y = 2\n",
        "",
        "if flag:\n    log(\n    ); noop()\n\n    x = 1\n    y = 2\n",
    );
    common::assert_clean(
        "block-header-cuddle-strict",
        "x = (\n    1\n); y = 2\nif y:\n    pass\n",
        "",
    );
}

#[test]
fn same_subject_guards_remain_related_when_ladders_are_disabled() {
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(value):\n    if value == 1:\n        return 1\n    if value == 2:\n        return 2\n",
        "allow_compact_guard_ladders = false",
    );
}

#[test]
fn branch_thresholds_use_formatted_line_counts() {
    let options = "allow_related_return_tails = false";
    let source = "def f():\n    result = compute(first_argument, second_argument, third_argument, fourth_argument, fifth_argument)\n    return result\n";
    assert_eq!(
        common::check("blank-line-before-branch", source, options)
            .as_array()
            .unwrap()
            .len(),
        1
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f():\n    result = (\n        1\n    )\n    return result\n",
        options,
    );
}

#[test]
fn after_control_and_comment_separators() {
    common::assert_fix(
        "blank-line-after-control-block",
        "def f(x):\n    if x:\n        x += 1\n    return x\n",
        "",
        "def f(x):\n    if x:\n        x += 1\n\n    return x\n",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(x):\n    if x:\n        x += 1\n    # separator\n    return x\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(x):\n    if x: return x\n    return 0\n",
        "",
    );
}

#[test]
fn terminal_alternatives_and_primary_continue() {
    common::assert_fix(
        "blank-line-after-terminal-control-block",
        "def f(x):\n    if x:\n        work()\n    else:\n        return\n    followup()\n",
        "",
        "def f(x):\n    if x:\n        work()\n    else:\n        return\n\n    followup()\n",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(xs):\n    for x in xs:\n        if x:\n            clean()\n            continue\n        consume(x)\n",
        "",
    );
    common::assert_fix(
        "blank-line-after-terminal-control-block",
        "def f():\n    try:\n        return\n    except* ValueError:\n        recover()\n    work()\n",
        "",
        "def f():\n    try:\n        return\n    except* ValueError:\n        recover()\n\n    work()\n",
    );
}

#[test]
fn compact_guards_are_contiguous() {
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def f(a, b):\n    if a:\n        return 1\n    if b:\n        return 2\n    return 3\n",
        "",
    );
    common::assert_fix(
        "blank-line-after-terminal-control-block",
        "def f(a, b):\n    if a:\n        return 1\n\n    work()\n    if b:\n        return 2\n    return 3\n",
        "",
        "def f(a, b):\n    if a:\n        return 1\n\n    work()\n    if b:\n        return 2\n\n    return 3\n",
    );
    let d = common::check(
        "blank-line-after-control-block",
        "def f(a, b):\n    if a:\n        return 1\n    if b:\n        return 2\n    return 3\n",
        "allow_compact_guard_ladders = false",
    );
    assert_eq!(d.as_array().unwrap().len(), 2);
}

#[test]
fn assignment_capture_resolves_shadowing() {
    common::assert_clean(
        "blank-line-before-assignment",
        "def f():\n    log()\n    payload = make()\n    def helper():\n        return payload\n",
        "",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f():\n    log()\n    payload = make()\n    def helper(payload):\n        return payload\n",
        "",
        "def f():\n    log()\n\n    payload = make()\n    def helper(payload):\n        return payload\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f():\n    log()\n    payload = make()\n    def helper():\n        payload = other()\n        return payload\n",
        "",
        "def f():\n    log()\n\n    payload = make()\n    def helper():\n        payload = other()\n        return payload\n",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f():\n    log()\n    payload = make()\n    class Helper:\n        def get(self):\n            return payload\n",
        "",
    );
}

#[test]
fn receiver_setup_removes_only_pure_blanks() {
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(logger, handler):\n    logger.addHandler(handler)\n\n    logger.propagate = False\n",
        "",
        "def f(logger, handler):\n    logger.addHandler(handler)\n    logger.propagate = False\n",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(logger, handler):\n    logger.addHandler(handler)\n\n    # keep\n    logger.propagate = False\n",
        "",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f():\n    log()\n\n    result = compute()\n    return result\n",
        "",
        "def f():\n    log()\n    result = compute()\n    return result\n",
    );
}

#[test]
fn short_alternative_suites_and_cleanup() {
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(x):\n    if x:\n        pass\n    else:\n        log()\n        result = make()\n",
        "short_control_flow_max_statements = 2",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f():\n    try:\n        work()\n    except Exception:\n        clean()\n        audit()\n        flush()\n        state = None\n        raise\n",
        "",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(x):\n    if x:\n        return\n    result = make()\n    return result\n",
        "",
        "def f(x):\n    if x:\n        return\n\n    result = make()\n    return result\n",
    );
}

#[test]
fn annotated_return_tails_remove_extra_separation() {
    common::assert_fix(
        "blank-line-before-branch",
        "def f():\n    a = 1\n    b = 2\n    result: int = a + b\n\n    return result\n",
        "",
        "def f():\n    a = 1\n    b = 2\n    result: int = a + b\n    return result\n",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f():\n    a = 1\n    return a\n",
        "",
    );
}

#[test]
fn headers_distinguish_relaxed_and_strict() {
    let source =
        "def f(flag):\n    prepared = compute()\n    if flag:\n        consume(prepared)\n";
    common::assert_clean("blank-line-before-unrelated-block", source, "");
    common::assert_fix(
        "block-header-cuddle-strict",
        source,
        "",
        "def f(flag):\n    prepared = compute()\n\n    if flag:\n        consume(prepared)\n",
    );
    common::assert_fix(
        "blank-line-before-unrelated-block",
        source,
        "body_usage_lookahead = 0",
        "def f(flag):\n    prepared = compute()\n\n    if flag:\n        consume(prepared)\n",
    );
}

#[test]
fn header_binding_and_receiver_relationships() {
    common::assert_fix(
        "blank-line-before-unrelated-block",
        "def f(items):\n    item = default()\n    for item in items:\n        consume(item)\n",
        "",
        "def f(items):\n    item = default()\n\n    for item in items:\n        consume(item)\n",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(session):\n    session.refresh()\n    if session.stale:\n        reset(session)\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(session):\n    session.ready = compute()\n    if session.ready:\n        start()\n",
        "",
    );
}

#[test]
fn with_inspection_and_cluster_options() {
    let source = "def f(path):\n    with open(path) as handle:\n        content = handle.read()\n    assert content\n";
    common::assert_clean("blank-line-after-control-block", source, "");
    common::assert_fix(
        "blank-line-after-control-block",
        source,
        "allow_with_immediate_inspection = false",
        "def f(path):\n    with open(path) as handle:\n        content = handle.read()\n\n    assert content\n",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def f():\n    try:\n        return\n    except KeyError:\n        pass\n    work()\n",
        "",
    );
}

#[test]
fn match_case_threshold_and_comments() {
    common::assert_fix(
        "match-case-separation",
        "match value:\n    case 1:\n        a = 1\n        b = 2\n        c = 3\n    case _:\n        pass\n",
        "",
        "match value:\n    case 1:\n        a = 1\n        b = 2\n        c = 3\n\n    case _:\n        pass\n",
    );
    common::assert_clean(
        "match-case-separation",
        "match value:\n    case 1:\n        a = 1\n        b = 2\n        c = 3\n    # separator\n    case _:\n        pass\n",
        "",
    );
}

#[test]
fn suite_boundaries_preserve_nested_definition_exception() {
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "def f():\n\n    work()\n\n\n",
        "",
        "def f():\n    work()\n",
    );
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "def f():\n\n\n    def inner():\n        work()\n",
        "",
        "def f():\n\n    def inner():\n        work()\n",
    );
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "class C:\n\n    def f(self):\n        work()\n",
        "",
        "class C:\n    def f(self):\n        work()\n",
    );
    common::assert_clean(
        "no-suite-leading-trailing-blank-lines",
        "def f():\n    # comment\n\n    work()\n",
        "",
    );
}

#[test]
fn original_behavioral_fixtures() {
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(value: int) -> int:\n    if value > 0:\n        value += 1\n\n    return value\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(value: int) -> int:\n    if value > 0:\n        value += 1\n    # comment separator\n    return value\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(value: int) -> int:\n    if value > 0: return value\n    return 0\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def load_config(text: str, format_name: str) -> object:\n    if format_name == \"json\":\n        return json.loads(text)\n    if format_name == \"toml\":\n        return tomllib.loads(text)\n    if format_name == \"yaml\":\n        return _load_yaml_text(text)\n\n    raise ValueError(format_name)\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def normalize(parts: list[str], values: list[str]) -> None:\n    for part in values:\n        if part == \"..\":\n            parts.pop()\n            continue\n        parts.append(part)\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def render(parser: object, capsys: object) -> object:\n    try:\n        parser.run()\n    except SystemExit:\n        pass\n    out = capsys.readouterr()\n    return out\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f() -> None:\n    with pytest.raises(ValueError):\n        parse(\"x\")\n    with pytest.raises(TypeError):\n        parse(3)\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(path: str) -> None:\n    with open(path) as handle:\n        content = handle.read()\n    assert content\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(width: int | None, columns: list[str]) -> list[str]:\n    if width is not None:\n        template = f\"{width:02d}\"\n        columns.append(template)\n    columns.append(template if width is not None else \"default\")\n    return columns\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(flag: bool, label: str) -> str:\n    if not flag:\n        return label\n    cleaned = label.strip()\n    return cleaned\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(shell_name: str, interactive: bool) -> list[str]:\n    if shell_name == \"zsh\":\n        return [\"-lic\"]\n    if interactive:\n        return [\"-ic\"]\n    return [\"-lc\"]\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(primary: str | None, fallback: str | None) -> str:\n    if primary is not None:\n        return primary\n    if fallback is not None:\n        return fallback\n    return \"guest\"\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(value: int, other: int) -> int:\n    if value > 0:\n        log(value)\n        audit(value)\n        return value\n    if other > 0:\n        return other\n    return 0\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-control-block",
        "def f(flag: bool, label: str) -> str:\n    if not flag:\n        return label\n    return label.strip()\n",
        "",
    );
    common::assert_fix(
        "blank-line-after-control-block",
        "def f(value: int) -> int:\n    if value > 0:\n        value += 1\n    return value\n",
        "",
        "def f(value: int) -> int:\n    if value > 0:\n        value += 1\n\n    return value\n",
    );
    common::assert_fix(
        "blank-line-after-control-block",
        "def f(values: list[int]) -> int:\n    total = 0\n    for value in values:\n        total += value\n    return total\n",
        "",
        "def f(values: list[int]) -> int:\n    total = 0\n    for value in values:\n        total += value\n\n    return total\n",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def normalize(value: str | None) -> str:\n    if value is None:\n        return \"\"\n\n    cleaned = value.strip()\n    return cleaned\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def shell_args(shell_name: str, interactive: bool) -> list[str]:\n    if shell_name == \"zsh\":\n        return [\"-lic\"]\n    if interactive:\n        return [\"-ic\"]\n    return [\"-lc\"]\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def consume(name: str) -> str:\n    parts: list[str] = []\n    index = 0\n    while index < len(name):\n        ch = name[index]\n        if ch in {\"'\", '\"'}:\n            end = _consume_quoted_segment(name, index)\n            parts.append(name[index:end])\n            index = end\n            continue\n\n        parts.append(ch)\n        index += 1\n\n    return \"\".join(parts)\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def collect(values: list[int]) -> list[int]:\n    result: list[int] = []\n    for value in values:\n        if value < 0:\n            continue\n\n        result.append(value)\n\n    return result\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def render(parser: object, capsys: object) -> object:\n    try:\n        parser.run()\n    except SystemExit:\n        pass\n    out = capsys.readouterr()\n    return out\n",
        "",
    );
    common::assert_clean(
        "blank-line-after-terminal-control-block",
        "def get_value(cache: dict[str, str], key: str) -> str:\n    try:\n        return cache[key]\n    except KeyError:\n        pass\n    value = compute()\n    return value\n",
        "",
    );
    common::assert_fix(
        "blank-line-after-terminal-control-block",
        "def normalize(value: str | None) -> str:\n    if value is None:\n        return \"\"\n    cleaned = value.strip()\n    return cleaned\n",
        "",
        "def normalize(value: str | None) -> str:\n    if value is None:\n        return \"\"\n\n    cleaned = value.strip()\n    return cleaned\n",
    );
    common::assert_fix(
        "blank-line-after-terminal-control-block",
        "def collect(values: list[int]) -> list[int]:\n    result: list[int] = []\n    for value in values:\n        if value < 0:\n            continue\n        result.append(value)\n\n    return result\n",
        "",
        "def collect(values: list[int]) -> list[int]:\n    result: list[int] = []\n    for value in values:\n        if value < 0:\n            continue\n\n        result.append(value)\n\n    return result\n",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> int:\n    value = 1\n    other = value + 1\n    return other\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> int:\n    log_start()\n\n    value = compute()\n    log_value(value)\n    return value\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> int:\n    total = 0\n    total += 1\n    return total\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> int:\n    \"\"\"Compute value.\"\"\"\n    value = compute()\n    return value\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(backend: object, archiver: object, writer: object) -> None:\n    if needs_status:\n        log_status(backend=backend, archiver=archiver, writer=writer)\n        last_status_time = loop.time()\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> None:\n    if needs_status:\n        log_status()\n        update_metrics()\n        last_status_time = loop.time()\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> None:\n    if needs_status:\n        log_status()\n        update_metrics()\n        refresh_cache()\n        last_status_time = loop.time()\n",
        "short_control_flow_max_statements = 4",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "async def f() -> None:\n    try:\n        work()\n    except Exception:\n        cleanup_a()\n        cleanup_b()\n        await cleanup_c()\n        collector_id = None\n        raise\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "async def f() -> None:\n    try:\n        work()\n    except Exception:\n        cleanup()\n        state = None\n        log_error()\n        raise\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(output: object) -> None:\n    output.write(\"ok\")\n    bar = output.bars[\"task\"]\n    assert bar.n == 1\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> None:\n    assert output.exists()\n    payload = json.loads(output.read_text())\n    assert \"themes\" in payload\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(name: str | None) -> object:\n    configure_logging()\n    logger_name = \"default\" if name is None else name\n    return make_logger(logger_name)\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(candidate: object, parser_input: str, style: object) -> object:\n    validate(candidate)\n    display_value = parser_input or str(candidate)\n    if supports_live_interaction():\n        highlight(display_value, style)\n    else:\n        summarize(display_value, style)\n    return candidate\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(monkeypatch: object) -> dict[str, object]:\n    monkeypatch.setenv(\"TOKEN\", \"abc\")\n    calls: dict[str, object] = {}\n    class FakeRepo:\n        def __init__(self) -> None:\n            calls[\"created\"] = True\n    return calls\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f(logger: logging.Logger, handler: logging.Handler) -> None:\n    logger.addHandler(handler)\n    logger.propagate = False\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-assignment",
        "def f() -> int:\n    log_start()\n    value = compute()\n    return value\n",
        "",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(values: list[int]) -> int:\n    total = 0\n    if values:\n        total += len(values)\n    total += 1\n    return total\n",
        "",
        "def f(values: list[int]) -> int:\n    total = 0\n    if values:\n        total += len(values)\n\n    total += 1\n    return total\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(flag: bool, value: str) -> str:\n    if not flag:\n        return value\n    normalized = value.strip()\n    return normalized\n",
        "",
        "def f(flag: bool, value: str) -> str:\n    if not flag:\n        return value\n\n    normalized = value.strip()\n    return normalized\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(value: int) -> int:\n    if value > 0:\n        log_status(value)\n        update_metrics(value)\n        adjusted = value + 1\n        return adjusted\n\n    return value\n",
        "",
        "def f(value: int) -> int:\n    if value > 0:\n        log_status(value)\n        update_metrics(value)\n\n        adjusted = value + 1\n        return adjusted\n\n    return value\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f() -> None:\n    if needs_status:\n        log_status()\n        last_status_time = loop.time()\n",
        "short_control_flow_max_statements = 1",
        "def f() -> None:\n    if needs_status:\n        log_status()\n\n        last_status_time = loop.time()\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(candidate: object) -> object:\n    validate(candidate)\n    display_value = str(candidate)\n    if supports_live_interaction():\n        highlight(candidate)\n    return candidate\n",
        "",
        "def f(candidate: object) -> object:\n    validate(candidate)\n\n    display_value = str(candidate)\n    if supports_live_interaction():\n        highlight(candidate)\n    return candidate\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f(logger: logging.Logger, handler: logging.Handler) -> None:\n    logger.addHandler(handler)\n\n    logger.propagate = False\n",
        "",
        "def f(logger: logging.Logger, handler: logging.Handler) -> None:\n    logger.addHandler(handler)\n    logger.propagate = False\n",
    );
    common::assert_fix(
        "blank-line-before-assignment",
        "def f() -> int:\n    log_start()\n\n    value = compute()\n    return value\n",
        "",
        "def f() -> int:\n    log_start()\n    value = compute()\n    return value\n",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(value: int) -> int:\n    x = value + 1\n    y = x + 1\n\n    return y\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(value: int) -> int:\n    x = value + 1\n    return x\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(parts: list[str]) -> dict[str, int]:\n    cleaned = [part.strip() for part in parts]\n    joined = \",\".join(cleaned)\n    payload: dict[str, int] = {\"count\": len(cleaned), \"width\": len(joined)}\n    return payload\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(value: int) -> int:\n    x = value + 1\n    y = x + 1\n    z = y + 1\n    # comment separator\n    return z\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f() -> int:\n    \"\"\"Return constant.\"\"\"\n    return 1\n    value = 2\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(value: int) -> int:\n    x = value + 1\n    y = x + 1\n    return y\n",
        "max_suite_non_empty_lines = 3",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "async def f() -> None:\n    try:\n        work()\n    except Exception:\n        cleanup_a()\n        cleanup_b()\n        await cleanup_c()\n        collector_id = None\n        raise\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "async def f() -> None:\n    try:\n        work()\n    except Exception:\n        cleanup()\n        state = None\n        log_error()\n        raise\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "async def f() -> None:\n    try:\n        work()\n    finally:\n        cleanup()\n        log_teardown()\n        return\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(created_at: object) -> object:\n    payload = {\"created_at\": created_at}\n    return ArchivedPost(created_at=created_at, payload=payload)\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(shell_name: str, interactive: bool) -> list[str]:\n    if shell_name == \"zsh\":\n        return [\"-lic\"]\n    if interactive:\n        return [\"-ic\"]\n    return [\"-lc\"]\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-branch",
        "def f(values: list[int]) -> int:\n    total = 0\n    for value in values:\n        total += value\n    return total\n",
        "",
    );
    common::assert_fix(
        "blank-line-before-branch",
        "def f(value: int) -> int:\n    x = value + 1\n    y = x + 1\n    z = y + 1\n    return z\n",
        "",
        "def f(value: int) -> int:\n    x = value + 1\n    y = x + 1\n    z = y + 1\n\n    return z\n",
    );
    common::assert_fix(
        "blank-line-before-branch",
        "def f(values: list[int]) -> int:\n    total = 0\n    message = str(total)\n    flag = bool(message)\n    raise RuntimeError(\"boom\")\n",
        "",
        "def f(values: list[int]) -> int:\n    total = 0\n    message = str(total)\n    flag = bool(message)\n\n    raise RuntimeError(\"boom\")\n",
    );
    common::assert_fix(
        "blank-line-before-branch",
        "def f(value: int) -> int:\n    x = value + 1\n    return x\n",
        "allow_related_return_tails = false\nmax_suite_non_empty_lines = 1",
        "def f(value: int) -> int:\n    x = value + 1\n\n    return x\n",
    );
    common::assert_fix(
        "blank-line-before-branch",
        "def f(parts: list[str]) -> dict[str, int]:\n    cleaned = [part.strip() for part in parts]\n    joined = \",\".join(cleaned)\n    payload: dict[str, int] = {\"count\": len(cleaned), \"width\": len(joined)}\n\n    return payload\n",
        "",
        "def f(parts: list[str]) -> dict[str, int]:\n    cleaned = [part.strip() for part in parts]\n    joined = \",\".join(cleaned)\n    payload: dict[str, int] = {\"count\": len(cleaned), \"width\": len(joined)}\n    return payload\n",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    prepared = value + 1\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    prepared = value + 1\n    if value > 0:\n        result = prepared\n        return result\n\n    return 0\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    log_start()\n    prepared = value + 1\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    prepared = value + 1\n    not_used = value + 2\n\n    if value > 0:\n        return value\n\n    return prepared\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    \"\"\"Compute value.\"\"\"\n    if value > 0:\n        return value\n\n    return 0\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(base: dict[str, object], override: dict[str, object]) -> dict[str, object]:\n    merged = dict(base)\n    for key, value in override.items():\n        if key not in merged:\n            merged[key] = value\n            continue\n        merged[key] = value\n\n    return merged\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(directory: str) -> None:\n    queue = Queue()\n    queue.put(directory)\n    while not queue.empty():\n        item = queue.get()\n        visit(item)\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(shell_name: str) -> list[str]:\n    interactive = shell_name == \"zsh\"\n    if shell_name == \"zsh\":\n        return [\"-lic\"]\n    if interactive:\n        return [\"-ic\"]\n    return [\"-lc\"]\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(candidate: object, parser_input: str, style: object) -> object:\n    display_value = parser_input or str(candidate)\n    if supports_live_interaction():\n        highlight(display_value, style)\n    else:\n        summarize(display_value, style)\n    return candidate\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(override_name: str | None) -> str:\n    display_name = \"guest\"\n    if override_name is not None:\n        display_name = override_name\n    return display_name\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f(default_value: object) -> dict[str, object]:\n    prompt_kwargs: dict[str, object] = {}\n    if default_value:\n        prompt_kwargs[\"placeholder\"] = str(default_value)\n    return prompt_kwargs\n",
        "",
    );
    common::assert_clean(
        "blank-line-before-unrelated-block",
        "def f() -> None:\n    session = build_session()\n    session.refresh()\n    if session.is_stale():\n        reset_session(session)\n        return\n    cleanup()\n",
        "",
    );
    common::assert_fix(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    prepared = value + 1\n    if value > 0:\n        return value\n\n    return 0\n",
        "",
        "def f(value: int) -> int:\n    prepared = value + 1\n\n    if value > 0:\n        return value\n\n    return 0\n",
    );
    common::assert_fix(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    prepared = value + 1\n    log(prepared)\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
        "",
        "def f(value: int) -> int:\n    prepared = value + 1\n    log(prepared)\n\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
    );
    common::assert_fix(
        "blank-line-before-unrelated-block",
        "def f(value: int) -> int:\n    prepared = value + 1\n    trailing = value + 2\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
        "",
        "def f(value: int) -> int:\n    prepared = value + 1\n    trailing = value + 2\n\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
    );
    common::assert_fix(
        "blank-line-before-unrelated-block",
        "def f(base: dict[str, object], override: dict[str, object]) -> dict[str, object]:\n    merged = dict(base)\n    for key, value in override.items():\n        log_key(key)\n        audit(value)\n        publish(key, value)\n        notify_team(key)\n        merged[key] = value\n\n    return merged\n",
        "",
        "def f(base: dict[str, object], override: dict[str, object]) -> dict[str, object]:\n    merged = dict(base)\n\n    for key, value in override.items():\n        log_key(key)\n        audit(value)\n        publish(key, value)\n        notify_team(key)\n        merged[key] = value\n\n    return merged\n",
    );
    common::assert_clean(
        "block-header-cuddle-strict",
        "def f(value: int) -> int:\n    prepared = value + 1\n    if prepared > 0:\n        return prepared\n\n    return 0\n",
        "",
    );
    common::assert_clean(
        "block-header-cuddle-strict",
        "def f(value: int) -> int:\n    prepared = value + 1\n\n    if value > 0:\n        return value\n\n    return 0\n",
        "",
    );
    common::assert_clean(
        "block-header-cuddle-strict",
        "def f(value: int) -> int:\n    \"\"\"Compute value.\"\"\"\n    if value > 0:\n        return value\n\n    return 0\n",
        "",
    );
    common::assert_fix(
        "block-header-cuddle-strict",
        "def f(value: int) -> int:\n    header_value = value + 1\n    trailing = value + 2\n    if header_value > 0:\n        return header_value\n\n    return 0\n",
        "",
        "def f(value: int) -> int:\n    header_value = value + 1\n    trailing = value + 2\n\n    if header_value > 0:\n        return header_value\n\n    return 0\n",
    );
    common::assert_fix(
        "block-header-cuddle-strict",
        "def f(value: int) -> int:\n    prepared = value + 1\n    if value > 0:\n        result = prepared\n        return result\n\n    return 0\n",
        "",
        "def f(value: int) -> int:\n    prepared = value + 1\n\n    if value > 0:\n        result = prepared\n        return result\n\n    return 0\n",
    );
    common::assert_clean(
        "match-case-separation",
        "def f(value: int) -> int:\n    match value:\n        case 1:\n            return 1\n        case _:\n            return 0\n",
        "",
    );
    common::assert_clean(
        "match-case-separation",
        "def f(value: int) -> int:\n    match value:\n        case 1:\n            a = 1\n            b = 2\n            c = 3\n\n        case _:\n            return 0\n",
        "",
    );
    common::assert_clean(
        "match-case-separation",
        "def f(value: int) -> int:\n    match value:\n        case 1:\n            a = 1\n            b = 2\n            c = 3\n        case _:\n            return 0\n",
        "max_case_non_empty_lines = 3",
    );
    assert!(!common::check("match-case-separation", "def f(value: int) -> int:\n    match value:\n        case 1:\n            a = 1\n            b = 2\n            c = 3\n        case _:\n            return 0\n", "").as_array().unwrap().is_empty());
    assert!(!common::check("match-case-separation", "def f(value: int) -> int:\n    match value:\n        case 1:\n            first = 1\n            second = 2\n            third = 3\n        case 2:\n            return 2\n        case _:\n            return 0\n", "").as_array().unwrap().is_empty());
    assert!(!common::check("match-case-separation", "def f(value: int) -> int:\n    match value:\n        case 1:\n            a = 1\n            b = 2\n        case _:\n            return 0\n", "max_case_non_empty_lines = 1").as_array().unwrap().is_empty());
    common::assert_clean(
        "no-suite-leading-trailing-blank-lines",
        "def f() -> int:\n    value = 1\n    return value\n",
        "",
    );
    common::assert_clean(
        "no-suite-leading-trailing-blank-lines",
        "def f() -> int:\n    # comment lines are separators, not blank lines\n    value = 1\n    return value\n",
        "",
    );
    common::assert_clean(
        "no-suite-leading-trailing-blank-lines",
        "def f(items: list[int]) -> None:\n\n    def emit() -> None:\n        print(items)\n",
        "",
    );
    common::assert_clean(
        "no-suite-leading-trailing-blank-lines",
        "def f(items: list[int]) -> None:\n    for item in items:\n\n        @track(item)\n        def emit() -> None:\n            print(item)\n",
        "",
    );
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "def f() -> int:\n\n    value = 1\n    return value\n",
        "",
        "def f() -> int:\n    value = 1\n    return value\n",
    );
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "def f(items: list[int]) -> None:\n\n\n    def emit() -> None:\n        print(items)\n",
        "",
        "def f(items: list[int]) -> None:\n\n    def emit() -> None:\n        print(items)\n",
    );
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "def f(items: list[int]) -> None:\n    for item in items:\n\n\n        @track(item)\n        def emit() -> None:\n            print(item)\n",
        "",
        "def f(items: list[int]) -> None:\n    for item in items:\n\n        @track(item)\n        def emit() -> None:\n            print(item)\n",
    );
    common::assert_fix(
        "no-suite-leading-trailing-blank-lines",
        "def f() -> int:\n    value = 1\n    return value\n\n",
        "",
        "def f() -> int:\n    value = 1\n    return value\n",
    );
}
