//! `#[derive(Options)]`.

use darling::{FromDeriveInput, FromVariant};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident};

use crate::common::{RenameRule, doc_comment, err, krate};

#[derive(FromDeriveInput)]
#[darling(attributes(options), supports(enum_unit))]
struct Container {
    ident: Ident,
    data: darling::ast::Data<Variant, darling::util::Ignored>,
    #[darling(default)]
    rename_all: Option<String>,
}

#[derive(FromVariant)]
#[darling(attributes(options), forward_attrs(doc))]
struct Variant {
    ident: Ident,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    name: Option<String>,
    #[darling(default)]
    describe: Option<String>,
    #[darling(default)]
    unknown: bool,
}

pub(crate) fn expand(input: &DeriveInput) -> darling::Result<TokenStream> {
    let container = Container::from_derive_input(input)?;
    let ident = &container.ident;
    let krate = krate();

    let rule = match &container.rename_all {
        Some(value) => RenameRule::parse(value, &input.ident)?,
        None => RenameRule::default(),
    };

    let variants = container.data.take_enum().unwrap_or_default();
    if variants.is_empty() {
        return Err(err(
            &input.ident,
            "an Options enum needs at least one variant; a choice with no options cannot be \
             answered",
        ));
    }

    let mut names = Vec::new();
    let mut descriptions = Vec::new();
    let mut idents = Vec::new();
    let mut fallback: Option<Ident> = None;

    for variant in &variants {
        let name = variant
            .name
            .clone()
            .unwrap_or_else(|| rule.apply(&variant.ident.to_string()));
        if names.contains(&name) {
            return Err(err(
                &variant.ident,
                format!("`{name}` is used by more than one variant; option names must be unique"),
            ));
        }
        if variant.unknown {
            if fallback.is_some() {
                return Err(err(
                    &variant.ident,
                    "only one variant may be marked #[options(unknown)]",
                ));
            }
            fallback = Some(variant.ident.clone());
        }

        descriptions.push(
            variant
                .describe
                .clone()
                .or_else(|| doc_comment(&variant.attrs)),
        );
        names.push(name);
        idents.push(variant.ident.clone());
    }

    let count = names.len();
    let criteria = names.iter().zip(&descriptions).map(|(name, description)| {
        if let Some(text) = description {
            quote!(map.insert(#name.to_owned(), Some(#krate::Entry::text(#text)));)
        } else {
            quote!(map.insert(#name.to_owned(), None);)
        }
    });

    let from_option_arms = names
        .iter()
        .zip(&idents)
        .map(|(name, variant)| quote!(#name => Ok(Self::#variant),));

    let from_option_fallback = if let Some(variant) = &fallback {
        quote!(_ => Ok(Self::#variant),)
    } else {
        quote! {
            other => Err(#krate::UnknownOption {
                answer: other.to_owned(),
                expected: <Self as #krate::Options>::variants().to_vec(),
            }),
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #krate::Options for #ident {
            fn criteria() -> #krate::indexmap::IndexMap<
                ::std::string::String,
                ::core::option::Option<#krate::Entry>,
            > {
                let mut map = #krate::indexmap::IndexMap::with_capacity(#count);
                #(#criteria)*
                map
            }

            fn variants() -> &'static [&'static str] {
                &[#(#names),*]
            }

            fn option_name(&self) -> &'static str {
                match self {
                    #(Self::#idents => #names,)*
                }
            }

            fn from_option(name: &str) -> ::core::result::Result<Self, #krate::UnknownOption> {
                match name {
                    #(#from_option_arms)*
                    #from_option_fallback
                }
            }
        }
    })
}
