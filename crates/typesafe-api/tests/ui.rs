//! Compile-fail tests: a misuse of a derive must say what is wrong and why.
//!
//! Regenerate the expected output with `TRYBUILD=overwrite cargo test --test ui
//! --all-features`. Read the diff before accepting it: these messages are the
//! first thing a user sees.

#![cfg(feature = "derive")]

#[test]
fn derive_misuse_is_explained() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
