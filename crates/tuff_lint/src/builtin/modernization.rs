use std::cmp::Ordering;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use ruff_python_ast::{self as ast, AnyNodeRef, CmpOp, Expr, NodeKind};
use ruff_text_size::{Ranged, TextRange};

use super::typing::imported;
use crate::{Context, Finding, LintRule, RuleDefinition, RuleOptions, reject_options};

pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! { name: "explicit-frozen-dataclass", code: "TUF020", explanation: "Specify dataclass frozen=True or frozen=False explicitly.", node_kinds: &[NodeKind::StmtClassDef], checks_comments: false, fixable: false, create: frozen_dataclass },
    crate::native_rule! { name: "no-named-tuple", code: "TUF021", explanation: "Consider dataclasses instead of named tuples, after checking tuple compatibility.", node_kinds: &[NodeKind::StmtClassDef, NodeKind::ExprCall], checks_comments: false, fixable: false, create: named_tuple }.replacing(&["UP014", "PYI024"]),
    crate::native_rule! { name: "no-static-if-condition", code: "TUF022", explanation: "Remove constant if conditions and temporary debug clauses.", node_kinds: &[NodeKind::StmtIf, NodeKind::ElifElseClause], checks_comments: false, fixable: false, create: static_if },
    crate::native_rule! { name: "use-tuff-ignore-comment", code: "TUF023", explanation: "Prefer named tuff: ignore[rule-name] comments for every lint check.", node_kinds: &[], checks_comments: true, fixable: false, create: ignore_comment },
];
#[derive(Debug)]
enum Modernization {
    Frozen,
    NamedTuple,
    StaticIf,
    Ignore,
}
fn frozen_dataclass(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Modernization::Frozen))
}
fn named_tuple(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Modernization::NamedTuple))
}
fn static_if(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Modernization::StaticIf))
}
fn ignore_comment(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(Modernization::Ignore))
}

fn is_named_tuple(c: &Context<'_, '_, '_>, e: &Expr) -> bool {
    imported(
        c,
        e,
        &["typing.NamedTuple", "typing_extensions.NamedTuple"],
        &["typing", "typing_extensions"],
        "NamedTuple",
    )
}
impl LintRule for Modernization {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        match (self, node) {
            (Self::Frozen, AnyNodeRef::StmtClassDef(class)) => {
                for decorator in &class.decorator_list {
                    let expression = &decorator.expression;
                    let (callable, frozen) = match expression {
                        Expr::Call(call) => (
                            &*call.func,
                            call.arguments
                                .keywords
                                .iter()
                                .any(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "frozen")),
                        ),
                        e => (e, false),
                    };
                    if !frozen
                        && imported(
                            c,
                            callable,
                            &["dataclasses.dataclass"],
                            &["dataclasses"],
                            "dataclass",
                        )
                    {
                        out.push(Finding::new("explicit-frozen-dataclass", decorator.range(), "Dataclass mutability must be explicit. Add `frozen=True` for immutable value objects or `frozen=False` when instances are intentionally mutable."));
                    }
                }
            }
            (Self::NamedTuple, AnyNodeRef::StmtClassDef(class)) => {
                if let Some(arguments) = &class.arguments {
                    if let Some(base) = arguments.args.iter().find(|e| is_named_tuple(c, e)) {
                        named_tuple_finding(base.range(), out);
                    }
                }
            }
            (Self::NamedTuple, AnyNodeRef::ExprCall(call)) => {
                if is_named_tuple(c, &call.func)
                    || imported(
                        c,
                        &call.func,
                        &["collections.namedtuple"],
                        &["collections"],
                        "namedtuple",
                    )
                {
                    named_tuple_finding(call.range(), out);
                }
            }
            (Self::StaticIf, AnyNodeRef::StmtIf(statement)) => {
                check_condition(&statement.test, out)
            }
            (Self::StaticIf, AnyNodeRef::ElifElseClause(clause)) => {
                if let Some(test) = &clause.test {
                    check_condition(test, out);
                }
            }
            _ => {}
        }
    }
    fn check_comment(&self, range: TextRange, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        if !matches!(self, Self::Ignore) {
            return;
        }
        // A directive must start a comment, or a nested whitespace-delimited
        // comment, and have a real token boundary (not noqaed/noqa-compatible).
        static NOQA: LazyLock<Option<Regex>> = LazyLock::new(|| {
            Regex::new(r"(?i)(?:^|\s)#\s*(?:(?:flake8|ruff):\s*)?noqa(?:$|[\s:,\[])").ok()
        });
        let Some(pattern) = &*NOQA else {
            return;
        };
        let text = c.text(range);
        if !pattern.is_match(text) {
            return;
        }
        out.push(Finding::new(
            "use-tuff-ignore-comment",
            range,
            "Use `tuff: ignore[rule-name]` to suppress lint checks by name instead of `noqa`.",
        ));
    }
}
fn named_tuple_finding(range: TextRange, out: &mut Vec<Finding>) {
    out.push(Finding::new("no-named-tuple", range, "NamedTuple can often be replaced with @dataclass, but dataclasses are not tuple-compatible; check callers before converting."));
}
fn check_condition(test: &Expr, out: &mut Vec<Finding>) {
    if truthiness(test).is_some() {
        out.push(Finding::new("no-static-if-condition", test.range(), "This `if` condition appears constant; verify the logic and remove any temporary debug clause."));
    }
}

