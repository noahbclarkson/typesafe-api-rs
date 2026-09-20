# Design

What this SDK is, the surface it exposes, and why each decision went the way it
did. Read this before proposing a change to the public API.

## The upstream model in one page

The API is a single endpoint plus a model list.

```http
POST https://api.typesafe.ai/v1/systemone
GET  https://api.typesafe.ai/v1/models
Authorization: Bearer <API_KEY>
```

One request carries one **state** (the content to judge) and a map of named
**questions**. Every question is evaluated independently and in parallel against
that state, and one **answer** comes back under each name you chose. Names are
never shown to the model.

There are three question types:

| Type | Criteria | Answer |
| --- | --- | --- |
| `noul` | optional `{true, false}` descriptions | `noul: f64` in 0..=1 |
| `choice` | map of option name to description | `choice`, `probabilities`, `confidence` |
| `score` | ordered array of level descriptions | `score`, `probabilities`, `legend`, `confidence` |

`instructions` and every description accept a string, a JSON object, or a JSON
array — never a bare number or boolean. The docs call this `EntryType`.

Documented limits: 255 options per Choice, 2 to 10 levels per Score, 64k tokens
per request, 32k for the state plus the longest single question. `429` and `529`
are retryable and may carry `Retry-After`.

## What Rust adds

The Python and JavaScript SDKs stop at "typed request objects, dictionary of
answers". Python returns `dict[str, Answer]` and you narrow by hand. TypeScript
gets closer with conditional types, but the option names stay strings.

Rust can go further, because the thing the API is modelling — a closed set of
outcomes with descriptions attached — is an enum with doc comments. So the
design goal is:

> Declare the judgement once as a type. That type produces the request and
> receives the answer.

Everything else follows from that, with one constraint: **no layer may hide
what goes on the wire.** Each level is built on the one below it and the lower
level stays public, so `Request` can always be inspected, logged, or sent by
hand.

## Layers

### Layer 0 — the wire types (built)

`State`, `Entry`, `Question`/`Noul`/`Choice`/`Score`, `Request`, `Response`,
`Answer`/`NoulAnswer`/`ChoiceAnswer`/`ScoreAnswer`, `Usage`, `ModelCard`.

Round-tripped against the payloads published in the API reference by
`crates/typesafe-api/tests/wire.rs`. Decisions worth naming:

- **`Entry` has no `Null` variant.** Absence is `Option<Entry>`, so a missing
  description and an explicitly null one are one value, not two.
- **`Entry` cannot hold a number or boolean.** The invalid shapes are not
  representable; `Entry::json` is the fallible door for arbitrary values.
- **Insertion order is preserved** for the questions map, Choice options, and
  object entries, via `indexmap`. Requests are byte-reproducible, which makes
  snapshot tests and diffing possible.
- **`State` takes `impl Serialize`.** Your domain struct is the state. This is
  the single biggest ergonomic win over the other SDKs and it costs one
  `to_value` per request, which also buys shape validation.
- **`ScoreAnswer` keys by `u32`**, not by the `"0"` strings on the wire. Levels
  are numbers; the SDK should say so.
- **Unknown answer kinds are kept**, payload intact, as `Answer::Unknown`. The
  Python SDK logs a warning and drops them. A future primitive stays readable
  here without an upgrade.
- **`Error::Api` is boxed** so `Result<T, Error>` stays small on the happy path.

### Layer 1 — the client (built)

```rust
// Construction
Client::from_env()?;                     // TYPESAFE_API_KEY, and nothing else required
Client::new("sk-...")?;
Client::builder()
    .api_key("sk-...")                   // or leave it to the environment
    .base_url("https://api.typesafe.ai")
    .model(typesafe_api::JEV_LATEST)
    .timeout(Duration::from_secs(10))    // per attempt
    .deadline(Duration::from_secs(30))   // whole call, retries included
    .retry(RetryPolicy::default().max_retries(3))
    .limits(Limits::default())           // or Limits::unbounded() to skip local checks
    .header("x-team", "payments")
    .http_client(my_reqwest_client)      // bring your own pool, proxy, or mock
    .build()?;

// Calls
client.evaluate(state, questions).await?          -> Response
client.evaluate_as::<T, _>(state).await?           -> T  (T: Evaluation)
client.models().await?                            -> Vec<ModelCard>

// Per-call overrides, same knobs as the builder
client.evaluate(state, questions)
    .model("jev-1.13.0")
    .timeout(Duration::from_secs(2))
    .retry(RetryPolicy::none())
    .extra_field("beam_width", 4)       // forward compatibility, merged top level
    .header("x-request-tag", "triage")
    .await?;
```

