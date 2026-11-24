//! Maps Ruff's AST onto the `ast` class names that Python rules declare in `node_kinds`.
//!
//! The mapping may over-approximate: it only decides whether a rule can skip a file.

use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_node};
use ruff_python_ast::{AnyNodeRef, ModModule};

const KINDS: &[&str] = &[
    "Module",
    "FunctionDef",
    "AsyncFunctionDef",
    "ClassDef",
    "Return",
    "Delete",
    "Assign",
    "TypeAlias",
    "AugAssign",
    "AnnAssign",
    "For",
    "AsyncFor",
    "While",
    "If",
    "With",
    "AsyncWith",
    "Match",
    "Raise",
    "Try",
    "TryStar",
    "Assert",
    "Import",
    "ImportFrom",
    "Global",
    "Nonlocal",
    "Expr",
    "Pass",
    "Break",
    "Continue",
    "BoolOp",
    "NamedExpr",
    "BinOp",
    "UnaryOp",
    "Lambda",
    "IfExp",
    "Dict",
    "Set",
    "ListComp",
    "SetComp",
    "DictComp",
    "GeneratorExp",
    "Await",
    "Yield",
    "YieldFrom",
    "Compare",
    "Call",
    "FormattedValue",
    "Interpolation",
    "JoinedStr",
    "TemplateStr",
    "Constant",
    "Attribute",
    "Subscript",
    "Starred",
    "Name",
    "List",
    "Tuple",
    "Slice",
    "ExceptHandler",
    "arguments",
    "arg",
    "keyword",
    "alias",
    "withitem",
    "match_case",
    "comprehension",
    "MatchValue",
    "MatchSingleton",
    "MatchSequence",
    "MatchMapping",
    "MatchClass",
    "MatchStar",
    "MatchAs",
    "MatchOr",
    "TypeVar",
    "ParamSpec",
    "TypeVarTuple",
];

/// A set of Python node kinds, one bit per entry of [`KINDS`].
pub(crate) type KindSet = u128;

const _: () = assert!(KINDS.len() <= KindSet::BITS as usize);

pub(crate) fn kind_bit(name: &str) -> Option<KindSet> {
    KINDS
        .iter()
        .position(|kind| *kind == name)
        .map(|index| 1 << index)
}

fn bits(names: &[&str]) -> KindSet {
    names
        .iter()
        .filter_map(|name| kind_bit(name))
        .fold(0, |set, bit| set | bit)
}

