#[path = "tuff/mod.rs"]
mod common;

#[test]
fn self_annotation_fixes_preserve_eager_evaluation() {
    for annotation in ["annotate()", "module.Type", "First | Second", "{[]}"] {
        let source = format!("class Container:\n    def run(self: {annotation}): pass\n");
        common::assert_unsafe_fix("no-annotated-self", &source);
    }
    let reports = common::check(
        "no-annotated-self",
        "class Container:\n    def run(self: 'Container'): pass\n",
        "",
    );
    assert_eq!(reports[0]["fix"]["applicability"], "safe");
    let reports = common::check(
        "no-annotated-self",
        "from __future__ import annotations\nclass Container:\n    def run(self: annotate()): pass\n",
        "",
    );
    assert_eq!(reports[0]["fix"]["applicability"], "safe");
}

#[test]
fn message_inlining_preserves_evaluated_annotations() {
    for source in [
        "message: annotate() = 'bad'\nraise ValueError(message)\n",
        "class Container:\n    message: annotate() = 'bad'\n    raise ValueError(message)\n",
        "message = {[]}\nraise ValueError(message)\n",
    ] {
        common::assert_unsafe_fix("no-exception-message-variables", source);
    }
    let reports = common::check(
        "no-exception-message-variables",
        "def run():\n    message: annotate() = 'bad'\n    raise ValueError(message)\n",
        "",
    );
    assert_eq!(reports[0]["fix"]["applicability"], "safe");
}

#[test]
fn self_annotations_and_aliases() {
    common::assert_fix(
        "no-annotated-self",
        "class Item:\n    def run(self: 'Item'): pass\n",
        "",
        "class Item:\n    def run(self): pass\n",
    );
    for source in [
        "from builtins import staticmethod as sm\nclass Item:\n    @sm\n    def run(self: int): pass\n",
        "class Item:\n    def __new__(self: object): pass\n",
        "class Item:\n    def run(self: T) -> T: pass\n",
        "class Item:\n    def run(self: list[int]): pass\n",
    ] {
        common::assert_clean("no-annotated-self", source, "");
    }
}

