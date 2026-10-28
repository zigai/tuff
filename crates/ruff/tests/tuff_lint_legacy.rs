#[path = "tuff/mod.rs"]
mod common;

#[test]
fn collections_modernization_only_changes_absolute_imports() {
    for source in [
        "from .collections import Sequence\n",
        "from ..collections import Sequence, Counter\n",
    ] {
        common::assert_clean("use-collections-abc", source, "");
    }
    common::assert_fix(
        "use-collections-abc",
        "from collections import Sequence\n",
        "",
        "from collections.abc import Sequence\n",
    );
}

#[test]
fn primitive_comparison_fixes_require_known_equality_behavior() {
    for source in ["value is 42\n", "42 is value\n"] {
        common::assert_unsafe_fix("use-eq-for-primitives", source);
    }
    diagnostic_only("use-is-for-singletons", "[value] == None\n");
    let reports = common::check("use-eq-for-primitives", "value = 42\nvalue is 42\n", "");
    assert_eq!(reports[0]["fix"]["applicability"], "safe");
}

#[test]
fn assertion_rewrites_require_explicit_unsafe_fixes() {
    for (rule, source) in [
        (
            "use-assert-equal",
            "def run(self):\n    self.assertTrue(first() == second(), message())\n",
        ),
        (
            "use-assert-in",
            "def run(self):\n    self.assertTrue(first() in container(), message())\n",
        ),
        (
            "use-assert-is-not-none",
            "def run(self):\n    self.assertTrue(first() is not None, message())\n",
        ),
        (
            "use-assert-equal",
            "import unittest\nclass Item: pass\nclass Container(unittest.TestCase):\n    def setUp(self):\n        self.addTypeEqualityFunc(Item, self.compare)\n    def run(self):\n        self.assertTrue(Item() == Item(), message())\n",
        ),
        (
            "use-assert-equal",
            "class Base:\n    def assertEqual(self, left, right, message=None): pass\nclass Container(Base):\n    def run(self):\n        self.assertTrue(first() == second(), message())\n",
        ),
        (
            "deprecated-unittest-asserts",
            "def run(self):\n    self.assertEquals(first(), second())\n",
        ),
    ] {
        common::assert_unsafe_fix(rule, source);
    }
}

fn count(rule: &str, source: &str, expected: usize) {
    assert_eq!(
        common::check(rule, source, "").as_array().unwrap().len(),
        expected,
        "{rule}: {source}"
    );
}
fn diagnostic_only(rule: &str, source: &str) {
    let result = common::check(rule, source, "");
    assert_eq!(result.as_array().unwrap().len(), 1, "{rule}: {source}");
    assert!(result[0]["fix"].is_null(), "{rule}: {result}");
}

#[test]
fn diagnostic_only_rules_preserve_rattle_semantics() {
    for (rule, source) in [
        (
            "collapse-isinstance-checks",
            "isinstance(x, A) or isinstance(x, build_type())\n",
        ),
        (
            "no-redundant-arguments-super",
            "class Child(Parent):\n    def method(self):\n        return lambda: super(Child, self).method()\n",
        ),
        (
            "no-redundant-lambda",
            "callback = lambda value: target(value)\n",
        ),
        (
            "no-redundant-list-comprehension",
            "any([f(x) for x in xs])\n",
        ),
        ("use-comprehension", "set([f(x) for x in xs])\n"),
        ("use-comprehension", "list(stop() for _ in xs)\n"),
        (
            "replace-union-with-optional",
            "class Union: pass\nx: Union[str, None]\n",
        ),
    ] {
        diagnostic_only(rule, source);
    }
    for (rule, source) in [
        (
            "collapse-isinstance-checks",
            "isinstance(f(), A) or isinstance(f(), B)\n",
        ),
        (
            "collapse-isinstance-checks",
            "isinstance(x, (A, B)) or isinstance(x, C)\n",
        ),
        ("no-redundant-lambda", "lambda x: obj.method(x)\n"),
        (
            "no-redundant-lambda",
            "class C:\n    callback = lambda x: foo(x)\n",
        ),
        ("no-redundant-lambda", "lambda x=1: foo(x)\n"),
        (
            "no-redundant-list-comprehension",
            "sum([f(x) for x in xs])\n",
        ),
        ("replace-union-with-optional", "x: Union[str, int, None]\n"),
        ("replace-union-with-optional", "x: list[Union[str, None]]\n"),
    ] {
        common::assert_clean(rule, source, "");
    }
}

