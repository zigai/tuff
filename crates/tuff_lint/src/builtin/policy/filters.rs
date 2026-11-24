use std::fmt;
use std::marker::PhantomData;
use std::path::Component;

use anyhow::{Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use ruff_python_ast::{self as ast, AnyNodeRef, Expr};
use ruff_text_size::{Ranged, TextRange};
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};

use crate::Context;

#[derive(Debug)]
pub(super) enum Shorthand<T> {
    Text(String),
    Table(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Shorthand<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntryVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for EntryVisitor<T> {
            type Value = Shorthand<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a shorthand string or a table")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(Shorthand::Text(value.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                // Deserializing the table directly keeps precise errors such as unknown fields.
                T::deserialize(MapAccessDeserializer::new(map)).map(Shorthand::Table)
            }
        }

        deserializer.deserialize_any(EntryVisitor(PhantomData))
    }
}

pub(super) fn dedup_by<T>(items: Vec<T>, same: impl Fn(&T, &T) -> bool) -> Vec<T> {
    let mut unique: Vec<T> = Vec::new();
    for item in items {
        if let Some(previous) = unique.iter_mut().find(|previous| same(previous, &item)) {
            *previous = item;
        } else {
            unique.push(item);
        }
    }
    unique
}

pub(super) fn optional(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[derive(Debug, Default)]
pub(super) struct Advice {
    message: Option<String>,
    replacement: Option<String>,
}

impl Advice {
    pub(super) fn new(message: Option<&str>, replacement: Option<String>) -> Self {
        Self {
            message: optional(message),
            replacement,
        }
    }

    // A recommendation need not accept the same arguments or preserve semantics, so it is
    // advice and never a fix.
    pub(super) fn render(&self, default: impl FnOnce() -> String) -> String {
        let mut message = self.message.clone().unwrap_or_else(default);
        if let Some(replacement) = &self.replacement {
            message.push_str(" Use instead: ");
            message.push_str(replacement);
            message.push('.');
        }
        message
    }
}

#[derive(Debug, Default)]
pub(super) struct PathScope {
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
}

impl PathScope {
    pub(super) fn new(include: &[String], exclude: &[String]) -> Result<Self> {
        Ok(Self {
            include: glob_set(include)?,
            exclude: glob_set(exclude)?,
        })
    }

    pub(super) fn is_unrestricted(&self) -> bool {
        self.include.is_none() && self.exclude.is_none()
    }

    // `path` is `None` for files outside the configuration root, which match no pattern.
    pub(super) fn allows(&self, path: Option<&str>) -> bool {
        let included = match (&self.include, path) {
            (None, _) => true,
            (Some(include), Some(path)) => include.is_match(path),
            (Some(_), None) => false,
        };
        included
            && !self
                .exclude
                .as_ref()
                .zip(path)
                .is_some_and(|(exclude, path)| exclude.is_match(path))
    }
}

fn glob_set(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let normalized = normalize_glob(pattern)?;
        // `*` stays within one path segment, and a directory pattern such as `tests` also
        // covers every file below it.
        builder.add(
            GlobBuilder::new(&normalized)
                .literal_separator(true)
                .build()?,
        );
        if !normalized.ends_with("**") {
            builder.add(
                GlobBuilder::new(&format!("{normalized}/**"))
                    .literal_separator(true)
                    .build()?,
            );
        }
    }
    Ok(Some(builder.build()?))
}

fn normalize_glob(pattern: &str) -> Result<String> {
    let path = std::path::Path::new(pattern);
    if pattern.starts_with('/') || path.is_absolute() || path.has_root() {
        bail!("path pattern {pattern:?} must be relative to the configuration root");
    }
    let mut parts = Vec::new();
    for part in pattern.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => bail!("path pattern {pattern:?} must stay within the configuration root"),
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        bail!("path pattern {pattern:?} must name a path");
    }
    Ok(parts.join("/"))
}

