use ruff_python_ast::Expr;
use ruff_python_semantic::BindingKind;
use ruff_text_size::Ranged;

use crate::Context;

pub(super) fn is_local(context: &Context<'_, '_, '_>, expression: &Expr) -> bool {
    let name = match expression {
        Expr::Name(name) => name,
        Expr::Attribute(attribute) => return is_local(context, &attribute.value),
        _ => return false,
    };
    let Some(semantic) = context.resolver.semantic_model() else {
        return false;
    };
    if context.binding_range(name) != context.resolver.binding_range(name) {
        return true;
    }
    let binding = semantic.resolve_name(name).or_else(|| {
        semantic
            .lookup_symbol_in_scope(name.id.as_str(), context.scope_at(name.start())?, false)
            .binding_id()
    });
    binding.is_some_and(|id| {
        let binding = semantic.binding(id);
        binding.as_any_import().is_none() && !matches!(binding.kind, BindingKind::Builtin)
    })
}

pub(super) fn star_matches(
    context: &Context<'_, '_, '_>,
    symbol: &str,
    name: &str,
    expression: &Expr,
) -> bool {
    symbol.rsplit_once('.').is_some_and(|(module, member)| {
        member == name && super::super::typing::has_star(context, &[module], expression.start())
    })
}