#[test]
fn or_in_except_is_recursive_but_not_and() {
    for expression in [
        "ValueError or TypeError",
        "(ValueError, TypeError or OSError)",
    ] {
        for keyword in ["except", "except*"] {
            diagnostic_only(
                "no-or-in-except",
                &format!("try:\n    pass\n{keyword} {expression}:\n    pass\n"),
            );
        }
    }
    common::assert_clean("no-or-in-except", "try: pass\nexcept A and B: pass\n", "");
}

#[test]
fn primitive_and_singleton_comparisons() {
    common::assert_fix("use-eq-for-primitives", "1  is   2\n", "", "1  ==   2\n");
    common::assert_fix(
        "use-eq-for-primitives",
        "a is f'{b}'\n",
        "",
        "a == f'{b}'\n",
    );
    common::assert_fix(
        "use-eq-for-primitives",
        "3 > a is 2 is not 'x'\n",
        "",
        "3 > a == 2 != 'x'\n",
    );
    for source in ["1 is True\n", "True is 1\n", "[] is x\n"] {
        common::assert_clean("use-eq-for-primitives", source, "");
    }
    common::assert_fix(
        "use-is-for-singletons",
        "None==False\n",
        "",
        "None is False\n",
    );
    common::assert_fix(
        "use-is-for-singletons",
        "value = []\nvalue!=None\n",
        "",
        "value = []\nvalue is not None\n",
    );
    for source in ["x == True\n", "x!=None\n", "x is True == y\n"] {
        diagnostic_only("use-is-for-singletons", source);
    }
    for source in ["1 == True\n", "False == -0\n", "True != +1\n"] {
        common::assert_clean("use-is-for-singletons", source, "");
    }
}

#[test]
fn unittest_alias_scope_and_fixes() {
    for receiver in ["self", "case", "cls", "super()"] {
        common::assert_fix(
            "deprecated-unittest-asserts",
            &format!("{receiver}.assertEquals(a, b)\n"),
            "",
            &format!("{receiver}.assertEqual(a, b)\n"),
        );
    }
    common::assert_clean(
        "deprecated-unittest-asserts",
        "obj.assertEquals(a, b)\nself.failUnless(x)\n",
        "",
    );
    for (assertion, condition, destination) in [
        ("assertTrue", "a in b", Some("assertIn(a, b)")),
        ("assertTrue", "not a in b", Some("assertNotIn(a, b)")),
        ("assertTrue", "a not in b", Some("assertNotIn(a, b)")),
        ("assertTrue", "not a not in b", None),
        ("assertFalse", "a in b", Some("assertNotIn(a, b)")),
        ("assertFalse", "not a in b", None),
        ("assertFalse", "a not in b", None),
        ("assertFalse", "not a not in b", None),
    ] {
        let source = format!("self.{assertion}({condition})\n");
        if let Some(destination) = destination {
            common::assert_fix(
                "use-assert-in",
                &source,
                "",
                &format!("self.{destination}\n"),
            );
        } else {
            common::assert_clean("use-assert-in", &source, "");
        }
    }
    for prefix in ["", "expr=", "*", "**"] {
        common::assert_fix(
            "use-assert-in",
            &format!("self.assertTrue({prefix}(a in b))\n"),
            "",
            "self.assertIn(a, b)\n",
        );
    }
    common::assert_fix(
        "use-assert-equal",
        "self.assertTrue(a != b, 'message')\n",
        "",
        "self.assertNotEqual(a, b, 'message')\n",
    );
    diagnostic_only("use-assert-equal", "self.assertTrue(not a == b)\n");
    for (condition, replacement) in [
        ("x is None", "assertIsNone"),
        ("x is not None", "assertIsNotNone"),
        ("not x is None", "assertIsNotNone"),
        ("not x is not None", "assertIsNone"),
    ] {
        common::assert_fix(
            "use-assert-is-not-none",
            &format!("self.assertTrue({condition}, 'message')\n"),
            "",
            &format!("self.{replacement}(x, 'message')\n"),
        );
    }
}

#[test]
fn assertion_overrides_and_comments() {
    for (rule, condition, destination) in [
        ("use-assert-in", "a in b", "assertIn"),
        ("use-assert-equal", "a == b", "assertEqual"),
        ("use-assert-is-not-none", "a is None", "assertIsNone"),
    ] {
        for overridden in ["assertTrue", destination] {
            common::assert_clean(
                rule,
                &format!(
                    "class Checker:\n    def {overridden}(self, *args): pass\n    def check(self, a, b):\n        self.assertTrue({condition})\n"
                ),
                "",
            );
        }
        diagnostic_only(
            rule,
            &format!("self.assertTrue(\n    {condition}  # rationale\n)\n"),
        );
    }
}

