//! `#[derive(Levels)]`.

use darling::{FromDeriveInput, FromVariant};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident};

use crate::common::{doc_comment, err, krate};

#[derive(FromDeriveInput)]
#[darling(attributes(levels), supports(enum_unit))]
struct Container {
    ident: Ident,
    data: darling::ast::Data<Variant, darling::util::Ignored>,
}

#[derive(FromVariant)]
#[darling(attributes(levels), forward_attrs(doc))]
struct Variant {
    ident: Ident,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    describe: Option<String>,
}

pub(crate) fn expand(input: &DeriveInput) -> darling::Result<TokenStream> {
    let container = Container::from_derive_input(input)?;
    let ident = &container.ident;
    let krate = krate();

    let variants = container.data.take_enum().unwrap_or_default();
    if variants.len() < 2 {
        return Err(err(
            &input.ident,
            "a Levels enum needs at least two variants; a one-level scale has nothing to weigh, \
             so ask a Noul instead",
        ));
    }
    if variants.len() > 10 {
        return Err(err(
            &input.ident,
            format!(
                "a Levels enum may declare at most 10 levels; this one declares {}",
                variants.len()
            ),
        ));
    }

    let mut idents = Vec::new();
    let mut descriptions = Vec::new();

    for variant in &variants {
        let description = variant
            .describe
            .clone()
            .or_else(|| doc_comment(&variant.attrs));
        let Some(description) = description else {
            return Err(err(
                &variant.ident,
                "every level needs a description: add a doc comment or \
                 #[levels(describe = \"...\")]. The model sees only the descriptions, so a level \
                 without one has nothing to match against",
            ));
        };
        descriptions.push(description);
        idents.push(variant.ident.clone());
    }

    let count = idents.len();
    let numbers: Vec<u32> = (0..u32::try_from(count).unwrap_or(u32::MAX)).collect();

    Ok(quote! {
        #[automatically_derived]
        impl #krate::Levels for #ident {
            fn criteria() -> ::std::vec::Vec<#krate::Entry> {
                ::std::vec![#(#krate::Entry::text(#descriptions)),*]
            }

            fn count() -> usize {
                #count
            }

            fn level(&self) -> u32 {
                match self {
                    #(Self::#idents => #numbers,)*
                }
            }

            fn from_level(level: u32) -> ::core::option::Option<Self> {
                match level {
                    #(#numbers => ::core::option::Option::Some(Self::#idents),)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}
