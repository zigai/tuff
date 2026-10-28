use std::collections::HashSet;

use heck::{ToKebabCase, ToUpperCamelCase};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Expr, Ident, ItemFn, Pat, Stmt, parse_macro_input, parse_quote};
use tuff_lint::RuleDefinition;

mod codes;

struct NativeRule {
    definition: &'static RuleDefinition,
    ident: Ident,
    suffix: String,
}

fn native_rules() -> syn::Result<Vec<NativeRule>> {
    let mut names = HashSet::new();
    let mut codes = HashSet::new();
    let mut result = Vec::new();
    for definition in tuff_lint::builtin::definitions() {
        if !names.insert(definition.name) || !codes.insert(definition.code) {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "duplicate native rule name or code: {} ({})",
                    definition.name, definition.code
                ),
            ));
        }
        let Some(suffix) = definition.code.strip_prefix("TUF").filter(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        }) else {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "native rule `{}` needs a TUF-prefixed numeric code",
                    definition.name
                ),
            ));
        };
        let type_name = definition.name.to_upper_camel_case();
        let directive =
            tuff_lint::suppression::parse_comment(&format!("# tuff: ignore[{}]", definition.name))
                .map_err(|error| syn::Error::new(Span::call_site(), error.to_string()))?;
        let valid_name = directive.is_some_and(|directive| {
            directive
                .names
                .is_some_and(|names| names.len() == 1 && names[0] == definition.name)
        });
        if !valid_name || type_name.to_kebab_case() != definition.name {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "native rule name `{}` must be canonical lower kebab-case",
                    definition.name
                ),
            ));
        }
        if codes.iter().any(|code| {
            *code != definition.code
                && (code.starts_with(definition.code) || definition.code.starts_with(*code))
        }) {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "native rule code `{}` overlaps another rule's prefix",
                    definition.code
                ),
            ));
        }
        let ident = syn::parse_str(&type_name)?;
        result.push(NativeRule {
            definition,
            ident,
            suffix: suffix.to_owned(),
        });
    }
    Ok(result)
}

#[proc_macro_attribute]
pub fn register_codes(attributes: TokenStream, input: TokenStream) -> TokenStream {
    if !attributes.is_empty() {
        return syn::Error::new(Span::call_site(), "register_codes takes no arguments")
            .into_compile_error()
            .into();
    }
    let mut function = parse_macro_input!(input as ItemFn);
    match add_codes(&mut function) {
        Ok(replacements) => quote!(#function #replacements).into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn add_codes(function: &mut ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let Some(Stmt::Expr(Expr::Call(call), _)) = function.block.stmts.last_mut() else {
        return Err(syn::Error::new_spanned(
            function,
            "expected Some(match ...)",
        ));
    };
    let Some(Expr::Match(expression)) = call.args.first_mut() else {
        return Err(syn::Error::new_spanned(call, "expected Some(match ...)"));
    };
    let index = expression
        .arms
        .iter()
        .position(|arm| matches!(arm.pat, Pat::Wild(_)))
        .unwrap_or(expression.arms.len());
    let native = native_rules()?;
    let replacements = codes::replacements(&expression.arms, &native)?;
    let mut arms = Vec::new();
    for rule in native {
        let suffix = rule.suffix;
        let ident = rule.ident;
        arms.push(parse_quote!((Tuff, #suffix) => rules::tuff::#ident,));
    }
    expression.arms.splice(index..index, arms);
    Ok(replacements)
}

#[proc_macro]
pub fn violations(input: TokenStream) -> TokenStream {
    if !input.is_empty() {
        return syn::Error::new(Span::call_site(), "violations takes no arguments")
            .into_compile_error()
            .into();
    }
    match expand_violations() {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_violations() -> syn::Result<proc_macro2::TokenStream> {
    let mut types = Vec::new();
    let mut mappings = Vec::new();
    let mut reports = Vec::new();
    let mut rule_ids = Vec::new();
    for rule in native_rules()? {
        let ident = rule.ident;
        let name = rule.definition.name;
        rule_ids.push(quote!(crate::registry::Rule::#ident));
        let explanation = format!("## What it does\n\n{}", rule.definition.explanation);
        let source_file = rule.definition.source_file;
        let source_line = rule.definition.source_line;
        let fix_availability = if rule.definition.fixable {
            quote!(crate::FixAvailability::Sometimes)
        } else {
            quote!(crate::FixAvailability::None)
        };
        types.push(quote! {
            #[doc = #explanation]
            pub struct #ident {
                message: String,
                fix_title: Option<String>,
            }

            impl crate::ViolationMetadata for #ident {
                fn rule() -> crate::registry::Rule { crate::registry::Rule::#ident }
                fn explain() -> &'static str { #explanation }
                fn status() -> crate::codes::RuleStatus { crate::codes::RuleStatus::Stable { since: "0.17.1" } }
                fn category() -> crate::codes::Category { crate::codes::Category::Restriction }
                fn file() -> &'static str { #source_file }
                fn line() -> u32 { #source_line }
            }

            impl crate::Violation for #ident {
                const FIX_AVAILABILITY: crate::FixAvailability = #fix_availability;
                fn message(&self) -> String { self.message.clone() }
                fn fix_title(&self) -> Option<String> { self.fix_title.clone() }
                fn message_formats() -> &'static [&'static str] { &["{message}"] }
            }
        });
        mappings.push(quote!(#name => Some(crate::registry::Rule::#ident),));
        reports.push(quote! {
            #name => {
                let mut diagnostic = context.report_diagnostic(#ident { message, fix_title }, range);
                if let Some(fix) = fix { diagnostic.set_fix(fix); }
            }
        });
    }
    Ok(quote! {
        #(#types)*
        pub(crate) const NATIVE_RULES: &[crate::registry::Rule] = &[#(#rule_ids),*];
        pub(crate) fn rule_for_name(name: &str) -> Option<crate::registry::Rule> {
            match name { #(#mappings)* _ => None }
        }
        pub(crate) fn report(context: &crate::checkers::ast::LintContext<'_>, finding: tuff_lint::Finding) {
            let tuff_lint::Finding { rule, range, message, fix, fix_title } = finding;
            match rule {
                #(#reports)*
                _ => crate::tuff::report_unknown_rule(context, rule, range),
            }
        }
    })
}