The returned type is a future that is also a builder, so the common call reads
as one `await` and an override is one more line. `send()` is available for
anyone who prefers it explicit.

Escape hatches, so nothing is trapped behind the ergonomics:

```rust
let request: Request = client.request(state, questions)?;   // exactly what would be sent
let raw: serde_json::Value = client.send_raw(&request).await?;
let response: Response = client.send(&request).await?;
response.request_id                                          // x-typesafe-request-id
```

The blocking client mirrors it behind the `blocking` feature, sharing every type
except the future: `typesafe_api::blocking::Client`.

### Layer 2 — typed options and levels (built)

```rust
#[derive(Options)]
#[options(rename_all = "snake_case")]   // the default
enum Department {
    /// Payment or subscription issues
    Billing,
    /// Bugs or integration problems
    Technical,
    #[options(name = "sales", describe = "Pricing or account questions")]
    Sales,
    /// A request that fits none of the above
    Other,
}
```

The derive writes one `Options` impl:

- `criteria() -> IndexMap<String, Option<Entry>>`, from doc comments or
  `describe`, in declaration order;
- `variants()` and `option_name()`, the names as the API sees them;
- `from_option(&str)`, which turns the returned `choice` back into a variant.

`ChoiceOf<Department>` is built on that impl and carries `.value`,
`.probability(&Department::Sales)`, `.ranked()`, and `.runners_up(0.25)`.

An unknown option is an `UnknownOption` error naming the variants, not a panic.
Marking one variant `#[options(unknown)]` redirects it there instead, for anyone
who would rather keep going.

`#[derive(Levels)]` is the same shape for Score, in declaration order from
level 0, and backs `ScoreOf<T>`: `nearest()`, `normalized()`, `top_level()`,
`probability(&level)`, and `describe(&level)`. Levels are positions rather than
names, so `Levels` takes `describe` but no `name` or `rename_all`.

### Layer 3 — the evaluation struct (built)

```rust
#[derive(Evaluation)]
struct Triage {
    /// The message conveys urgency or time-sensitivity
    is_urgent: NoulAnswer,

    #[question(
        instructions = "Does the message request a refund or credit?",
        yes = "Directly asks for money back or an account credit",
        no = "A billing question with no requested remedy",
    )]
    refund_requested: NoulAnswer,

    /// Which team should handle this?
    department: ChoiceOf<Department>,

    /// How frustrated the customer appears
    frustration: ScoreOf<Frustration>,

    /// An answer the server may not send back yet
    experimental: Option<NoulAnswer>,
}
```

Field name becomes the question id. Doc comment becomes `instructions`, unless
`#[question(instructions = ...)]` overrides it. `yes` and `no` fill in the two
Noul criteria, and are rejected on a Choice or Score field. `ChoiceOf<T>` and
`ScoreOf<T>` take their criteria from `T`. Wrapping a field in `Option` makes a
missing answer `None` rather than an error, which is how a question the server
does not answer stays non-fatal. Generated:
`Triage::questions() -> Questions` and
`Triage::from_response(&Response) -> Result<Triage, Error>`.

This is the layer the other SDKs cannot reach, and it is why the crate exists.

Both directions stay open:

```rust
let triage: Triage = client.evaluate_as(&ticket).await?;   // typed end to end
let response = client.evaluate(&ticket, Triage::questions()).await?;
let triage: Triage = response.extract()?;                  // typed from a raw response
```

### Layer 4 — the documented patterns (built)

Thin helpers over the API, not a framework. Each is a few lines and each stays
optional.

```rust
// Speculative fan-out over a corpus, bounded and back-pressured.
let results = client
    .evaluate_many(tickets, Triage::questions())
    .concurrency(16)
    .collect_all()   // or .try_collect() to stop at the first failure
    .await;

// Confidence-gated routing, the three-way split the docs recommend.
match answer.gate(0.5, 0.9) {
    Gate::Low => review_by_hand(ticket),
    Gate::Medium => confirm_with_user(ticket),
    Gate::High => act(ticket),
}

// Composite scoring, normalized so scales of different lengths compare.
let priority = Composite::new()
    .weigh(0.6, triage.severity.normalized())
    .weigh(0.3, triage.frustration.normalized())
    .weigh(0.1, triage.report_quality.normalized())
    .sum();
```