pub(super) fn relative_path(context: &Context<'_, '_, '_>) -> Option<String> {
    let path = super::absolute(context.path);
    let root = super::absolute(context.root);
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<_> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect();
    Some(parts.join("/"))
}

pub(super) fn dotted_pattern(pattern: &str) -> Result<Regex> {
    static SYNTAX: std::sync::OnceLock<Result<Regex, regex::Error>> = std::sync::OnceLock::new();
    let valid = SYNTAX
        .get_or_init(|| {
            Regex::new(r"\A[\p{XID_Continue}*?\[\]!-]+(?:\.[\p{XID_Continue}*?\[\]!-]+)*\z")
        })
        .as_ref()
        .map_err(Clone::clone)?
        .is_match(pattern);
    if !valid {
        bail!("invalid name pattern: {pattern:?}");
    }
    super::shell_pattern(pattern)
}

pub(super) fn identifier_pattern(pattern: &str) -> Result<Regex> {
    if pattern.contains('.') {
        bail!("invalid identifier pattern: {pattern:?}");
    }
    dotted_pattern(pattern)
}

#[derive(Debug, Default)]
pub(super) struct ContextFilter {
    in_async: Option<bool>,
    decorators: Vec<Regex>,
}

impl ContextFilter {
    pub(super) fn new(in_async: Option<bool>, decorators: &[String]) -> Result<Self> {
        Ok(Self {
            in_async,
            decorators: decorators
                .iter()
                .map(|pattern| dotted_pattern(pattern))
                .collect::<Result<_>>()?,
        })
    }

    pub(super) fn allows<'ast>(
        &self,
        context: &Context<'_, 'ast, '_>,
        ancestors: &[AnyNodeRef<'ast>],
        range: TextRange,
    ) -> bool {
        if let Some(expected) = self.in_async {
            // Class bodies and lambdas run synchronously even inside an `async def`.
            let in_async = enclosing_scopes(ancestors, range).next().is_some_and(
                |scope| matches!(scope, AnyNodeRef::StmtFunctionDef(function) if function.is_async),
            );
            if in_async != expected {
                return false;
            }
        }
        self.decorators.is_empty()
            || enclosing_scopes(ancestors, range).any(|scope| {
                let decorators = match scope {
                    AnyNodeRef::StmtFunctionDef(function) => &function.decorator_list,
                    AnyNodeRef::StmtClassDef(class) => &class.decorator_list,
                    _ => return false,
                };
                decorators
                    .iter()
                    .any(|decorator| self.matches_decorator(context, decorator))
            })
    }

    fn matches_decorator<'ast>(
        &self,
        context: &Context<'_, 'ast, '_>,
        decorator: &'ast ast::Decorator,
    ) -> bool {
        let expression = match &decorator.expression {
            Expr::Call(call) => &*call.func,
            expression => expression,
        };
        let names = [
            super::dotted(expression),
            super::super::typing::resolve(context, expression, &[]),
        ];
        names
            .iter()
            .flatten()
            .any(|name| self.decorators.iter().any(|pattern| pattern.is_match(name)))
    }
}

// Decorators, default values, and annotations evaluate in the surrounding scope, so only a
// definition's body counts as inside it.
fn enclosing_scopes<'a, 'ast>(
    ancestors: &'a [AnyNodeRef<'ast>],
    range: TextRange,
) -> impl Iterator<Item = AnyNodeRef<'ast>> + 'a {
    ancestors.iter().rev().copied().filter(move |node| {
        let body = match node {
            AnyNodeRef::StmtFunctionDef(function) => suite_range(&function.body),
            AnyNodeRef::StmtClassDef(class) => suite_range(&class.body),
            AnyNodeRef::ExprLambda(lambda) => Some(lambda.body.range()),
            _ => None,
        };
        body.is_some_and(|body| body.contains_range(range))
    })
}

fn suite_range(body: &[ast::Stmt]) -> Option<TextRange> {
    Some(TextRange::new(body.first()?.start(), body.last()?.end()))
}
