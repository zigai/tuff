use anyhow::{Context as _, Result, anyhow};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use memchr::memmem::Finder;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, ModModule};
use ruff_python_trivia::CommentRanges;
use ruff_text_size::{Ranged, TextRange};

use crate::manifest::RuleManifest;
use crate::node_kinds::{KindSet, kind_bit};

/// The checks Tuff runs before sending a file to Python for a rule.
#[derive(Debug)]
pub(crate) struct RuleFilter {
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
    patterns: Vec<Finder<'static>>,
    node_kinds: KindSet,
}

impl RuleFilter {
    pub(crate) fn new(rule: &RuleManifest) -> Result<Self> {
        let mut node_kinds = 0;
        for kind in &rule.node_kinds {
            node_kinds |= kind_bit(kind).ok_or_else(|| {
                anyhow!("unknown node kind `{kind}`; use an `ast` class name such as `Call`")
            })?;
        }
        Ok(Self {
            include: globs(&rule.include)?,
            exclude: globs(&rule.exclude)?,
            patterns: rule
                .source_patterns
                .iter()
                .map(|pattern| Finder::new(pattern.as_bytes()).into_owned())
                .collect(),
            node_kinds,
        })
    }

    pub(crate) fn matches_path(&self, relative: &str) -> bool {
        self.include
            .as_ref()
            .is_none_or(|include| include.is_match(relative))
            && !self
                .exclude
                .as_ref()
                .is_some_and(|exclude| exclude.is_match(relative))
    }

    pub(crate) fn matches_source(&self, source: &[u8]) -> bool {
        self.patterns.is_empty()
            || self
                .patterns
                .iter()
                .any(|pattern| pattern.find(source).is_some())
    }

    pub(crate) fn uses_node_kinds(&self) -> bool {
        self.node_kinds != 0
    }

    pub(crate) fn matches_node_kinds(&self, present: KindSet) -> bool {
        self.node_kinds == 0 || self.node_kinds & present != 0
    }
}

fn globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("invalid glob `{pattern}`"))?,
        );
    }
    Ok(Some(builder.build()?))
}

#[derive(Debug, Default)]
pub(crate) struct SideTable {
    pub(crate) qualified_names: Vec<(TextRange, String)>,
    pub(crate) comments: Vec<TextRange>,
}

/// Collects the facts Python rules read through `RuleContext`, keyed by source span.
pub(crate) fn side_table(
    module: &ModModule,
    comments: &CommentRanges,
    qualified_name: &dyn Fn(&Expr) -> Option<String>,
) -> SideTable {
    struct Names<'a> {
        resolve: &'a dyn Fn(&Expr) -> Option<String>,
        names: Vec<(TextRange, String)>,
    }

    impl<'ast> SourceOrderVisitor<'ast> for Names<'_> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if matches!(expr, Expr::Name(_) | Expr::Attribute(_))
                && let Some(name) = (self.resolve)(expr)
            {
                self.names.push((expr.range(), name));
            }
            source_order::walk_expr(self, expr);
        }
    }

    let mut names = Names {
        resolve: qualified_name,
        names: Vec::new(),
    };
    names.visit_body(&module.body);
    SideTable {
        qualified_names: names.names,
        comments: comments.iter().copied().collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ruff_python_ast::token::TokenKind;
    use ruff_python_parser::parse_module;

    use super::*;
    use crate::node_kinds;

    fn rule(edit: impl FnOnce(&mut RuleManifest)) -> RuleFilter {
        let mut rule = RuleManifest {
            name: "example".to_owned(),
            description: String::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            source_patterns: Vec::new(),
            node_kinds: Vec::new(),
            watch_files: Vec::new(),
            cacheable: true,
            path: PathBuf::from("rule.py"),
        };
        edit(&mut rule);
        RuleFilter::new(&rule).unwrap()
    }

    #[test]
    fn include_and_exclude_match_relative_paths() {
        let filter = rule(|rule| {
            rule.include = vec!["src/**/*.py".to_owned()];
            rule.exclude = vec!["src/generated/**".to_owned()];
        });
        assert!(filter.matches_path("src/package/module.py"));
        assert!(filter.matches_path("src/module.py"));
        assert!(!filter.matches_path("tests/test_module.py"));
        assert!(!filter.matches_path("src/generated/schema.py"));
    }

    #[test]
    fn single_star_stays_in_one_directory() {
        let filter = rule(|rule| rule.include = vec!["*.py".to_owned()]);
        assert!(filter.matches_path("module.py"));
        assert!(!filter.matches_path("package/module.py"));
    }

    #[test]
    fn any_source_pattern_admits_the_file() {
        let filter = rule(|rule| {
            rule.source_patterns = vec!["print(".to_owned(), "pdb".to_owned()];
        });
        assert!(filter.matches_source(b"import pdb\n"));
        assert!(filter.matches_source(b"print(1)\n"));
        assert!(!filter.matches_source(b"value = 1\n"));
        assert!(rule(|_| {}).matches_source(b""));
    }

    #[test]
    fn node_kinds_require_a_present_kind() {
        let filter = rule(|rule| rule.node_kinds = vec!["ClassDef".to_owned()]);
        let with_class = parse_module("class Model:\n    pass\n").unwrap();
        let without_class = parse_module("value = 1\n").unwrap();
        assert!(filter.matches_node_kinds(node_kinds::present(with_class.syntax())));
        assert!(!filter.matches_node_kinds(node_kinds::present(without_class.syntax())));
    }

    #[test]
    fn unknown_node_kinds_and_bad_globs_are_rejected() {
        let mut manifest = RuleManifest {
            name: "example".to_owned(),
            description: String::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            source_patterns: Vec::new(),
            node_kinds: vec!["ExprCall".to_owned()],
            watch_files: Vec::new(),
            cacheable: true,
            path: PathBuf::from("rule.py"),
        };
        assert!(RuleFilter::new(&manifest).is_err());
        manifest.node_kinds.clear();
        manifest.include = vec!["src/[".to_owned()];
        assert!(RuleFilter::new(&manifest).is_err());
    }

    #[test]
    fn side_table_records_resolved_names_and_comments() {
        let source = "import os  # note\nos.path.join(value)\n";
        let parsed = parse_module(source).unwrap();
        let comments = CommentRanges::new(
            parsed
                .tokens()
                .iter()
                .filter(|token| token.kind() == TokenKind::Comment)
                .map(Ranged::range)
                .collect(),
        );
        let table = side_table(parsed.syntax(), &comments, &|expr| match expr {
            Expr::Name(name) if name.id.as_str() == "os" => Some("os".to_owned()),
            Expr::Attribute(attribute) => Some(format!("os.{}", attribute.attr)),
            _ => None,
        });
        let names: Vec<_> = table
            .qualified_names
            .iter()
            .map(|(range, name)| (&source[*range], name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("os.path.join", "os.join"),
                ("os.path", "os.path"),
                ("os", "os"),
            ]
        );
        assert_eq!(
            table
                .comments
                .iter()
                .map(|range| &source[*range])
                .collect::<Vec<_>>(),
            ["# note"]
        );
    }
}
