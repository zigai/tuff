use std::path::Path;

use ruff_python_ast::{self as ast, AnyNodeRef, Expr, ModModule, PythonVersion};
use ruff_python_semantic::{BindingKind, ScopeId, ScopeKind};
use ruff_python_trivia::leading_indentation;
use ruff_source_file::{LineRanges, find_newline};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::NameResolver;

pub struct Context<'ctx, 'ast, 'src> {
    pub module: &'ast ModModule,
    pub source: &'src str,
    pub path: &'ctx Path,
    pub root: &'ctx Path,
    pub python_version: PythonVersion,
    pub resolver: &'ctx dyn NameResolver,
    pub ancestors: &'ctx [AnyNodeRef<'ast>],
    pub rendered_metrics: Option<&'ctx dyn crate::builtin::spacing::RenderedMetrics>,
}

impl<'ast, 'src> Context<'_, 'ast, 'src> {
    pub fn text(&self, range: TextRange) -> &'src str {
        &self.source[range]
    }

    pub fn qualified_name(&self, expression: &Expr) -> Option<String> {
        let mut head = expression;
        while let Expr::Attribute(attribute) = head {
            head = &attribute.value;
        }
        if let Expr::Name(name) = head
            && self.binding_range(name) != self.resolver.binding_range(name)
        {
            return None;
        }
        self.resolver.qualified_name(expression)
    }

    pub fn binding_range(&self, expression: &ast::ExprName) -> Option<TextRange> {
        let original = self.resolver.binding_range(expression);
        let Some(semantic) = self.resolver.semantic_model() else {
            return original;
        };
        let function = |scope| {
            semantic.scopes.ancestor_ids(scope).find(|id| {
                matches!(
                    semantic.scopes[*id].kind,
                    ScopeKind::Function(_) | ScopeKind::Lambda(_)
                )
            })
        };
        let Some(scope) = self.scope_at(expression.start()) else {
            return original;
        };
        // Ruff retains executed class-body global writes in the class scope.
        semantic.bindings.iter().filter(|binding| {
            (binding.is_global() || binding.is_nonlocal())
                && !matches!(binding.kind, BindingKind::Global(_) | BindingKind::Nonlocal(..))
                && binding.name(self.source) == expression.id.as_str()
                && function(binding.scope) == function(scope)
                && matches!(semantic.scopes[binding.scope].kind, ScopeKind::Class(class) if class.end() < expression.start() && original.is_none_or(|range| range.end() < class.end()))
        }).max_by_key(|binding| binding.start()).map(Ranged::range).or(original)
    }

    pub fn scope_at(&self, offset: TextSize) -> Option<ScopeId> {
        let semantic = self.resolver.semantic_model()?;
        if let Some(reference) = semantic
            .bindings
            .iter()
            .flat_map(|binding| binding.references())
            .map(|id| semantic.reference(id))
            .filter(|reference| reference.range().contains(offset))
            .min_by_key(|reference| reference.range().len())
        {
            return Some(reference.scope_id());
        }
        semantic
            .scopes
            .iter_enumerated()
            .filter_map(|(id, scope)| {
                let range = match scope.kind {
                    ScopeKind::Function(function) => {
                        let annotation = function
                            .parameters
                            .iter()
                            .filter_map(|parameter| parameter.annotation())
                            .chain(function.returns.as_deref())
                            .find(|annotation| annotation.range().contains(offset));
                        if let Some(annotation) = annotation {
                            let parent = semantic.scopes.ancestor_ids(id).nth(1)?;
                            return Some((parent, annotation.range().len()));
                        }
                        TextRange::new(function.body.first()?.start(), function.body.last()?.end())
                    }
                    ScopeKind::Class(class) => {
                        TextRange::new(class.body.first()?.start(), class.body.last()?.end())
                    }
                    ScopeKind::Lambda(lambda) => lambda.body.range(),
                    _ => return None,
                };
                range.contains(offset).then_some((id, range.len()))
            })
            .min_by_key(|(_, length)| *length)
            .map(|(id, _)| id)
            .or(Some(ScopeId::global()))
    }

    pub fn enclosing_function(&self) -> Option<&'ast ast::StmtFunctionDef> {
        self.ancestors.iter().rev().find_map(|node| match node {
            AnyNodeRef::StmtFunctionDef(function) => Some(*function),
            _ => None,
        })
    }

    pub fn enclosing_class(&self) -> Option<&'ast ast::StmtClassDef> {
        self.ancestors.iter().rev().find_map(|node| match node {
            AnyNodeRef::StmtClassDef(class) => Some(*class),
            _ => None,
        })
    }

    pub fn in_class_body(&self) -> bool {
        for node in self.ancestors.iter().rev() {
            match node {
                AnyNodeRef::StmtClassDef(_) => return true,
                AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::ExprLambda(_) => return false,
                _ => {}
            }
        }
        false
    }

    pub fn in_annotation(&self, range: TextRange) -> bool {
        self.ancestors.iter().any(|node| match node {
            AnyNodeRef::Parameter(parameter) => parameter
                .annotation
                .as_ref()
                .is_some_and(|annotation| annotation.range().contains_range(range)),
            AnyNodeRef::StmtAnnAssign(statement) => {
                statement.annotation.range().contains_range(range)
            }
            AnyNodeRef::StmtFunctionDef(function) => function
                .returns
                .as_ref()
                .is_some_and(|annotation| annotation.range().contains_range(range)),
            AnyNodeRef::StmtTypeAlias(statement) => statement.value.range().contains_range(range),
            _ => false,
        })
    }

    pub fn line_start(&self, offset: TextSize) -> TextSize {
        self.source.line_start(offset)
    }

    pub fn line_end(&self, offset: TextSize) -> TextSize {
        self.source.line_end(offset)
    }

    pub fn indentation(&self, offset: TextSize) -> &'src str {
        let start = usize::from(self.line_start(offset));
        let end = usize::from(offset);
        leading_indentation(&self.source[start..end])
    }

    pub fn line_ending(&self) -> &'static str {
        find_newline(self.source).map_or("\n", |(_, ending)| ending.as_str())
    }
}