fn collection_truth(items: &[Expr]) -> Option<bool> {
    if items.is_empty() {
        Some(false)
    } else if items.iter().any(|e| !matches!(e, Expr::Starred(_))) {
        Some(true)
    } else {
        None
    }
}
fn truthiness(e: &Expr) -> Option<bool> {
    match e {
        Expr::BooleanLiteral(b) => Some(b.value),
        Expr::NoneLiteral(_) => Some(false),
        Expr::EllipsisLiteral(_) => Some(true),
        Expr::NumberLiteral(n) => Some(match &n.value {
            ast::Number::Int(i) => i != &ast::Int::ZERO,
            ast::Number::Float(f) => *f != 0.0,
            ast::Number::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
        }),
        Expr::StringLiteral(s) => Some(!s.value.is_empty()),
        Expr::BytesLiteral(s) => Some(!s.value.is_empty()),
        Expr::FString(s) => {
            if s.value.is_empty_literal() {
                return Some(false);
            }
            if s.value.literals().any(|part| !part.value.is_empty())
                || s.value.elements().any(|element| match element {
                    ast::InterpolatedStringElement::Literal(literal) => !literal.value.is_empty(),
                    ast::InterpolatedStringElement::Interpolation(expression) => {
                        expression.debug_text.is_some()
                    }
                })
            {
                Some(true)
            } else {
                None
            }
        }
        Expr::List(n) => collection_truth(&n.elts),
        Expr::Tuple(n) => collection_truth(&n.elts),
        Expr::Set(n) => collection_truth(&n.elts),
        Expr::Dict(n) => {
            if n.items.is_empty() {
                Some(false)
            } else if n.items.iter().any(|item| item.key.is_some()) {
                Some(true)
            } else {
                None
            }
        }
        Expr::UnaryOp(n) => match n.op {
            ast::UnaryOp::Not => truthiness(&n.operand).map(|v| !v),
            ast::UnaryOp::UAdd | ast::UnaryOp::USub => truthiness(&n.operand),
            ast::UnaryOp::Invert => None,
        },
        Expr::BoolOp(n) => {
            let values: Vec<_> = n.values.iter().map(truthiness).collect();
            match n.op {
                ast::BoolOp::Or if values.contains(&Some(true)) => Some(true),
                ast::BoolOp::And if values.contains(&Some(false)) => Some(false),
                ast::BoolOp::Or if values.iter().all(Option::is_some) => Some(false),
                ast::BoolOp::And if values.iter().all(Option::is_some) => Some(true),
                _ => None,
            }
        }
        Expr::Named(n) => truthiness(&n.value),
        Expr::Generator(_) | Expr::Lambda(_) => Some(true),
        Expr::Compare(comparison) => comparison_truth(comparison),
        _ => None,
    }
}