fn python_kinds(node: AnyNodeRef<'_>) -> KindSet {
    let names: &[&str] = match node {
        AnyNodeRef::ModModule(_) => &["Module"],
        AnyNodeRef::StmtFunctionDef(function) if function.is_async => &["AsyncFunctionDef"],
        AnyNodeRef::StmtFunctionDef(_) => &["FunctionDef"],
        AnyNodeRef::StmtClassDef(_) => &["ClassDef"],
        AnyNodeRef::StmtReturn(_) => &["Return"],
        AnyNodeRef::StmtDelete(_) => &["Delete"],
        AnyNodeRef::StmtTypeAlias(_) => &["TypeAlias"],
        AnyNodeRef::StmtAssign(_) => &["Assign"],
        AnyNodeRef::StmtAugAssign(_) => &["AugAssign"],
        AnyNodeRef::StmtAnnAssign(_) => &["AnnAssign"],
        AnyNodeRef::StmtFor(statement) if statement.is_async => &["AsyncFor"],
        AnyNodeRef::StmtFor(_) => &["For"],
        AnyNodeRef::StmtWhile(_) => &["While"],
        AnyNodeRef::StmtIf(_) => &["If"],
        AnyNodeRef::ElifElseClause(clause) if clause.test.is_some() => &["If"],
        AnyNodeRef::StmtWith(statement) if statement.is_async => &["AsyncWith"],
        AnyNodeRef::StmtWith(_) => &["With"],
        AnyNodeRef::StmtMatch(_) => &["Match"],
        AnyNodeRef::StmtRaise(_) => &["Raise"],
        AnyNodeRef::StmtTry(statement) if statement.is_star => &["TryStar"],
        AnyNodeRef::StmtTry(_) => &["Try"],
        AnyNodeRef::StmtAssert(_) => &["Assert"],
        AnyNodeRef::StmtImport(_) => &["Import"],
        AnyNodeRef::StmtImportFrom(_) => &["ImportFrom"],
        AnyNodeRef::StmtGlobal(_) => &["Global"],
        AnyNodeRef::StmtNonlocal(_) => &["Nonlocal"],
        AnyNodeRef::StmtExpr(_) => &["Expr"],
        AnyNodeRef::StmtPass(_) => &["Pass"],
        AnyNodeRef::StmtBreak(_) => &["Break"],
        AnyNodeRef::StmtContinue(_) => &["Continue"],
        AnyNodeRef::ExprBoolOp(_) => &["BoolOp"],
        AnyNodeRef::ExprNamed(_) => &["NamedExpr"],
        AnyNodeRef::ExprBinOp(_) => &["BinOp"],
        AnyNodeRef::ExprUnaryOp(_) => &["UnaryOp"],
        // CPython gives every lambda an `arguments` node, even without parameters.
        AnyNodeRef::ExprLambda(_) => &["Lambda", "arguments"],
        AnyNodeRef::ExprIf(_) => &["IfExp"],
        AnyNodeRef::ExprDict(_) => &["Dict"],
        AnyNodeRef::ExprSet(_) => &["Set"],
        AnyNodeRef::ExprListComp(_) => &["ListComp"],
        AnyNodeRef::ExprSetComp(_) => &["SetComp"],
        AnyNodeRef::ExprDictComp(_) => &["DictComp"],
        AnyNodeRef::ExprGenerator(_) => &["GeneratorExp"],
        AnyNodeRef::ExprAwait(_) => &["Await"],
        AnyNodeRef::ExprYield(_) => &["Yield"],
        AnyNodeRef::ExprYieldFrom(_) => &["YieldFrom"],
        AnyNodeRef::ExprCompare(_) => &["Compare"],
        AnyNodeRef::ExprCall(_) => &["Call"],
        AnyNodeRef::ExprFString(_) | AnyNodeRef::InterpolatedStringFormatSpec(_) => &["JoinedStr"],
        AnyNodeRef::ExprTString(_) => &["TemplateStr"],
        AnyNodeRef::InterpolatedElement(_) => &["FormattedValue", "Interpolation"],
        AnyNodeRef::ExprStringLiteral(_)
        | AnyNodeRef::ExprBytesLiteral(_)
        | AnyNodeRef::ExprNumberLiteral(_)
        | AnyNodeRef::ExprBooleanLiteral(_)
        | AnyNodeRef::ExprNoneLiteral(_)
        | AnyNodeRef::ExprEllipsisLiteral(_)
        | AnyNodeRef::InterpolatedStringLiteralElement(_) => &["Constant"],
        AnyNodeRef::ExprAttribute(_) => &["Attribute"],
        AnyNodeRef::ExprSubscript(_) => &["Subscript"],
        AnyNodeRef::ExprStarred(_) => &["Starred"],
        AnyNodeRef::ExprName(_) => &["Name"],
        AnyNodeRef::ExprList(_) => &["List"],
        AnyNodeRef::ExprTuple(_) => &["Tuple"],
        AnyNodeRef::ExprSlice(_) => &["Slice"],
        AnyNodeRef::ExceptHandlerExceptHandler(_) => &["ExceptHandler"],
        AnyNodeRef::Parameters(_) => &["arguments"],
        AnyNodeRef::Parameter(_) => &["arg"],
        AnyNodeRef::Keyword(_) => &["keyword"],
        AnyNodeRef::Alias(_) => &["alias"],
        AnyNodeRef::WithItem(_) => &["withitem"],
        AnyNodeRef::MatchCase(_) => &["match_case"],
        AnyNodeRef::Comprehension(_) => &["comprehension"],
        AnyNodeRef::PatternMatchValue(_) => &["MatchValue"],
        AnyNodeRef::PatternMatchSingleton(_) => &["MatchSingleton"],
        AnyNodeRef::PatternMatchSequence(_) => &["MatchSequence"],
        AnyNodeRef::PatternMatchMapping(_) => &["MatchMapping"],
        AnyNodeRef::PatternMatchClass(_) => &["MatchClass"],
        AnyNodeRef::PatternMatchStar(_) => &["MatchStar"],
        AnyNodeRef::PatternMatchAs(_) => &["MatchAs"],
        AnyNodeRef::PatternMatchOr(_) => &["MatchOr"],
        AnyNodeRef::TypeParamTypeVar(_) => &["TypeVar"],
        AnyNodeRef::TypeParamParamSpec(_) => &["ParamSpec"],
        AnyNodeRef::TypeParamTypeVarTuple(_) => &["TypeVarTuple"],
        _ => &[],
    };
    bits(names)
}

pub(crate) fn present(module: &ModModule) -> KindSet {
    struct Collector(KindSet);

    impl<'a> SourceOrderVisitor<'a> for Collector {
        fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
            self.0 |= python_kinds(node);
            TraversalSignal::Traverse
        }
    }

    let mut collector = Collector(0);
    walk_node(&mut collector, AnyNodeRef::ModModule(module));
    collector.0
}

#[cfg(test)]
mod tests {
    use ruff_python_parser::parse_module;

    use super::*;

    #[test]
    fn present_kinds_use_python_class_names() {
        let parsed = parse_module("async def run():\n    await call(f'{x}')\n").unwrap();
        let present = present(parsed.syntax());
        for name in [
            "Module",
            "AsyncFunctionDef",
            "Await",
            "Call",
            "JoinedStr",
            "Name",
        ] {
            assert_ne!(present & kind_bit(name).unwrap(), 0, "{name}");
        }
        for name in ["FunctionDef", "Import", "ClassDef"] {
            assert_eq!(present & kind_bit(name).unwrap(), 0, "{name}");
        }
    }

    #[test]
    fn unknown_kind_names_have_no_bit() {
        assert_eq!(kind_bit("ExprCall"), None);
        assert_eq!(kind_bit("expr"), None);
    }
}
