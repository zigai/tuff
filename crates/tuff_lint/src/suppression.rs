use std::sync::LazyLock;

use anyhow::{Result, anyhow};
use regex::Regex;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor, TraversalSignal};
use ruff_python_ast::{AnyNodeRef, ModModule, Stmt};
use ruff_python_trivia::{BackwardsTokenizer, CommentRanges, SimpleTokenKind, leading_indentation};
use ruff_source_file::{LineRanges, UniversalNewlines};
use ruff_text_size::{Ranged, TextRange, TextSize};

#[derive(Clone, Debug)]
pub struct IgnoreDirective {
    pub names: Option<Vec<String>>,
}

pub fn parse_comment(comment: &str) -> Result<Option<IgnoreDirective>> {
    static PATTERN: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
        Regex::new(
            r"#\s*tuff:\s*ignore(?:[ \t]*\[(?P<names>[a-z][a-z0-9]*(?:-[a-z0-9]+)*(?:,[ \t]*[a-z][a-z0-9]*(?:-[a-z0-9]+)*)*)\])?[ \t]*(?:#.*)?$",
        )
    });
    let pattern = PATTERN
        .as_ref()
        .map_err(|error| anyhow!("invalid Tuff ignore grammar: {error}"))?;
    Ok(pattern.captures(comment).map(|captures| IgnoreDirective {
        names: captures.name("names").map(|names| {
            names
                .as_str()
                .split(',')
                .map(|name| name.trim().to_owned())
                .collect()
        }),
    }))
}

#[derive(Debug)]
pub struct Suppression {
    pub comment: TextRange,
    pub target: TextRange,
    pub names: Option<Vec<String>>,
    module_header: bool,
    following_comment: Option<TextRange>,
}

#[derive(Debug, Default)]
pub struct Suppressions {
    pub directives: Vec<Suppression>,
    module: TextRange,
}

impl Suppressions {
    pub fn from_module(module: &ModModule, source: &str, comments: &CommentRanges) -> Result<Self> {
        if comments.is_empty() {
            return Ok(Self::default());
        }
        let mut parsed = Vec::new();
        for range in comments.iter().copied() {
            if source[range].contains("tuff:")
                && let Some(directive) = parse_comment(&source[range])?
            {
                parsed.push((range, directive));
            }
        }
        if parsed.is_empty() {
            return Ok(Self::default());
        }
        let mut collector = Statements {
            source,
            comments,
            headers: Vec::new(),
        };
        for statement in &module.body {
            collector.visit_stmt(statement);
        }
        collector.headers.sort_by_key(ruff_text_size::Ranged::start);
        let first = module.body.first().map(statement_start);
        let mut directives = Vec::new();
        for (comment, directive) in parsed {
            let line_start = source.line_start(comment.start());
            let prefix = &source[TextRange::new(line_start, comment.start())];
            let own_line = prefix.trim().is_empty();
            let leading = own_line
                .then(|| {
                    collector.headers.iter().copied().find(|header| {
                        if header.start() < comment.end() {
                            return false;
                        }
                        let indentation = leading_indentation(
                            &source
                                [TextRange::new(source.line_start(header.start()), header.start())],
                        );
                        prefix == indentation
                            && source[TextRange::new(comment.end(), header.start())]
                                .universal_newlines()
                                .all(|line| {
                                    let text = line.trim();
                                    text.is_empty() || text.starts_with('#')
                                })
                    })
                })
                .flatten();
            let target = leading.or_else(|| {
                collector
                    .headers
                    .iter()
                    .copied()
                    .filter(|header| {
                        header.contains(comment.start()) || header.end() == comment.start()
                    })
                    .min_by_key(|header| {
                        let same_line = source.line_start(header.start()) == line_start;
                        (
                            !same_line,
                            if same_line {
                                header.start()
                            } else {
                                header.len()
                            },
                        )
                    })
            });
            let following_comment = own_line
                .then(|| {
                    comments.iter().copied().find(|next| {
                        next.start() > comment.end()
                            && source[TextRange::new(comment.end(), next.start())]
                                .trim()
                                .is_empty()
                            && leading_indentation(
                                &source
                                    [TextRange::new(source.line_start(next.start()), next.start())],
                            ) == prefix
                    })
                })
                .flatten();
            let target = target.unwrap_or(comment);
            let module_header =
                first.is_some_and(|first| comment.end() <= first && target.contains(first));
            directives.push(Suppression {
                comment,
                target,
                names: directive.names,
                module_header,
                following_comment,
            });
        }
        Ok(Self {
            directives,
            module: module.range(),
        })
    }

    pub fn suppresses(&self, range: TextRange, matches: impl Fn(&str) -> bool) -> bool {
        self.directives.iter().any(|directive| {
            (directive.comment.contains(range.start())
                || directive
                    .following_comment
                    .is_some_and(|comment| comment.contains(range.start()))
                || directive.target.contains(range.start())
                || directive.module_header && range == self.module)
                && directive
                    .names
                    .as_ref()
                    .is_none_or(|names| names.iter().any(|name| matches(name)))
        })
    }
}

fn statement_start(statement: &Stmt) -> TextSize {
    let decorators = match statement {
        Stmt::FunctionDef(function) => Some(&function.decorator_list),
        Stmt::ClassDef(class) => Some(&class.decorator_list),
        _ => None,
    };
    decorators
        .and_then(|decorators| decorators.first())
        .map_or(statement.start(), |decorator| {
            decorator.start().min(statement.start())
        })
}

struct Statements<'a> {
    source: &'a str,
    comments: &'a CommentRanges,
    headers: Vec<TextRange>,
}

impl Statements<'_> {
    fn push(&mut self, range: TextRange, first_body: Option<TextSize>) {
        let end = first_body
            .filter(|body| self.source.line_start(*body) != self.source.line_start(range.start()))
            .map(|body| {
                BackwardsTokenizer::new(
                    self.source,
                    TextRange::new(range.start(), body),
                    self.comments,
                )
                .find(|token| token.kind() == SimpleTokenKind::Colon)
                .map_or(body, |colon| self.source.line_end(colon.end()))
            })
            .unwrap_or_else(|| self.source.line_end(range.end()));
        self.headers.push(TextRange::new(range.start(), end));
    }
}

impl<'ast> SourceOrderVisitor<'ast> for Statements<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        let body = match statement {
            Stmt::FunctionDef(node) => node.body.first(),
            Stmt::ClassDef(node) => node.body.first(),
            Stmt::If(node) => node.body.first(),
            Stmt::For(node) => node.body.first(),
            Stmt::While(node) => node.body.first(),
            Stmt::With(node) => node.body.first(),
            Stmt::Try(node) => node.body.first(),
            _ => None,
        };
        let first = body.map(statement_start).or_else(|| match statement {
            Stmt::Match(node) => node.cases.first().map(Ranged::start),
            _ => None,
        });
        self.push(
            TextRange::new(statement_start(statement), statement.end()),
            first,
        );
        source_order::walk_stmt(self, statement);
    }

    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        match node {
            AnyNodeRef::MatchCase(node) => {
                self.push(node.range(), node.body.first().map(statement_start))
            }
            AnyNodeRef::ElifElseClause(node) => {
                self.push(node.range(), node.body.first().map(statement_start))
            }
            AnyNodeRef::ExceptHandlerExceptHandler(node) => {
                self.push(node.range(), node.body.first().map(statement_start))
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }
}
