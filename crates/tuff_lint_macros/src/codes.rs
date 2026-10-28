use std::collections::HashMap;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Arm, Expr, Item, Lit, Meta, Pat};

use crate::NativeRule;

pub(super) fn replacements(arms: &[Arm], native: &[NativeRule]) -> syn::Result<TokenStream> {
    let prefixes = prefixes()?;
    let mut existing = HashMap::new();
    for arm in arms {
        let (Pat::Tuple(pattern), Expr::Path(body)) = (&arm.pat, arm.body.as_ref()) else {
            continue;
        };
        let (Some(Pat::Ident(linter)), Some(Pat::Lit(code))) =
            (pattern.elems.first(), pattern.elems.get(1))
        else {
            continue;
        };
        let (Lit::Str(code), Some(ident)) = (&code.lit, body.path.segments.last()) else {
            continue;
        };
        let Some(prefix) = prefixes.get(&linter.ident.to_string()) else {
            return Err(syn::Error::new_spanned(
                linter,
                "linter namespace has no prefix",
            ));
        };
        existing.insert(format!("{prefix}{}", code.value()), ident.ident.clone());
    }
    let mut replacements = Vec::new();
    for rule in native {
        if rule.definition.replaces.is_empty() {
            continue;
        }
        let mut replaced = Vec::new();
        for code in rule.definition.replaces {
            if code.starts_with("TUF") {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "native replacements must name Ruff checks, not other Tuff rules",
                ));
            }
            let Some(ident) = existing.get(*code) else {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "native rule `{}` replaces unknown check `{code}` ({}:{})",
                        rule.definition.name,
                        rule.definition.source_file,
                        rule.definition.source_line
                    ),
                ));
            };
            replaced.push(quote!(Rule::#ident));
        }
        let ident = &rule.ident;
        let name = rule.definition.name;
        let versions = versions(rule.definition.versions);
        replacements.push(quote! {
            crate::tuff::RuleReplacement {
                rule: Rule::#ident,
                name: #name,
                replaced: &[#(#replaced),*],
                versions: #versions,
            }
        });
    }
    Ok(quote! {
        pub(crate) const TUFF_REPLACEMENTS: &[crate::tuff::RuleReplacement] = &[#(#replacements),*];
    })
}

fn versions(range: tuff_lint::VersionRange) -> TokenStream {
    let minimum = version(range.minimum);
    let before = version(range.before);
    quote!(tuff_lint::VersionRange { minimum: #minimum, before: #before })
}

fn version(version: Option<ruff_python_ast::PythonVersion>) -> TokenStream {
    match version {
        Some(version) => {
            let major = version.major;
            let minor = version.minor;
            quote!(Some(ruff_python_ast::PythonVersion { major: #major, minor: #minor }))
        }
        None => quote!(None),
    }
}

fn prefixes() -> syn::Result<HashMap<String, String>> {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ruff_linter/src/registry.rs"
    ));
    let file = syn::parse_file(source)?;
    let Some(linter) = file.items.iter().find_map(|item| match item {
        Item::Enum(item) if item.ident == "Linter" => Some(item),
        _ => None,
    }) else {
        return Err(syn::Error::new(
            Span::call_site(),
            "cannot find Ruff linter namespaces",
        ));
    };
    let mut result = HashMap::new();
    for variant in &linter.variants {
        let mut prefixes = Vec::new();
        for attribute in &variant.attrs {
            if !attribute.path().is_ident("prefix") {
                continue;
            }
            if let Meta::NameValue(value) = &attribute.meta
                && let Expr::Lit(literal) = &value.value
                && let Lit::Str(prefix) = &literal.lit
            {
                prefixes.push(prefix.value());
            }
        }
        let prefix = match prefixes.as_slice() {
            [prefix] => prefix.clone(),
            [] => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "missing linter namespace prefix",
                ));
            }
            _ => String::new(),
        };
        result.insert(variant.ident.to_string(), prefix);
    }
    Ok(result)
}