// A small native literal evaluator, intentionally excluding calls, arithmetic,
// unpacking and comprehensions, just like ast.literal_eval in the source rule.
#[derive(Clone, Debug)]
enum Literal {
    None,
    Ellipsis,
    Bool(bool),
    Int(Integer),
    Float(f64),
    Complex(f64, f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Literal>),
    Tuple(Vec<Literal>),
    Set(Vec<Literal>),
    Dict(Vec<(Literal, Literal)>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Integer {
    negative: bool,
    digits: Vec<u8>,
}
impl Integer {
    fn parse(text: &str) -> Option<Self> {
        let normalized = text.replace('_', "").to_ascii_lowercase();
        let (negative, text) = normalized
            .strip_prefix('-')
            .map_or((false, normalized.as_str()), |s| (true, s));
        let (base, text) = if let Some(s) = text.strip_prefix("0x") {
            (16u16, s)
        } else if let Some(s) = text.strip_prefix("0o") {
            (8, s)
        } else if let Some(s) = text.strip_prefix("0b") {
            (2, s)
        } else {
            (10, text)
        };
        let mut digits = vec![0u8];
        for ch in text.chars() {
            let value = ch.to_digit(u32::from(base))?;
            let mut carry = u16::try_from(value).ok()?;
            for digit in &mut digits {
                let n = u16::from(*digit) * base + carry;
                *digit = (n % 10) as u8;
                carry = n / 10;
            }
            while carry > 0 {
                digits.push((carry % 10) as u8);
                carry /= 10;
            }
        }
        Some(Self {
            negative: negative && digits != [0],
            digits,
        })
    }
    fn cmp(&self, other: &Self) -> Ordering {
        if self.negative != other.negative {
            return if self.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let magnitude = self
            .digits
            .len()
            .cmp(&other.digits.len())
            .then_with(|| self.digits.iter().rev().cmp(other.digits.iter().rev()));
        if self.negative {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
    fn cmp_float(&self, other: f64) -> Option<Ordering> {
        if other.is_nan() {
            return None;
        }
        if other == f64::INFINITY {
            return Some(Ordering::Less);
        }
        if other == f64::NEG_INFINITY {
            return Some(Ordering::Greater);
        }
        let integral = Self::parse(&format!("{:.0}", other.trunc()))?;
        let order = self.cmp(&integral);
        Some(if order == Ordering::Equal && other.fract() != 0.0 {
            if other > 0.0 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else {
            order
        })
    }
    fn negate(&mut self) {
        if self.digits != [0] {
            self.negative = !self.negative;
        }
    }
}
fn literal(e: &Expr) -> Option<Literal> {
    match e {
        Expr::NoneLiteral(_) => Some(Literal::None),
        Expr::EllipsisLiteral(_) => Some(Literal::Ellipsis),
        Expr::BooleanLiteral(b) => Some(Literal::Bool(b.value)),
        Expr::NumberLiteral(n) => match &n.value {
            ast::Number::Int(i) => Some(Literal::Int(Integer::parse(&i.to_string())?)),
            ast::Number::Float(f) => Some(Literal::Float(*f)),
            ast::Number::Complex { real, imag } => Some(Literal::Complex(*real, *imag)),
        },
        Expr::StringLiteral(s) => Some(Literal::String(s.value.to_str().to_string())),
        Expr::BytesLiteral(s) => Some(Literal::Bytes(s.value.bytes().collect())),
        Expr::UnaryOp(u) if matches!(&*u.operand, Expr::NumberLiteral(_)) => {
            let mut value = literal(&u.operand)?;
            match u.op {
                ast::UnaryOp::UAdd => Some(value),
                ast::UnaryOp::USub => {
                    match &mut value {
                        Literal::Int(i) => i.negate(),
                        Literal::Float(f) => *f = -*f,
                        Literal::Complex(r, i) => {
                            *r = -*r;
                            *i = -*i;
                        }
                        _ => return None,
                    }
                    Some(value)
                }
                _ => None,
            }
        }
        Expr::List(n) => Some(Literal::List(
            n.elts.iter().map(literal).collect::<Option<_>>()?,
        )),
        Expr::Tuple(n) => Some(Literal::Tuple(
            n.elts.iter().map(literal).collect::<Option<_>>()?,
        )),
        Expr::Set(n) => {
            let values = n.elts.iter().map(literal).collect::<Option<Vec<_>>>()?;
            if values.iter().all(hashable) {
                Some(Literal::Set(values))
            } else {
                None
            }
        }
        Expr::Dict(n) => {
            let mut values: Vec<(Literal, Literal)> = Vec::new();
            for item in &n.items {
                let key = literal(item.key.as_ref()?)?;
                if !hashable(&key) {
                    return None;
                }
                let value = literal(&item.value)?;
                if let Some(old) = values.iter_mut().find(|(k, _)| equal(k, &key)) {
                    old.1 = value;
                } else {
                    values.push((key, value));
                }
            }
            Some(Literal::Dict(values))
        }
        _ => None,
    }
}
fn hashable(value: &Literal) -> bool {
    match value {
        Literal::List(_) | Literal::Dict(_) | Literal::Set(_) => false,
        Literal::Tuple(v) => v.iter().all(hashable),
        _ => true,
    }
}
fn as_integer(value: &Literal) -> Option<Integer> {
    match value {
        Literal::Bool(value) => Integer::parse(if *value { "1" } else { "0" }),
        Literal::Int(value) => Some(value.clone()),
        _ => None,
    }
}
fn numeric_order(a: &Literal, b: &Literal) -> Option<Ordering> {
    if let (Some(a), Some(b)) = (as_integer(a), as_integer(b)) {
        return Some(a.cmp(&b));
    }
    match (a, b) {
        (_, Literal::Float(b)) if as_integer(a).is_some() => as_integer(a)?.cmp_float(*b),
        (Literal::Float(a), _) if as_integer(b).is_some() => {
            as_integer(b)?.cmp_float(*a).map(Ordering::reverse)
        }
        (Literal::Float(a), Literal::Float(b)) => a.partial_cmp(b),
        _ => None,
    }
}
fn equal(a: &Literal, b: &Literal) -> bool {
    if let Some(order) = numeric_order(a, b) {
        return order == Ordering::Equal;
    }
    match (a, b) {
        (Literal::None, Literal::None) | (Literal::Ellipsis, Literal::Ellipsis) => true,
        (Literal::String(a), Literal::String(b)) => a == b,
        (Literal::Bytes(a), Literal::Bytes(b)) => a == b,
        (Literal::Complex(ar, ai), Literal::Complex(br, bi)) => ar == br && ai == bi,
        (Literal::Complex(real, imag), other) | (other, Literal::Complex(real, imag)) => {
            *imag == 0.0 && numeric_order(&Literal::Float(*real), other) == Some(Ordering::Equal)
        }
        (Literal::List(a), Literal::List(b)) | (Literal::Tuple(a), Literal::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
        }
        (Literal::Set(a), Literal::Set(b)) => subset(a, b) && subset(b, a),
        (Literal::Dict(a), Literal::Dict(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(ak, av)| b.iter().any(|(bk, bv)| equal(ak, bk) && equal(av, bv)))
        }
        _ => false,
    }
}
fn subset(a: &[Literal], b: &[Literal]) -> bool {
    a.iter().all(|a| b.iter().any(|b| equal(a, b)))
}
fn order(a: &Literal, b: &Literal) -> Option<Ordering> {
    if let Some(order) = numeric_order(a, b) {
        return Some(order);
    }
    match (a, b) {
        (Literal::String(a), Literal::String(b)) => Some(a.cmp(b)),
        (Literal::Bytes(a), Literal::Bytes(b)) => Some(a.cmp(b)),
        (Literal::List(a), Literal::List(b)) | (Literal::Tuple(a), Literal::Tuple(b)) => {
            for (a, b) in a.iter().zip(b) {
                if !equal(a, b) {
                    return order(a, b);
                }
            }
            Some(a.len().cmp(&b.len()))
        }
        _ => None,
    }
}
fn contains(container: &Literal, item: &Literal) -> Option<bool> {
    match container {
        Literal::List(values) | Literal::Tuple(values) => {
            Some(values.iter().any(|v| equal(v, item)))
        }
        Literal::Set(values) if hashable(item) => Some(values.iter().any(|v| equal(v, item))),
        Literal::Dict(values) if hashable(item) => Some(values.iter().any(|(v, _)| equal(v, item))),
        Literal::String(value) => {
            if let Literal::String(needle) = item {
                Some(value.contains(needle.as_str()))
            } else {
                None
            }
        }
        Literal::Bytes(value) => match item {
            Literal::Bytes(needle) => {
                Some(needle.is_empty() || value.windows(needle.len()).any(|w| w == needle))
            }
            _ => {
                let integer = as_integer(item)?;
                if integer.negative || integer.digits.len() > 3 {
                    return None;
                }
                let n = integer
                    .digits
                    .iter()
                    .rev()
                    .fold(0u16, |n, d| n * 10 + u16::from(*d));
                Some(value.contains(&u8::try_from(n).ok()?))
            }
        },
        _ => None,
    }
}
fn compare(a: &Literal, op: ast::CmpOp, b: &Literal) -> Option<bool> {
    match op {
        CmpOp::Eq => Some(equal(a, b)),
        CmpOp::NotEq => Some(!equal(a, b)),
        CmpOp::In => contains(b, a),
        CmpOp::NotIn => contains(b, a).map(|v| !v),
        CmpOp::Is | CmpOp::IsNot => {
            let same = match (a, b) {
                (Literal::None, Literal::None) | (Literal::Ellipsis, Literal::Ellipsis) => true,
                (Literal::Bool(a), Literal::Bool(b)) => a == b,
                (Literal::String(a), Literal::String(b)) if a == b => return None,
                (Literal::Bytes(a), Literal::Bytes(b)) if a == b => return None,
                (Literal::Tuple(_), Literal::Tuple(_)) if equal(a, b) => {
                    return None;
                }
                (Literal::Complex(_, _), Literal::Complex(_, _)) if equal(a, b) => return None,
                (Literal::Int(a), Literal::Int(b)) if a == b => return None,
                (Literal::Float(a), Literal::Float(b)) if a == b => return None,
                _ => false,
            };
            Some(if op == CmpOp::Is { same } else { !same })
        }
        _ => {
            if let (Literal::Set(a), Literal::Set(b)) = (a, b) {
                let ab = subset(a, b);
                let ba = subset(b, a);
                return Some(match op {
                    CmpOp::Lt => ab && !ba,
                    CmpOp::LtE => ab,
                    CmpOp::Gt => ba && !ab,
                    CmpOp::GtE => ba,
                    _ => false,
                });
            }
            let ordering = order(a, b)?;
            Some(match op {
                CmpOp::Lt => ordering == Ordering::Less,
                CmpOp::LtE => ordering != Ordering::Greater,
                CmpOp::Gt => ordering == Ordering::Greater,
                CmpOp::GtE => ordering != Ordering::Less,
                _ => false,
            })
        }
    }
}
fn comparison_truth(comparison: &ast::ExprCompare) -> Option<bool> {
    let mut operands = comparison.operands.iter();
    let mut current = literal(operands.next()?)?;
    for (operator, next) in comparison.ops.iter().zip(operands) {
        let next = literal(next)?;
        if !compare(&current, *operator, &next)? {
            return Some(false);
        }
        current = next;
    }
    Some(true)
}
