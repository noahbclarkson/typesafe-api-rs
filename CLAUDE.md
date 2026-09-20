# Working in this repo

A Rust client for the TypeSafe System One API. `AGENTS.md` is a symlink to this
file.

## Orientation

| Path | Holds |
| --- | --- |
| `crates/typesafe-api/src/` | the crate; one module per concept |
| `crates/typesafe-api-macros/` | the three derives, behind the `derive` feature |
| `crates/typesafe-api/tests/` | wire format, client, typed layer, blocking, snapshots, compile-fail |
| `crates/typesafe-api/examples/` | runnable programs, each one self-contained |
| `docs/DESIGN.md` | the API surface, the reasoning, the roadmap |

Read `docs/DESIGN.md` before changing anything public. It is the source of
truth for the shape of this crate, and it is kept current.

The crate is layered, and each layer is built from the public layer below it:
wire types (`entry`, `state`, `question`, `answer`, `request`, `response`) →
client (`config`, `retry`, `transport`, `client`, `blocking`) → typed (`typed`,
plus the macros crate) → patterns (`patterns`, `stream`).

## Commands

```sh
cargo test --workspace --all-features       # what CI runs first
cargo clippy --workspace --all-targets --all-features
cargo fmt --all
cargo doc --workspace --all-features --no-deps --open

TRYBUILD=overwrite cargo test --all-features --test ui   # regenerate .stderr
cargo insta accept                                       # accept wire snapshots
```

Live-API tests are `#[ignore]`d and need `TYPESAFE_API_KEY`. Do not run them
without being asked; they cost money.

## House style

- **The wire format is the contract.** Any change to the JSON sent or accepted
  needs a test in `tests/wire.rs` and usually a snapshot in `tests/snapshots.rs`,
  quoting the payload from the API reference at <https://docs.typesafe.ai/api>.
- **Make invalid states unrepresentable** before reaching for a runtime check.
  `Entry` cannot hold a number because the API will not accept one.
- **Never hide the wire.** Every convenience is built from a public lower layer.
  If a caller cannot reach the `Request` a helper builds, the helper is wrong.
- **Errors name the thing that failed**, where it failed, and whether retrying
  could help. Struct variants with named fields, not formatted strings. A
  compile error from a derive is an error message too: say what is wrong and
  what to do instead.
- **No running commentary.** Doc comments on public items are required. Comments
  inside a function are for a decision or a constraint the code cannot state.
- **Dependencies are a cost.** Prefer the standard library or something already
  in the tree. New dependencies belong behind a feature flag unless they are
  load-bearing.
- **MSRV is 1.88.** No newer APIs, no `nightly`.

## Gotchas

- `indexmap` is used everywhere order matters, because requests should be
  byte-reproducible. Do not swap in `HashMap`. It is re-exported as
  `typesafe_api::indexmap` so generated code and hand-written `Options` impls do
  not need it as a direct dependency.
- The Noul criteria keys are literally `true` and `false`, which are Rust
  keywords; the fields are `yes` and `no` with `#[serde(rename)]`.
- Score `legend` and `probabilities` arrive keyed by stringified integers and
  are deserialized into `u32` keys. That is deliberate.
- `Answer::Unknown` exists so a future primitive does not break deserialization.
  Do not "simplify" it away.
- `529 Overloaded` is a real status from this API and is retryable. It is not a
  typo for 502.
- `Option<Option<Duration>>` in the config and call options is three states
  (inherit, set, none), not an accident. It carries an `#[allow]` saying so.
- The macros crate is on **syn 3**, because that is what `darling` uses. Mixing
  syn versions produces confusing type errors about "two different `DeriveInput`".
- `reqwest::blocking` panics inside a runtime, so `tests/blocking.rs` runs the
  mock server on its own runtime and the client on a scoped thread.
- `backon` supplies the delay schedule only. The retry loop, `Retry-After`, the
  deadline, and jitter-as-a-fraction are ours; see `transport::Attempts`.

## Before you hand work back

`cargo fmt --all`, then clippy and tests with `--all-features`, all clean. If
the public API moved, update `docs/DESIGN.md` and `CHANGELOG.md` in the same
change.
