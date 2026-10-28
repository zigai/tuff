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
