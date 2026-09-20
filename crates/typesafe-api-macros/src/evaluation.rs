//! `#[derive(Evaluation)]`.

use darling::{FromDeriveInput, FromField};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, Type};

use crate::common::{doc_comment, err, generic_of, is_plain, krate};

#[derive(FromDeriveInput)]
#[darling(attributes(evaluation), supports(struct_named))]
struct Container {
    ident: Ident,
    data: darling::ast::Data<darling::util::Ignored, Field>,
}

#[derive(FromField)]
#[darling(attributes(question), forward_attrs(doc))]
struct Field {
    ident: Option<Ident>,
    ty: Type,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    id: Option<String>,
    #[darling(default)]
    instructions: Option<String>,
    #[darling(default)]
    yes: Option<String>,
    #[darling(default)]
    no: Option<String>,
}

/// What a field asks for, worked out from its type.
enum Shape {
    Noul,
    Choice(Type),
    Score(Type),
}

#[allow(clippy::too_many_lines)]
pub(crate) fn expand(input: &DeriveInput) -> darling::Result<TokenStream> {
    let container = Container::from_derive_input(input)?;
    let ident = &container.ident;
    let krate = krate();
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let fields = container
        .data
        .take_struct()
        .map(|fields| fields.fields)
        .unwrap_or_default();
    if fields.is_empty() {
        return Err(err(
            &input.ident,
            "an Evaluation struct needs at least one field; a request with no questions is \
             rejected by the API",
        ));
    }

    let mut question_inserts = Vec::new();
    let mut field_reads = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();

    for field in &fields {
        let name = field.ident.clone().expect("named struct");
        let id = field.id.clone().unwrap_or_else(|| name.to_string());
        if seen_ids.contains(&id) {
            return Err(err(
                &name,
                format!("`{id}` is used by more than one field; question ids must be unique"),
            ));
        }
        seen_ids.push(id.clone());

        let (inner, optional) = match generic_of(&field.ty, "Option") {
            Some(inner) => (inner.clone(), true),
            None => (field.ty.clone(), false),
        };
        let shape = shape_of(&inner, &name)?;

        let instructions = field
            .instructions
            .clone()
            .or_else(|| doc_comment(&field.attrs))
            .ok_or_else(|| {
                err(
                    &name,
                    "every question needs instructions: add a doc comment or \
                     #[question(instructions = \"...\")]",
                )
            })?;

        question_inserts.push(match &shape {
            Shape::Noul => {
                let mut build = quote!(#krate::Noul::new(#instructions));
                if let Some(yes) = &field.yes {
                    build = quote!(#build.yes(#yes));
                }
                if let Some(no) = &field.no {
                    build = quote!(#build.no(#no));
                }
                quote!(map.insert(#id.to_owned(), #krate::Question::from(#build));)
            }
            Shape::Choice(target) => {
                reject_noul_criteria(field, &name, "choice")?;
                quote! {
                    map.insert(#id.to_owned(), #krate::Question::from(#krate::Choice {
                        instructions: ::core::option::Option::Some(#krate::Entry::text(#instructions)),
                        criteria: <#target as #krate::Options>::criteria(),
                    }));
                }
            }
            Shape::Score(target) => {
                reject_noul_criteria(field, &name, "score")?;
                quote! {
                    map.insert(#id.to_owned(), #krate::Question::from(#krate::Score {
                        instructions: ::core::option::Option::Some(#krate::Entry::text(#instructions)),
                        criteria: <#target as #krate::Levels>::criteria(),
                    }));
                }
            }
        });

        let read = match &shape {
            Shape::Noul => quote!(*response.noul(#id)?),
            Shape::Choice(target) => quote!(response.choice_as::<#target>(#id)?),
            Shape::Score(target) => quote!(response.score_as::<#target>(#id)?),
        };
        let optional_read = match &shape {
            Shape::Noul => quote!(response.get(#id).and_then(#krate::Answer::as_noul).copied()),
            Shape::Choice(target) => {
                quote!(match response.get(#id) {
                    ::core::option::Option::Some(_) => {
                        ::core::option::Option::Some(response.choice_as::<#target>(#id)?)
                    }
                    ::core::option::Option::None => ::core::option::Option::None,
                })
            }
            Shape::Score(target) => {
                quote!(match response.get(#id) {
                    ::core::option::Option::Some(_) => {
                        ::core::option::Option::Some(response.score_as::<#target>(#id)?)
                    }
                    ::core::option::Option::None => ::core::option::Option::None,
                })
            }
        };

        field_reads.push(if optional {
            quote!(#name: #optional_read,)
        } else {
            quote!(#name: #read,)
        });
    }

    let count = question_inserts.len();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #krate::Evaluation for #ident #type_generics #where_clause {
            fn questions() -> #krate::Questions {
                let mut map = #krate::Questions::with_capacity(#count);
                #(#question_inserts)*
                map
            }

            fn from_response(
                response: &#krate::Response,
            ) -> ::core::result::Result<Self, #krate::Error> {
                ::core::result::Result::Ok(Self {
                    #(#field_reads)*
                })
            }
        }
    })
}

fn shape_of(ty: &Type, field: &Ident) -> darling::Result<Shape> {
    if is_plain(ty, "NoulAnswer") {
        return Ok(Shape::Noul);
    }
    if let Some(target) = generic_of(ty, "ChoiceOf") {
        return Ok(Shape::Choice(target.clone()));
    }
    if let Some(target) = generic_of(ty, "ScoreOf") {
        return Ok(Shape::Score(target.clone()));
    }
    Err(err(
        field,
        "unsupported field type: an Evaluation field must be NoulAnswer, ChoiceOf<T>, \
         ScoreOf<T>, or Option<_> of one of those",
    ))
}

fn reject_noul_criteria(field: &Field, name: &Ident, kind: &str) -> darling::Result<()> {
    if field.yes.is_some() || field.no.is_some() {
        return Err(err(
            name,
            format!(
                "`yes` and `no` describe the two outcomes of a noul; a {kind} takes its criteria \
                 from its type instead"
            ),
        ));
    }
    Ok(())
}
