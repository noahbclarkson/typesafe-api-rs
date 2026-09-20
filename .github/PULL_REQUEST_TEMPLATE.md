<!--
Thanks for contributing. Keep this short — the diff is the detail.
-->

## What this changes

<!-- One or two sentences. What is different after this merges? -->

## Why

<!-- The problem, or a link to the issue. "Closes #123" is enough when the issue says it. -->

## Public API

<!-- Delete the lines that do not apply. -->

- [ ] No change to the public API
- [ ] Adds to the public API (new items, new variants on a `#[non_exhaustive]` enum)
- [ ] Breaks the public API — describe the migration below

<!-- Migration notes, if the box above is ticked. -->

## Checks

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features` are clean
- [ ] `cargo test --workspace --all-features` passes
- [ ] New behaviour is covered by a test; wire-format changes are covered in `tests/wire.rs`
- [ ] Public items have doc comments, and anything non-obvious has a doc example
- [ ] `CHANGELOG.md` has an entry, or this change is invisible to users