#[test]
fn literal_rewrites_and_comment_safety() {
    for (source, expected) in [
        ("tuple([1, 2])", "(1, 2)"),
        ("tuple([1])", "(1,)"),
        ("list((1, 2))", "[1, 2]"),
        ("set([])", "set()"),
        ("set([1, 2])", "{1, 2}"),
        ("dict([(1, 2), (3, 4)])", "{1: 2, 3: 4}"),
        ("tuple()", "()"),
        ("list()", "[]"),
        ("dict()", "{}"),
    ] {
        common::assert_fix(
            "use-literal",
            &format!("{source}\n"),
            "",
            &format!("{expected}\n"),
        );
    }
    for source in [
        "dict(foo=[])\n",
        "tuple(*[[1, 2]])\n",
        "dict([(1, *items)])\n",
        "set()\n",
    ] {
        common::assert_clean("use-literal", source, "");
    }
    diagnostic_only("use-literal", "list([\n    # rationale\n    item,\n])\n");
}

#[test]
fn redundant_object_and_fstrings() {
    common::assert_fix(
        "no-inherit-from-object",
        "class C(object): pass\n",
        "",
        "class C: pass\n",
    );
    diagnostic_only(
        "no-inherit-from-object",
        "class C(object, metaclass=Meta): pass\n",
    );
    diagnostic_only("no-inherit-from-object", "class C(object, Other): pass\n");
    common::assert_clean(
        "no-inherit-from-object",
        "class object: pass\nclass C(object): pass\n",
        "",
    );
    for source in [
        "f'not a docstring'\n",
        "def f(): f'not a docstring'\n",
        "class C:\n    f'not a docstring'\n",
    ] {
        common::assert_clean("no-redundant-f-string", source, "");
    }
    common::assert_fix(
        "no-redundant-f-string",
        "x = f'{{braces}}'\n",
        "",
        "x = '{braces}'\n",
    );
    common::assert_fix("no-redundant-f-string", "x = f''\n", "", "x = ''\n");
    count("no-redundant-f-string", "x = f'plain' f'{value}'\n", 1);
}

#[test]
fn postponed_annotations() {
    common::assert_fix(
        "no-string-type-annotation",
        "from __future__ import annotations\nx: 'Lat' 'er'\n",
        "",
        "from __future__ import annotations\nx: Later\n",
    );
    common::assert_fix(
        "no-string-type-annotation",
        "from __future__ import annotations\nx: 'int, str'\n",
        "",
        "from __future__ import annotations\nx: (int, str)\n",
    );
    common::assert_clean("no-string-type-annotation", "x: 'Later'\n", "");
    common::assert_clean(
        "no-string-type-annotation",
        "from __future__ import annotations\nx: typing.Literal['unit']\nimport typing\n",
        "",
    );
    common::assert_clean(
        "no-string-type-annotation",
        "from __future__ import annotations\nfrom typing import Annotated\nx: Annotated[int, Metadata['unit']]\n",
        "",
    );
    diagnostic_only(
        "no-string-type-annotation",
        "from __future__ import annotations\nx: b'int'\n",
    );
    common::assert_clean(
        "no-string-type-annotation",
        "from __future__ import annotations\nfrom typing import Literal\nAlias = Literal\nvalue: Alias['unit']\n",
        "",
    );
    for source in [
        "from __future__ import annotations\nvalue: 'int # retained' = 1\n",
        "from __future__ import annotations\nvalue: ('in' # retained\n        't') = 1\n",
    ] {
        diagnostic_only("no-string-type-annotation", source);
    }
    common::assert_fix(
        "no-string-type-annotation",
        "from __future__ import annotations\nvalue: '''int\n''' = 1\n",
        "",
        "from __future__ import annotations\nvalue: (int\n) = 1\n",
    );
}

