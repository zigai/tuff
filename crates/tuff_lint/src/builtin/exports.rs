use std::collections::{HashMap, HashSet};

use anyhow::Result;
use ruff_diagnostics::{Edit, Fix};
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal};
use ruff_python_ast::{self as ast, AnyNodeRef, Expr, NodeKind, Stmt};
use ruff_text_size::{Ranged, TextRange};
use serde::Deserialize;

use super::style::{
    call_arguments, full_end, has_comment, is_name, sequence, simple_line, targets,
};
use crate::{
    Context, Finding, LintRule, RuleDefinition, RuleOptions, parse_options, reject_options,
};

pub static RULES: &[RuleDefinition] = &[
    crate::native_rule! { name: "module-all-at-bottom", code: "TUF015", explanation: "Place module export declarations after runtime definitions.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: true, create: all_bottom },
    crate::native_rule! { name: "no-underscore-all-exports", code: "TUF016", explanation: "Do not export underscore-prefixed symbols through module __all__.", node_kinds: &[NodeKind::ModModule], checks_comments: false, fixable: false, create: private_exports },
];

#[derive(Debug)]
struct AllBottom;
fn all_bottom(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    reject_options(o)?;
    Ok(Box::new(AllBottom))
}
fn all_assignment(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Assign(s) => s.targets.iter().any(|e| is_name(e, "__all__")),
        Stmt::AnnAssign(s) => is_name(&s.target, "__all__"),
        _ => false,
    }
}
fn all_statement(stmt: &Stmt) -> bool {
    if all_assignment(stmt) {
        return true;
    }
    match stmt {
        Stmt::AugAssign(s) => s.op == ast::Operator::Add && is_name(&s.target, "__all__"),
        Stmt::Expr(s) => {
            matches!(&*s.value, Expr::Call(call) if matches!(&*call.func, Expr::Attribute(a) if is_name(&a.value, "__all__") && matches!(a.attr.as_str(), "append" | "extend" | "insert")))
        }
        _ => false,
    }
}
impl LintRule for AllBottom {
    fn check(&self, node: AnyNodeRef<'_>, c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ModModule(module) = node else {
            return;
        };
        struct Entries<'a>(Vec<(&'a Stmt, bool)>);
        impl<'a> SourceOrderVisitor<'a> for Entries<'a> {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                match stmt {
                    Stmt::If(_)
                    | Stmt::For(_)
                    | Stmt::While(_)
                    | Stmt::With(_)
                    | Stmt::Try(_)
                    | Stmt::Match(_) => {
                        ruff_python_ast::visitor::source_order::walk_stmt(self, stmt)
                    }
                    _ => self.0.push((stmt, all_statement(stmt))),
                }
            }
        }
        let mut entries = Entries(Vec::new());
        entries.visit_body(&module.body);
        let Some(last) = entries.0.iter().rposition(|(_, all)| !all) else {
            return;
        };
        let misplaced: Vec<_> = entries.0[..last]
            .iter()
            .filter(|(_, all)| *all)
            .map(|(s, _)| *s)
            .collect();
        if misplaced.is_empty() {
            return;
        }
        if let Some(edit) = move_all(module, c) {
            out.push(
                Finding::new(
                    "module-all-at-bottom",
                    misplaced[0].range(),
                    "Define module __all__ at the bottom of the file.",
                )
                .with_fix(Fix::safe_edit(edit), "Move __all__ to the bottom"),
            );
        } else {
            out.extend(misplaced.into_iter().map(|s| {
                Finding::new(
                    "module-all-at-bottom",
                    s.range(),
                    "Define module __all__ at the bottom of the file.",
                )
            }));
        }
    }
}
fn move_all(module: &ast::ModModule, c: &Context<'_, '_, '_>) -> Option<Edit> {
    let indices: Vec<_> = module
        .body
        .iter()
        .enumerate()
        .filter(|(_, s)| all_assignment(s) && simple_line(c, s))
        .map(|(i, _)| i)
        .collect();
    if indices.len() != 1 {
        return None;
    }
    let index = indices[0];
    let all = &module.body[index];
    let trailing = &module.body[index + 1..];
    if trailing.is_empty() {
        return None;
    }
    let value = match all {
        Stmt::Assign(s) => &*s.value,
        Stmt::AnnAssign(s) => s.value.as_deref()?,
        _ => return None,
    };
    if !matches!(value, Expr::List(_) | Expr::Tuple(_))
        || !sequence(value)?
            .iter()
            .all(|e| matches!(e, Expr::StringLiteral(s) if s.as_single_part_string().is_some()))
    {
        return None;
    }
    let future = module.body.iter().any(|s| matches!(s, Stmt::ImportFrom(i) if i.level == 0 && i.module.as_ref().is_some_and(|m| m == "__future__") && i.names.iter().any(|n| n.name.as_str() == "annotations")));
    for stmt in trailing {
        let Stmt::FunctionDef(f) = stmt else {
            return None;
        };
        if f.name.as_str() == "__all__"
            || !f.decorator_list.is_empty()
            || f.type_params.is_some()
            || (!future && f.returns.is_some())
            || f.parameters
                .iter()
                .any(|p| p.default().is_some() || (!future && p.annotation().is_some()))
        {
            return None;
        }
    }
    let start = if index == 0 {
        c.line_start(all.start())
    } else {
        full_end(c, module.body[index - 1].end())
    };
    if has_comment(c.text(TextRange::new(start, all.start()))) {
        return None;
    }
    let all_end = full_end(c, all.end());
    let first_start = c.line_start(trailing[0].start());
    if first_start < all_end {
        return None;
    }
    let trailing_start = if index == 0 {
        if has_comment(c.text(TextRange::new(all_end, first_start))) {
            return None;
        }
        first_start
    } else {
        all_end
    };
    let end = full_end(c, trailing.last()?.end());
    let mut replacement = c.text(TextRange::new(trailing_start, end)).to_string();
    if !replacement.ends_with('\n') {
        replacement.push_str(c.line_ending());
    }
    replacement.push_str(c.line_ending());
    replacement.push_str(c.text(TextRange::new(c.line_start(all.start()), all_end)));
    if usize::from(end) == c.source.len() && !c.source.ends_with('\n') {
        replacement.truncate(replacement.len().saturating_sub(c.line_ending().len()));
    }
    Some(Edit::range_replacement(
        replacement,
        TextRange::new(start, end),
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PrivateExports {
    allowed_exports: Vec<String>,
    allow_dunder_exports: bool,
}
fn private_exports(o: &RuleOptions) -> Result<Box<dyn LintRule>> {
    Ok(Box::new(parse_options::<PrivateExports>(o)?))
}
const CONSTRUCTORS: &[&str] = &["frozenset", "list", "set", "tuple"];
#[derive(Clone)]
struct Known {
    names: Vec<String>,
    kind: String,
}
#[derive(Default)]
struct Aliases {
    aliases: HashMap<String, usize>,
    values: Vec<Known>,
    shadowed: HashSet<String>,
}
fn string(e: &Expr) -> Option<String> {
    if let Expr::StringLiteral(s) = e {
        Some(s.value.to_string())
    } else {
        None
    }
}
impl Aliases {
    fn known(&self, name: &str) -> Option<&Known> {
        self.aliases.get(name).and_then(|id| self.values.get(*id))
    }
    fn collection(&self, e: &Expr) -> Option<Vec<(TextRange, String)>> {
        match e {
            Expr::Name(n) => Some(
                self.known(n.id.as_str())?
                    .names
                    .iter()
                    .map(|name| (e.range(), name.clone()))
                    .collect(),
            ),
            Expr::Call(call) => {
                let Expr::Name(name) = &*call.func else {
                    return None;
                };
                let args = call_arguments(call);
                if !CONSTRUCTORS.contains(&name.id.as_str())
                    || self.shadowed.contains(name.id.as_str())
                    || args.len() != 1
                    || args[0].1
                {
                    return None;
                }
                self.collection(args[0].0)
            }
            Expr::BinOp(b) if b.op == ast::Operator::Add => {
                let mut names = self.collection(&b.left)?;
                names.extend(self.collection(&b.right)?);
                Some(names)
            }
            Expr::List(_) | Expr::Set(_) | Expr::Tuple(_) => {
                let mut names = Vec::new();
                for element in sequence(e)? {
                    if let Expr::Starred(s) = element {
                        if let Some(nested) = self.collection(&s.value) {
                            names.extend(nested);
                        }
                    } else if let Some(name) = string(element) {
                        names.push((element.range(), name));
                    }
                }
                Some(names)
            }
            _ => None,
        }
    }
    fn kind(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Name(n) => self.known(n.id.as_str()).map(|k| k.kind.clone()),
            Expr::List(_) => Some("list".into()),
            Expr::Tuple(_) => Some("tuple".into()),
            Expr::Set(_) => Some("set".into()),
            Expr::Call(call) => {
                let Expr::Name(name) = &*call.func else {
                    return None;
                };
                (CONSTRUCTORS.contains(&name.id.as_str())
                    && !self.shadowed.contains(name.id.as_str()))
                .then(|| name.id.to_string())
            }
            Expr::BinOp(b) if b.op == ast::Operator::Add => {
                let left = self.kind(&b.left)?;
                let right = self.kind(&b.right)?;
                (left == right && matches!(left.as_str(), "list" | "tuple")).then_some(left)
            }
            _ => None,
        }
    }
    fn forget(&mut self, target: &Expr) {
        for name in targets(target) {
            self.aliases.remove(name.id.as_str());
        }
    }
    fn invalidate(&mut self, id: usize) {
        self.aliases.retain(|_, candidate| *candidate != id);
    }
    fn mutated_target(&mut self, target: &Expr) {
        if let Expr::Subscript(s) = target {
            if let Expr::Name(n) = &*s.value {
                if let Some(id) = self.aliases.get(n.id.as_str()).copied() {
                    self.invalidate(id);
                }
            }
        }
    }
    fn bind(&mut self, name: &str) {
        self.aliases.remove(name);
        if CONSTRUCTORS.contains(&name) {
            self.shadowed.insert(name.to_string());
        }
    }
    fn unbind(&mut self, name: &str) {
        self.aliases.remove(name);
        self.shadowed.remove(name);
    }
    fn store(&mut self, name: &str, names: Vec<String>, kind: String) {
        let id = self.values.len();
        self.values.push(Known { names, kind });
        self.aliases.insert(name.to_string(), id);
    }
    fn remember(&mut self, target: &Expr, value: &Expr) {
        if matches!(target, Expr::List(_) | Expr::Tuple(_)) {
            if let (Some(ts), Some(vs)) = (
                sequence(target),
                sequence(value).filter(|_| matches!(value, Expr::List(_) | Expr::Tuple(_))),
            ) {
                if ts.len() == vs.len()
                    && !ts.iter().chain(vs).any(|e| matches!(e, Expr::Starred(_)))
                {
                    for (t, v) in ts.iter().zip(vs) {
                        self.remember(t, v);
                    }
                    return;
                }
            }
            self.forget(target);
            return;
        }
        let Expr::Name(name) = target else {
            return;
        };
        let name = name.id.as_str();
        if name == "__all__" {
            return;
        }
        if let Some(exports) = self.collection(value) {
            if let Expr::Name(n) = value {
                if let Some(id) = self.aliases.get(n.id.as_str()).copied() {
                    self.aliases.insert(name.to_string(), id);
                }
            } else {
                let kind = self.kind(value).unwrap_or_else(|| "unknown".into());
                self.store(
                    name,
                    exports.into_iter().map(|(_, name)| name).collect(),
                    kind,
                );
            }
        } else {
            self.aliases.remove(name);
        }
        if CONSTRUCTORS.contains(&name) {
            self.shadowed.insert(name.to_string());
        }
    }
    fn augmented(&mut self, target: &Expr, value: &Expr) {
        let Expr::Name(name) = target else {
            return;
        };
        let Some(id) = self.aliases.get(name.id.as_str()).copied() else {
            return;
        };
        let Some(exports) = self.collection(value) else {
            self.invalidate(id);
            return;
        };
        let added: Vec<_> = exports.into_iter().map(|(_, name)| name).collect();
        match self.values[id].kind.as_str() {
            "list" => self.values[id].names.extend(added),
            "tuple" => {
                let mut names = self.values[id].names.clone();
                names.extend(added);
                self.store(name.id.as_str(), names, "tuple".into());
            }
            _ => self.invalidate(id),
        }
    }
    fn mutate(&mut self, call: &ast::ExprCall) {
        let Expr::Attribute(attr) = &*call.func else {
            return;
        };
        let Expr::Name(alias) = &*attr.value else {
            return;
        };
        let Some(id) = self.aliases.get(alias.id.as_str()).copied() else {
            return;
        };
        if self.values[id].kind != "list" {
            self.invalidate(id);
            return;
        }
        let method = attr.attr.as_str();
        let args = call_arguments(call);
        let argument = match method {
            "append" | "extend" if args.len() == 1 && !args[0].1 => Some(args[0].0),
            "insert" if args.len() == 2 && args.iter().all(|(_, keyword)| !keyword) => {
                Some(args[1].0)
            }
            _ => None,
        };
        let Some(argument) = argument else {
            self.invalidate(id);
            return;
        };
        let exports = if method == "append" {
            string(argument).map(|s| vec![(argument.range(), s)])
        } else {
            self.collection(argument)
        };
        let Some(exports) = exports else {
            return;
        };
        let added = exports.into_iter().map(|(_, name)| name);
        if method == "insert" {
            let mut names: Vec<_> = added.collect();
            names.extend(self.values[id].names.clone());
            self.values[id].names = names;
        } else {
            self.values[id].names.extend(added);
        }
    }
}
impl PrivateExports {
    fn report(&self, aliases: &Aliases, expression: &Expr, single: bool, out: &mut Vec<Finding>) {
        let names = if let Some(name) = string(expression) {
            vec![(expression.range(), name)]
        } else if single {
            vec![]
        } else {
            aliases.collection(expression).unwrap_or_default()
        };
        for (range, name) in names {
            if !name.starts_with('_')
                || self.allowed_exports.contains(&name)
                || (self.allow_dunder_exports && name.starts_with("__") && name.ends_with("__"))
            {
                continue;
            }
            out.push(Finding::new("no-underscore-all-exports", range, format!("Do not export underscore-prefixed symbol '{name}' in __all__. Either remove it from __all__ or rename it to be public.")));
        }
    }
}
impl LintRule for PrivateExports {
    fn check(&self, node: AnyNodeRef<'_>, _c: &Context<'_, '_, '_>, out: &mut Vec<Finding>) {
        let AnyNodeRef::ModModule(module) = node else {
            return;
        };
        struct Exports<'a> {
            rule: &'a PrivateExports,
            aliases: Aliases,
            out: &'a mut Vec<Finding>,
        }
        impl<'a> SourceOrderVisitor<'a> for Exports<'_> {
            fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
                match node {
                    AnyNodeRef::StmtClassDef(s) => {
                        self.aliases.bind(s.name.as_str());
                        return TraversalSignal::Skip;
                    }
                    AnyNodeRef::StmtFunctionDef(s) => {
                        self.aliases.bind(s.name.as_str());
                        return TraversalSignal::Skip;
                    }
                    AnyNodeRef::StmtAssign(s) => {
                        if s.targets.iter().any(|t| is_name(t, "__all__")) {
                            self.rule.report(&self.aliases, &s.value, false, self.out);
                        }
                        for t in &s.targets {
                            self.aliases.mutated_target(t);
                        }
                        for t in &s.targets {
                            self.aliases.remember(t, &s.value);
                        }
                    }
                    AnyNodeRef::StmtAnnAssign(s) => {
                        if is_name(&s.target, "__all__") {
                            if let Some(v) = &s.value {
                                self.rule.report(&self.aliases, v, false, self.out);
                            }
                        } else if let Some(v) = &s.value {
                            self.aliases.remember(&s.target, v);
                        } else {
                            self.aliases.forget(&s.target);
                        }
                    }
                    AnyNodeRef::StmtAugAssign(s) => {
                        if s.op == ast::Operator::Add && is_name(&s.target, "__all__") {
                            self.rule.report(&self.aliases, &s.value, false, self.out);
                        }
                        if s.op == ast::Operator::Add && matches!(&*s.target, Expr::Name(_)) {
                            self.aliases.augmented(&s.target, &s.value);
                        } else {
                            self.aliases.forget(&s.target);
                        }
                    }
                    AnyNodeRef::ExprCall(call) => {
                        let args = call_arguments(call);
                        let all = match &*call.func {
                            Expr::Attribute(a)
                                if is_name(&a.value, "__all__")
                                    && args.iter().all(|(_, keyword)| !keyword) =>
                            {
                                match a.attr.as_str() {
                                    "append" | "extend" if args.len() == 1 => {
                                        Some((args[0].0, a.attr.as_str() != "extend"))
                                    }
                                    "insert" if args.len() == 2 => Some((args[1].0, true)),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some((expression, single)) = all {
                            self.rule
                                .report(&self.aliases, expression, single, self.out);
                        } else {
                            self.aliases.mutate(call);
                        }
                    }
                    AnyNodeRef::StmtImport(s) => {
                        for alias in &s.names {
                            self.aliases.bind(alias.asname.as_ref().map_or_else(
                                || alias.name.as_str().split('.').next().unwrap_or(""),
                                |n| n.as_str(),
                            ));
                        }
                    }
                    AnyNodeRef::StmtImportFrom(s) => {
                        let builtins = s.module.as_ref().is_some_and(|m| m == "builtins");
                        if s.names.iter().any(|n| n.name.as_str() == "*") {
                            self.aliases.aliases.clear();
                            if !builtins {
                                self.aliases
                                    .shadowed
                                    .extend(CONSTRUCTORS.iter().map(|s| (*s).to_string()));
                            }
                        } else {
                            for alias in &s.names {
                                let bound = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                                if builtins
                                    && bound == alias.name.as_str()
                                    && CONSTRUCTORS.contains(&bound)
                                {
                                    self.aliases.unbind(bound);
                                } else {
                                    self.aliases.bind(bound);
                                }
                            }
                        }
                    }
                    AnyNodeRef::StmtDelete(s) => {
                        for t in &s.targets {
                            for name in targets(t) {
                                self.aliases.unbind(name.id.as_str());
                            }
                        }
                    }
                    _ => {}
                }
                TraversalSignal::Traverse
            }
        }
        AnyNodeRef::ModModule(module).visit_source_order(&mut Exports {
            rule: self,
            aliases: Aliases::default(),
            out,
        });
    }
}
