//! Helpers shared by the three derives.

use heck::{ToKebabCase, ToLowerCamelCase, ToPascalCase, ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::TokenStream;
use quote::ToTokens;
use quote::quote;
use syn::{Attribute, Expr, Lit, Meta, Type};

/// A `darling` error carrying the span of the item it is about.
pub(crate) fn err(at: &impl ToTokens, message: impl std::fmt::Display) -> darling::Error {
    darling::Error::custom(message).with_span(at)
}

/// The path to the runtime crate, so generated code works wherever it lands.
pub(crate) fn krate() -> TokenStream {
    quote!(::typesafe_api)
}

/// Joins a run of `///` lines into one description, preserving paragraphs.
///
/// A blank doc line separates paragraphs; everything else folds into one line,
/// so a wrapped comment reaches the model as a single sentence.
pub(crate) fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for attr in attrs {
        let Meta::NameValue(pair) = &attr.meta else {
            continue;
        };
        if !pair.path.is_ident("doc") {
            continue;
        }
        let Expr::Lit(literal) = &pair.value else {
            continue;
        };
        let Lit::Str(text) = &literal.lit else {
            continue;
        };

        let line = text.value();
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line.to_owned());
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }

    let joined = paragraphs.join("\n\n");
    (!joined.is_empty()).then_some(joined)
}

/// The case conventions `rename_all` accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RenameRule {
    #[default]
    SnakeCase,
    KebabCase,
    CamelCase,
    PascalCase,
    LowerCase,
    UpperCase,
    ShoutySnakeCase,
    Verbatim,
}

impl RenameRule {
    pub(crate) fn parse(value: &str, at: &impl ToTokens) -> darling::Result<Self> {
        Ok(match value {
            "snake_case" => Self::SnakeCase,
            "kebab-case" => Self::KebabCase,
            "camelCase" => Self::CamelCase,
            "PascalCase" => Self::PascalCase,
            "lowercase" => Self::LowerCase,
            "UPPERCASE" => Self::UpperCase,
            "SCREAMING_SNAKE_CASE" => Self::ShoutySnakeCase,
            "verbatim" => Self::Verbatim,
            other => {
                return Err(err(
                    at,
                    format!(
                        "unknown rename rule `{other}`; expected one of snake_case, kebab-case, \
                         camelCase, PascalCase, lowercase, UPPERCASE, SCREAMING_SNAKE_CASE, \
                         verbatim"
                    ),
                ));
            }
        })
    }

    pub(crate) fn apply(self, name: &str) -> String {
        match self {
            Self::SnakeCase => name.to_snake_case(),
            Self::KebabCase => name.to_kebab_case(),
            Self::CamelCase => name.to_lower_camel_case(),
            Self::PascalCase => name.to_pascal_case(),
            Self::LowerCase => name.to_lowercase(),
            Self::UpperCase => name.to_uppercase(),
            Self::ShoutySnakeCase => name.to_shouty_snake_case(),
            Self::Verbatim => name.to_owned(),
        }
    }
}

/// Splits `Wrapper<Inner>` into its name and its first type argument.
pub(crate) fn generic_of<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// Whether a type path ends in `name` with no generic arguments.
pub(crate) fn is_plain(ty: &Type, name: &str) -> bool {
    let Type::Path(path) = ty else { return false };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name && segment.arguments.is_empty())
}