Fan-out is where Rust genuinely beats the alternatives: bounded concurrency over
a large corpus is a couple of lines and no thread pool.

## Dependencies

The rule is to reuse rather than write, but every dependency must earn its place
in a library that other people compile. What shipped:

| Crate | For | Why this one |
| --- | --- | --- |
| `serde` + `serde_json` | the whole data model | unavoidable and correct |
| `indexmap` | ordered maps | reproducible requests without forcing `preserve_order` on the dependency graph |
| `thiserror` | error types | the standard, no runtime cost |
| `reqwest` | HTTP | the default in the ecosystem; users can hand in their own `Client` |
| `backon` | the doubling, capped delay schedule | runtime-agnostic, so one schedule type serves both clients. `reqwest-retry` is async-only and would have forced a second implementation for the blocking client |
| `tracing` | logging | replaces the custom `Logger` the other SDKs ship; users already have a subscriber |
| `secrecy` | the API key | keeps the key out of `Debug` output by construction rather than by a manual impl |
| `futures-util` | `stream` feature only | `buffer_unordered` for bounded fan-out |
| `syn`, `quote`, `proc-macro2`, `darling`, `heck` | `derive` feature only | `darling` removes most attribute-parsing code; `heck` does the case conversion |

Deliberately **not** used:

- `async-trait` — not needed; inherent async methods are enough.
- `reqwest-middleware` — another layer, async-only, and it fixes the retry
  policy at client construction rather than per call.
- `anyhow` / `eyre` — application crates, not library ones.
- `once_cell` — `std::sync::OnceLock` covers it on the MSRV.

Dev only: `wiremock` (HTTP fixtures), `insta` (snapshot the serialized request
body — the wire format is the contract), `tokio` with `macros` and
`rt-multi-thread`, and `trybuild` (compile-fail tests for the derives).

## Testing

1. **Wire tests** — every documented payload round-trips. Already passing.
2. **Snapshot tests** — `insta` over serialized `Request` bodies, so a change in
   the JSON shows up as a reviewable diff.
3. **Server tests** — `wiremock` for status codes, `Retry-After`, malformed
   bodies, truncated responses, and unknown answer kinds.
4. **Retry tests** — a mock that fails N times and counts attempts, asserting
   backoff bounds and that the deadline is honoured.
5. **Compile-fail tests** — `trybuild` on the derives: seven fixtures covering
   an unsupported field type, a duplicate question id, missing instructions, a
   duplicate option name, a level with no description, a one-level `Levels`, and
   `Options` on a struct.
6. **Doc tests** — every public item with a non-obvious use carries one.
7. **Live tests** — behind `--ignored` and gated on `TYPESAFE_API_KEY`. Never in
   CI on pull requests.

## Versioning

Semantic versioning. Public enums that mirror server-side vocabulary carry
`#[non_exhaustive]`, so a new API error category or answer kind is a minor
release. `cargo-semver-checks` runs on every pull request. MSRV is 1.88 and a
bump is a minor release, called out in the changelog.

The floor comes from `darling`, which the derives are built on; everything
else in the tree builds on 1.85. It is a single number rather than one per
feature because `derive` is the reason most people reach for this crate, and a
promise that only holds with it turned off is not a useful promise.

## Roadmap

| Phase | Contents | Status |
| --- | --- | --- |
| 0 | Wire types, validation, errors, tests | done |
| 1 | Async client, retries, deadlines, `models()`, escape hatches | done |
| 2 | Blocking client, `wiremock` and `insta` suites | done |
| 3 | `derive`: `Options`, `Levels`, `Evaluation`, `trybuild` tests | done |
| 4 | `stream`: fan-out; `Gate` and `Composite` | done |
| 5 | Live smoke tests behind `--ignored` | done |
| 6 | Cookbook ports, 0.1.0 release | next |

Nothing is left stubbed: 67 tests and 20 doc tests cover the wire format, the
client, retries, the derives, and the compile-time diagnostics.
