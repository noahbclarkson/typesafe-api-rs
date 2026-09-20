//! Derive macros for [`typesafe-api`](https://docs.rs/typesafe-api).
//!
//! Depend on `typesafe-api` with the `derive` feature rather than on this
//! crate; the macros generate paths into it.

mod common;
mod evaluation;
mod levels;
mod options;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Derives `Options` on a fieldless enum of Choice options.
#[proc_macro_derive(Options, attributes(options))]
pub fn derive_options(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    options::expand(&input)
        .unwrap_or_else(darling::Error::write_errors)
        .into()
}

/// Derives `Levels` on a fieldless enum of Score levels, lowest first.
#[proc_macro_derive(Levels, attributes(levels))]
pub fn derive_levels(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    levels::expand(&input)
        .unwrap_or_else(darling::Error::write_errors)
        .into()
}

/// Derives `Evaluation` on a struct whose fields are answers.
#[proc_macro_derive(Evaluation, attributes(question, evaluation))]
pub fn derive_evaluation(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    evaluation::expand(&input)
        .unwrap_or_else(darling::Error::write_errors)
        .into()
}
