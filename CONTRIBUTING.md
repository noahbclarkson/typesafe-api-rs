# Contributing

Thanks for taking the time. This is a small, opinionated crate, so a short read
now saves a long review later.

## Getting set up

```sh
git clone https://github.com/noahbclarkson/typesafe-api-rs
cd typesafe-api-rs
cargo test --workspace --all-features
```

You need a stable toolchain with `rustfmt` and `clippy`. The MSRV is 1.88 and CI
enforces it, so avoid APIs newer than that.

Tests that hit the live API are `#[ignore]`d and read `TYPESAFE_API_KEY`. They
are never part of CI on pull requests. To run them:

```sh
TYPESAFE_API_KEY=... cargo test --workspace --all-features -- --ignored
```

## Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

CI runs these plus a feature powerset check, minimal dependency versions,
`cargo-deny`, `cargo-semver-checks`, and a spell check.

## What good looks like here

**Read [`docs/DESIGN.md`](docs/DESIGN.md) first** if the change touches the
public API. It records why things are the way they are, and a pull request that
contradicts it should say so and argue the case.

**The wire format is the contract.** Anything that changes the JSON sent or
accepted needs a test in `crates/typesafe-api/tests/wire.rs`, ideally quoting the
payload from the API reference.

**Errors are for the person reading them at 2am.** An error should say what was
being done, what went wrong, and whether trying again could help. Prefer a
struct variant with named fields over a formatted string.

**Do not hide the wire.** Every convenience must be built from a public,
inspectable lower layer. If a caller cannot get at the `Request` that a helper
builds, the helper is wrong.

**Comments explain why, not what.** The code says what it does. A comment earns
its place by recording a decision, a constraint, or a surprise. Doc comments on
public items are required; running commentary inside functions is not wanted.

**New dependencies need a reason.** Say in the pull request what it replaces,
what it costs in compile time, and why the standard library or an existing
dependency will not do. Anything reachable from the default feature set is held
to a higher bar than something behind a feature flag.

## Commits

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org):

```text
feat(client): honour Retry-After on 429 responses
fix(answer): keep unknown answer kinds instead of dropping them
docs(readme): show the derive layer before the raw one
```

The changelog and version bumps are generated from these by `release-plz`, so a
`feat:` or a `!` breaking marker has real consequences. Squash-merge is the
default; the pull request title becomes the commit.

## Releasing

Maintainers only. `release-plz` opens a release pull request with the version
bump and changelog; merging it publishes to crates.io.

## Code of conduct

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