#[test]
fn class_names_and_method_order() {
    assert_eq!(
        common::check(
            "no-underscore-class",
            "def factory():\n    class _Hidden: pass\n",
            ""
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        common::check(
            "public-method-order",
            "class Service:\n    def _helper(self): pass\n    def public(self): pass\n",
            ""
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    for source in [
        "from dataclasses import dataclass as dc\n@dc\nclass Service:\n    def _helper(self): pass\n    def public(self): pass\n",
        "from typing import overload as ov\nclass Service:\n    @ov\n    def public(self, x: int): ...\n    def _helper(self): pass\n    def public(self, x): pass\n",
        "class Service:\n    @property\n    def value(self): return 1\n    def _helper(self): pass\n    @value.setter\n    def value(self, v): pass\n",
        "from functools import singledispatchmethod as sd\nclass Service:\n    def _helper(self): pass\n    @sd\n    def public(self): pass\n",
    ] {
        common::assert_clean("public-method-order", source, "");
    }
    common::assert_clean(
        "public-method-order",
        "class Service:\n    def _helper(self): pass\n    def public(self): pass\n",
        "class_name_patterns = ['*Controller']",
    );
}

#[test]
fn sorting_preserves_comments_groups_and_annotations() {
    common::assert_fix(
        "sorted-attributes",
        "class Values:\n    '''@sorted-attributes'''\n    z = 1\n    # about a\n    a = 2\n\n    y: int\n    b: int\n",
        "",
        "class Values:\n    '''@sorted-attributes'''\n    # about a\n    a = 2\n    z = 1\n\n    b: int\n    y: int\n",
    );
    common::assert_clean(
        "sorted-attributes",
        "class Values:\n    '''@sorted-attributes'''\n    z = 1\n\n    a = 2\n",
        "",
    );
    let reports = common::check(
        "sorted-attributes",
        "class Values:\n    '''@sorted-attributes'''\n    z = 1\n    a = z\n",
        "",
    );
    assert_eq!(reports.as_array().unwrap().len(), 1);
    assert!(reports[0]["fix"].is_null());
    for source in [
        "class Values:\n    '''@sorted-attributes'''\n    z = int\n    a: z = 1\n",
        "class Values:\n    '''@sorted-attributes'''\n    z: record('first') = 1\n    a: record('second') = 2\n",
    ] {
        let reports = common::check("sorted-attributes", source, "");
        assert_eq!(reports.as_array().unwrap().len(), 1);
        assert!(reports[0]["fix"].is_null());
    }
}

#[test]
fn exception_messages_and_scope() {
    common::assert_fix(
        "no-exception-message-variables",
        "message = 'bad'\n# explanation\nraise ValueError(message)\n",
        "",
        "# explanation\nraise ValueError('bad')\n",
    );
    common::assert_clean(
        "no-exception-message-variables",
        "try:\n    message = 'bad'\n    raise ValueError(message)\nexcept ValueError:\n    log(message)\n",
        "",
    );
    common::assert_clean(
        "no-exception-message-variables",
        "def fail():\n    global message\n    message = 'bad'\n    raise ValueError(message)\n",
        "",
    );
    assert_eq!(
        common::check(
            "no-exception-message-variables",
            "message = 'old'\nlog(message)\nmessage = build()\nraise ValueError(message)\n",
            ""
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn exception_messages_keep_deletions_and_quoted_references() {
    common::assert_clean(
        "no-exception-message-variables",
        "try:\n    message = 'bad'\n    raise ValueError(message)\nfinally:\n    del message\n",
        "",
    );
    common::assert_clean(
        "no-exception-message-variables",
        "try:\n    message = 'bad'\n    raise ValueError(message)\nexcept ValueError:\n    value: 'message'\n",
        "",
    );
}

#[test]
fn sorting_and_message_fixes_preserve_crlf() {
    common::assert_fix(
        "sorted-attributes",
        "class Values:\r\n    '''@sorted-attributes'''\r\n    z = 1\r\n    a = 2\r\n",
        "",
        "class Values:\r\n    '''@sorted-attributes'''\r\n    a = 2\r\n    z = 1\r\n",
    );
    common::assert_fix(
        "no-exception-message-variables",
        "def fail():\r\n    log()\r\n    message = 'bad'\r\n    raise ValueError(message)\r\n",
        "",
        "def fail():\r\n    log()\r\n    raise ValueError('bad')\r\n",
    );
}

#[test]
fn exception_translation() {
    for expression in [
        "str(alias)",
        "f'{alias}'",
        "'{}'.format(alias)",
        "'%s' % (alias,)",
        "(f'' '{}').format(alias)",
        "(f'' '%s') % alias",
    ] {
        let source = format!(
            "try:\n    run()\nexcept ValueError as exc:\n    alias = exc\n    raise RuntimeError({expression}) from exc\n"
        );
        assert_eq!(
            common::check("no-str-exception-translation", &source, "")
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    for source in [
        "try:\n    run()\nexcept ValueError as exc:\n    exc: str = 'fixed'\n    raise RuntimeError(str(exc))\n",
        "try:\n    run()\nexcept ValueError as exc:\n    def nested():\n        raise RuntimeError(str(exc))\n",
        "try:\n    run()\nexcept ValueError as exc:\n    raise RuntimeError(f'context: {exc}')\n",
    ] {
        common::assert_clean("no-str-exception-translation", source, "");
    }
}

#[test]
fn exception_translation_reports_the_first_source_argument() {
    let reports = common::check(
        "no-str-exception-translation",
        "try:\n    run()\nexcept Exception as exc:\n    raise Error(detail=str(exc), *str(exc))\n",
        "",
    );
    assert_eq!(reports.as_array().unwrap().len(), 1);
    assert_eq!(reports[0]["location"]["row"], 4);
    assert_eq!(reports[0]["location"]["column"], 24);
}

#[test]
fn nested_sorting_and_parenthesized_messages() {
    common::assert_fix(
        "sorted-attributes",
        "class Outer:\n    '''@sorted-attributes'''\n    z = 1\n    a = 2\n    class Inner:\n        '''@sorted-attributes'''\n        z = 1\n        a = 2",
        "",
        "class Outer:\n    '''@sorted-attributes'''\n    a = 2\n    z = 1\n    class Inner:\n        '''@sorted-attributes'''\n        a = 2\n        z = 1",
    );
    common::assert_fix(
        "no-exception-message-variables",
        "def fail():\n    detail = ('bad')\n    raise ValueError(detail)\n",
        "",
        "def fail():\n    raise ValueError(('bad'))\n",
    );
    common::assert_clean(
        "no-exception-message-variables",
        "def fail():\n    detail = 'bad'\n    raise ValueError(detail)\n    def capture(value=detail): pass\n",
        "",
    );
}

#[test]
fn translation_builtin_alias_and_shadowing() {
    assert_eq!(common::check("no-str-exception-translation", "from builtins import str as stringify\ntry:\n    run()\nexcept ValueError as exc:\n    raise RuntimeError(stringify(exc)) from exc\n", "").as_array().unwrap().len(), 1);
    common::assert_clean(
        "no-str-exception-translation",
        "try:\n    run()\nexcept ValueError as exc:\n    str = lambda value: 'fixed'\n    raise RuntimeError(str(exc))\n",
        "",
    );
}

fn count(rule: &str, source: &str, options: &str) -> usize {
    common::check(rule, source, options)
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn deleted_arguments_are_reported_only_when_otherwise_unused() {
    for source in [
        "async def down(self, migrator):\n    del migrator\n    return None\n",
        "def serialize(key, value):\n    del key\n    return serialize(key='item', value=value)\n",
        "def callback(value):\n    del value\n",
        "def collision(value, _value):\n    del value\n    return _value\n",
        "def unpack(first, second):\n    del (first, [second])\n",
    ] {
        let reports = common::check("no-del-unused-argument", source, "");
        assert_eq!(reports.as_array().unwrap().len(), 1, "{source}");
        assert!(reports[0]["fix"].is_null(), "{source}");
    }
    for source in [
        "async def down(self, _migrator):\n    return None\n",
        "def consume(value):\n    consume(value)\n    del value\n",
        "def make_reader(value):\n    del value\n    def read():\n        return value\n    return read\n",
        "def remove(items, key):\n    del items[key]\n",
        "def clear(resource):\n    del resource.handle\n",
        "def outer(value):\n    def inner():\n        del value\n    return inner\n",
        "def rebind(value):\n    del value\n    value = 1\n    return value\n",
    ] {
        common::assert_clean("no-del-unused-argument", source, "");
    }
}

#[test]
fn deleted_argument_fixes_rename_only_positional_and_variadic_parameters() {
    for (source, expected) in [
        (
            "def callback(*args, **kwargs):\n    del args, kwargs\n    return None\n",
            "def callback(*_args, **_kwargs):\n    return None\n",
        ),
        (
            "def close(_exc_type):\n    del _exc_type\n    return None\n",
            "def close(_exc_type):\n    return None\n",
        ),
        (
            "def callback(value, /):\n    del value\n    return None\n",
            "def callback(_value, /):\n    return None\n",
        ),
        (
            "def callback(value, /):\n    del value\n",
            "def callback(_value, /):\n    pass\n",
        ),
        (
            "def callback(value, /):\n    cache = 1\n    del (cache, value), cache\n",
            "def callback(_value, /):\n    cache = 1\n    del cache, cache\n",
        ),
        (
            "def callback(first, second, /):\n    del first\n    del second\n",
            "def callback(_first, _second, /):\n    pass\n",
        ),
        (
            "def callback(value, /):\n    if value_ready(): del value\n    return None\n",
            "def callback(_value, /):\n    if value_ready(): pass\n    return None\n",
        ),
        (
            "def callback(value, /):\n    if value_ready():\n        del value\n    return None\n",
            "def callback(_value, /):\n    if value_ready():\n        pass\n    return None\n",
        ),
    ] {
        common::assert_fix("no-del-unused-argument", source, "", expected);
    }
    let reports = common::check(
        "no-del-unused-argument",
        "def callback(value, /):\n    del value\n    return _value\n",
        "",
    );
    assert!(reports[0]["fix"].is_null());
}

#[test]
fn deleted_arguments_skip_test_and_configured_paths() {
    let files = [("tests/helpers.py", "def callback(value):\n    del value\n")];
    for (path, options) in [
        ("tests/helpers.py", ""),
        ("test_helpers.py", ""),
        ("benchmarks/run.py", ""),
        ("generated/run.py", "excluded_path_parts = ['generated']"),
    ] {
        let files = [(path, files[0].1)];
        let reports = common::check_project("no-del-unused-argument", &files, options, path);
        assert_eq!(reports.as_array().unwrap().len(), 0, "{path}");
    }
    let reports = common::check_project(
        "no-del-unused-argument",
        &files,
        "excluded_path_parts = []",
        "tests/helpers.py",
    );
    assert_eq!(reports.as_array().unwrap().len(), 1);
}

#[test]
fn subprocess_run_requires_a_timeout_through_aliases() {
    for source in [
        "import subprocess\nsubprocess.run(command, check=False)\n",
        "import subprocess\nsubprocess.run(command, timeout=None)\n",
        "import subprocess as sp\nsp.run(command)\n",
        "from subprocess import run as run_process\nrun_process(command)\n",
        "import subprocess\nrunner = subprocess.run\nrunner(command)\n",
        "import subprocess\ndef build():\n    runner = subprocess.run\n    return runner(command)\n",
        "from subprocess import *\nrun(command, **options)\n",
    ] {
        assert_eq!(
            count("require-subprocess-run-timeout", source, ""),
            1,
            "{source}"
        );
    }
    for source in [
        "import subprocess\nsubprocess.run(command, check=False, timeout=30)\n",
        "from subprocess import run\nrun(command, timeout=limit)\n",
        "import asyncio\nasyncio.run(main())\n",
        "import subprocess\nrunner = subprocess.run\ndef call(runner):\n    runner(command)\n",
        "def run(command):\n    pass\nrun(command)\n",
        "import subprocess\nsubprocess.Popen(command)\n",
    ] {
        common::assert_clean("require-subprocess-run-timeout", source, "");
    }
}

#[test]
fn empty_container_assignments_need_annotations() {
    for source in [
        "items = []\n",
        "payload = {}\n",
        "seen = set()\n",
        "first = second = list()\n",
        "class Example:\n    items: list[str] = []\n\nitems = []\n",
        "def clear(items):\n    items = []\n",
        "if (items := []):\n    pass\n",
        "items = []\nitems: list[str] = ['value']\n",
    ] {
        assert_eq!(
            count("require-typed-empty-container-assignment", source, ""),
            1,
            "{source}"
        );
    }
    for source in [
        "items: list[str] = []\n",
        "payload: dict[str, object] = {}\n",
        "items = [value]\n",
        "self.items = []\n",
        "empty = tuple()\n",
        "empty = ()\n",
        "items = list(source)\n",
        "def clear(items: list[str]) -> None:\n    items = []\n",
        "class Example:\n    items: list[str] = []\n",
        "items: list[str]\nitems = []\n",
        "items: list[str] = ['value']\ndef reset():\n    items = []\n",
        "def list():\n    pass\nitems = list()\n",
    ] {
        common::assert_clean("require-typed-empty-container-assignment", source, "");
    }
}

#[test]
fn boilerplate_docstrings_are_removed() {
    for (source, expected) in [
        (
            "class Status(StrEnum):\n    \"\"\"Represent Status state and behavior.\"\"\"\n\n    ACTIVE = \"active\"\n",
            "class Status(StrEnum):\n\n    ACTIVE = \"active\"\n",
        ),
        (
            "def create_record(name: str) -> Record:\n    \"\"\"Value for   create_record.\"\"\"\n\n    return Record(name=name)\n",
            "def create_record(name: str) -> Record:\n\n    return Record(name=name)\n",
        ),
        (
            "def load(path: str, strict: bool, max_depth: int) -> Config:\n    \"\"\"Load the config; strict mode rejects unknown keys.\n\n    Args:\n        path (str): Value for path.\n        strict: Value for strict.\n        max_depth (int): Value for max depth.\n    \"\"\"\n    return Config.read(path, strict=strict)\n",
            "def load(path: str, strict: bool, max_depth: int) -> Config:\n    \"\"\"Load the config; strict mode rejects unknown keys.\"\"\"\n    return Config.read(path, strict=strict)\n",
        ),
        (
            "def load(path: str, strict: bool) -> Config:\n    r'''Load the config.\n\n    Args:\n        path: The path.\n        strict: Reject unknown keys.\n    '''\n    return Config.read(path, strict=strict)\n",
            "def load(path: str, strict: bool) -> Config:\n    r'''Load the config.\n\n    Args:\n        strict: Reject unknown keys.\n    '''\n    return Config.read(path, strict=strict)\n",
        ),
    ] {
        common::assert_fix("no-boilerplate-docstrings", source, "", expected);
    }
    for source in [
        "def create_record():\n    \"\"\"Value for create_record.\"\"\"\n",
        "def create_record():\n    # Keep this note.\n    \"\"\"Value for create_record.\"\"\"\n    return 1\n",
    ] {
        let reports = common::check("no-boilerplate-docstrings", source, "");
        assert_eq!(reports.as_array().unwrap().len(), 1, "{source}");
        assert!(reports[0]["fix"].is_null(), "{source}");
    }
    for source in [
        "class Status(StrEnum):\n    ACTIVE = \"active\"\n",
        "def retry_timeout() -> float | None:\n    \"\"\"Value for retry timeout; None disables retries.\"\"\"\n    return None\n",
        "def parse_config(value: str) -> Config:\n    \"\"\"Parse the legacy wire format accepted by older clients.\"\"\"\n    return Config.model_validate_json(value)\n",
        "def create_record(): \"Value for create_record.\"\n",
        "def create_record():\n    \"Value for \" \"create_record.\"\n    return 1\n",
        "def load(path):\n    \"\"\"Load.\n\n    Args:\n        path: The path to read, relative to the root.\n    \"\"\"\n",
    ] {
        common::assert_clean("no-boilerplate-docstrings", source, "");
    }
}