#[test]
fn abc_imports_and_bases() {
    common::assert_fix(
        "use-collections-abc",
        "from collections import Mapping\n",
        "",
        "from collections.abc import Mapping\n",
    );
    common::assert_fix(
        "use-collections-abc",
        "from collections import defaultdict, Mapping\n",
        "",
        "from collections import defaultdict\nfrom collections.abc import Mapping\n",
    );
    common::assert_fix(
        "use-collections-abc",
        "import collections.Mapping as mapping\n",
        "",
        "import collections.abc.Mapping as mapping\n",
    );
    common::assert_fix(
        "use-collections-abc",
        "class C(collections.Mapping): pass\n",
        "",
        "class C(collections.abc.Mapping): pass\n",
    );
    common::assert_clean(
        "use-collections-abc",
        "try: work()\nexcept ImportError:\n    from collections import Mapping\n",
        "",
    );
    count(
        "use-collections-abc",
        "class C(collections.Mapping, collections.Sequence): pass\n",
        2,
    );
    common::assert_fix(
        "use-collections-abc",
        "class C(Keep,\tcollections.Mapping ,  collections.Sequence, metaclass=Meta):\n    pass\n",
        "",
        "class C(Keep,\tcollections.abc.Mapping, collections.abc.Sequence, metaclass=Meta):\n    pass\n",
    );
}

#[test]
fn sleep_alias_and_async_scope() {
    for source in [
        "import time\nasync def f():\n    time.sleep(1)\n",
        "import time\nnap = time.sleep\nasync def f():\n    nap(1)\n",
        "from time import *\nasync def f():\n    sleep(1)\n",
    ] {
        diagnostic_only("use-async-sleep-in-async-def", source);
    }
    common::assert_clean(
        "use-async-sleep-in-async-def",
        "import time\nasync def f():\n    def g():\n        time.sleep(1)\n",
        "",
    );
    common::assert_clean(
        "use-async-sleep-in-async-def",
        "from time import *\nasync def f():\n    sleep = something_else\n    sleep(1)\n",
        "",
    );
}

#[test]
fn classmethod_binding_aware_rename() {
    common::assert_fix(
        "use-cls-in-classmethod",
        "class C:\n    @classmethod\n    def build(): pass\n",
        "",
        "class C:\n    @classmethod\n    def build(cls): pass\n",
    );
    common::assert_fix(
        "use-cls-in-classmethod",
        "class C:\n    @classmethod\n    def build(receiver):\n        return receiver\n",
        "",
        "class C:\n    @classmethod\n    def build(cls):\n        return cls\n",
    );
    for body in [
        "return locals()['receiver']",
        "global cls\n        return receiver",
        "cls = 1",
        "import math as receiver\n        return receiver.pi",
        "def receiver(): pass",
    ] {
        diagnostic_only(
            "use-cls-in-classmethod",
            &format!("class C:\n    @classmethod\n    def build(receiver):\n        {body}\n"),
        );
    }
    for parameters in ["*, cls", "*cls", "**cls", "*args, cls=None"] {
        diagnostic_only(
            "use-cls-in-classmethod",
            &format!("class C:\n    @classmethod\n    def build({parameters}): pass\n"),
        );
    }
}

#[test]
fn configurable_fstring_and_conservative_percent_fixes() {
    common::assert_fix(
        "use-f-string",
        "result = \"%s\" % \"hello\"\n",
        "",
        "result = f\"{'hello'!s}\"\n",
    );
    common::assert_fix(
        "use-f-string",
        "value = 'ok'\nresult = \"%s\" % value\n",
        "",
        "value = 'ok'\nresult = f\"{value!s}\"\n",
    );
    common::assert_fix(
        "use-f-string",
        "result = \"%s %s\" % (1, 2)\n",
        "",
        "result = f\"{1!s} {2!s}\"\n",
    );
    for source in [
        "\"%s\" % unknown\n",
        "\"%s\" % (first, second)\n",
        "\"%s %s\" % (f(), g())\n",
        "\"%d\" % 1\n",
        "\"hello\".format()\n",
        "class Value:\n    __new__ = lambda cls: (1, 2)\nresult = '%s' % Value()\n",
    ] {
        diagnostic_only("use-f-string", source);
    }
    common::assert_clean("use-f-string", "b'%s' % value\n", "");
    let source = "abcdefghijklmnopqrstuvwxyz0123456789 = 'known'\nx = '%s' % abcdefghijklmnopqrstuvwxyz0123456789\n";
    diagnostic_only("use-f-string", source);
    common::assert_fix(
        "use-f-string",
        source,
        "simple_expression_max_length = 100",
        "abcdefghijklmnopqrstuvwxyz0123456789 = 'known'\nx = f'{abcdefghijklmnopqrstuvwxyz0123456789!s}'\n",
    );
}
