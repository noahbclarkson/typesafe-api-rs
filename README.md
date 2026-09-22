# typesafe-api

[![CI](https://github.com/noahbclarkson/typesafe-api-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/noahbclarkson/typesafe-api-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/typesafe-api.svg)](https://crates.io/crates/typesafe-api)
[![docs.rs](https://img.shields.io/docsrs/typesafe-api)](https://docs.rs/typesafe-api)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)](https://releases.rs)

A Rust client for the [TypeSafe](https://typesafe.ai) System One API.

System One models answer typed questions about a state and hand back structured
values — a probability, a chosen option, a position on a rubric — with no text
to parse. Rust already speaks in enums and structs, so this SDK closes the loop:
describe a judgement once as a type, and that type is both the request you send
and the answer you get back.

```sh
cargo add typesafe-api
```

## Three ways to ask

Each layer is the one below it with more types. Nothing is hidden: drop a level
at any point and you still see exactly what goes on the wire.

### 1. Questions and answers by name

Closest to the HTTP API, and the right choice when questions are built at
runtime.

```rust
use typesafe_api::{Choice, Client, Noul, Score, questions};

let client = Client::from_env()?;

let answers = client
    .evaluate(
        "Hi, my Stripe integration has been failing for 3 days. I am losing sales.",
        questions! {
            "is_urgent" => Noul::new("The message conveys urgency or time-sensitivity"),
            "department" => Choice::new("Which team should handle this?")
                .option("billing", "Payment or subscription issues")
                .option("technical", "Bugs or integration problems")
                .option("sales", "Pricing or account questions"),
            "frustration" => Score::new("How frustrated the customer appears")
                .levels(["Calm, just stating facts", "Frustrated but civil", "Very angry"]),
        },
    )
    .await?;

if answers.noul("is_urgent")?.is_yes(0.9) {
    page_the_on_call();
}
```

The state is your own data. Every call takes `impl Serialize`, so a domain
struct goes straight through:

```rust
#[derive(Serialize)]
struct Ticket<'a> {
    message: &'a str,
    plan: &'a str,
    open_orders: &'a [Order],
}

let answers = client.evaluate(&ticket, questions).await?;
```

### 2. Options and levels as enums

A Choice is a closed set of named outcomes, which is what an enum is. The
derive writes the criteria from your doc comments and reads the answer back into
a variant.

```rust
use typesafe_api::Options;

#[derive(Options, Debug)]
enum Department {
    /// Payment or subscription issues
    Billing,
    /// Bugs or integration problems
    Technical,
    /// A request that fits none of the above
    Other,
}

let department = answers.choice_as::<Department>("department")?;

match department.value {
    Department::Technical => assign(ticket, Team::Engineering),
    Department::Billing => assign(ticket, Team::Finance),
    Department::Other => triage_by_hand(ticket),
}

for (team, share) in department.runners_up(0.25) {
    notify(ticket, team);
}
```

### 3. The whole evaluation as one struct

The struct declares the questions and receives the answers. One definition, one
request, one typed result.

```rust
use typesafe_api::{ChoiceOf, Evaluation, Levels, NoulAnswer, ScoreOf};

#[derive(Levels)]
enum Frustration {
    /// Calm, just stating facts
    Calm,
    /// Frustrated but civil
    Frustrated,
    /// Very angry, strong language or threatening to leave
    VeryAngry,
}

#[derive(Evaluation)]
struct Triage {
    /// The message conveys urgency or time-sensitivity
    is_urgent: NoulAnswer,

    /// Which team should handle this?
    department: ChoiceOf<Department>,

    /// How frustrated the customer appears
    frustration: ScoreOf<Frustration>,
}

let triage: Triage = client.evaluate_as(&ticket).await?;

// Normalize before weighting: a three-level and a four-level scale are not
// comparable until both sit on 0..=1.
let priority = Composite::new()
    .weigh(0.6, triage.frustration.normalized())
    .weigh(0.4, triage.is_urgent.noul)
    .sum();
```

Every question in one request is evaluated independently and in parallel against
the same state, so adding questions costs tokens rather than latency. Ask
everything your code might need and ignore the answers it does not use.

## Choosing a question type

| Your question | Primitive | What comes back |
| --- | --- | --- |
| Is this true? | `Noul` | one probability, 0 to 1 |
| Which one of these? | `Choice` | the pick, every probability, confidence |
| Where on this scale? | `Score` | a weighted position, every probability, confidence |

A Noul carries no separate confidence: with two outcomes, the single value
already describes the distribution.

## Without a runtime

The blocking client shares every type with the asynchronous one.

```rust
use typesafe_api::{blocking::Client, Noul, questions};

let client = Client::from_env()?;
let answers = client
    .evaluate(&ticket, questions! { "billing" => Noul::new("Is this about billing?") })
    .send()?;
```

## Over a corpus

```rust
let results = client
    .evaluate_many(tickets, Triage::questions())
    .concurrency(16)
    .collect_all()   // keeps failures rather than discarding the batch
    .await;
```

## Configuration

```rust
use std::time::Duration;
use typesafe_api::{Client, RetryPolicy};

let client = Client::builder()
    .model("jev-1.13.0")                    // pin a version rather than an alias
    .timeout(Duration::from_secs(5))        // per attempt
    .deadline(Duration::from_secs(20))      // whole call, retries included
    .retry(RetryPolicy::default().max_retries(4))
    .header("x-team", "payments")
    .http_client(my_reqwest_client)         // your own pool, proxy, or transport
    .build()?;
```

Explicit values win over environment variables, which win over defaults.

| Variable | Sets | Default |
| --- | --- | --- |
| `TYPESAFE_API_KEY` | the API key | required |
| `TYPESAFE_BASE_URL` | API root | `https://api.typesafe.ai` |
| `TYPESAFE_DEFAULT_MODEL` | default model | `jev-latest` |

Any option can be overridden for a single call:

```rust
client.evaluate(&state, questions)
    .model("jev-preview")
    .timeout(Duration::from_secs(2))
    .retry(RetryPolicy::none())
    .extra_field("beam_width", 4)   // a field the API has and this crate does not
    .await?;
```

## A local model

[laya-server](https://github.com/noahbclarkson/laya-server) runs
[Laya](https://github.com/NandhaKishorM/laya), an open-weights System 1 model,
behind the same API, in Docker or natively on a GPU. Point the client at it and
nothing else in your code changes:

```sh
TYPESAFE_BASE_URL=http://localhost:8765
TYPESAFE_API_KEY=local   # the client requires one; the server checks it only if configured to
```

The Docker image answers `jev-*` model names, so the default model works as it
is; a native server needs `--jev-alias`, or set `TYPESAFE_DEFAULT_MODEL=laya`.
`Response::model` always names the Laya checkpoint that answered, such as
`laya-english`. Laya reads only the first few hundred tokens of a state and is
weaker than Jev without fine-tuning; the laya-server README lists every
difference.

## Nothing is hidden

Every convenience is built from a public layer you can reach:

```rust
let request = client.request(&state, questions)?;   // exactly what will be sent
println!("{}", serde_json::to_string_pretty(&request)?);

let raw: serde_json::Value = client.send_raw(&request).await?;   // untouched body
let response = client.send(&request).await?;
let typed: Triage = response.extract()?;
```

Answer kinds this crate does not model are kept as `Answer::Unknown` with their
payload intact, so a newer API stays readable without an upgrade.

## Errors

Failures say what happened, where, and whether trying again could help.

```rust
match client.evaluate(&state, questions).await {
    Ok(answers) => route(answers),
    Err(error) if error.is_retryable() => defer(job),
    Err(error) => tracing::error!(request_id = error.request_id(), %error, "evaluation failed"),
}
```

`429`, `529`, and `5xx` are retried with exponential backoff and jitter,
honouring `Retry-After` when the server sends one. Requests are checked locally
first, so a malformed body comes back as a list of paths rather than a `422`:

```text
request rejected before sending (2 problem(s))
  - questions.tone.criteria: a choice must offer at least one option
  - questions.severity.criteria: 12 levels exceeds the limit of 10
```

The API key is held in a `SecretString` and marked sensitive on the header, so
it cannot reach `Debug` output or a log line.

## Features

| Feature | Default | What it adds |
| --- | --- | --- |
| `rustls-tls` | yes | TLS via rustls |
| `native-tls` | no | TLS via the platform stack |
| `blocking` | no | `blocking::Client`, for code without a runtime |
| `derive` | no | the `Evaluation`, `Options`, and `Levels` derive macros |
| `stream` | no | `evaluate_many`, bounded fan-out over many states |
| `preserve-order` | no | authored key order inside every nested JSON object |

The traits the derives target are always available, so `Options` and `Levels`
can be written by hand when a description needs more than a doc comment.

## Documentation

- [API reference on docs.rs](https://docs.rs/typesafe-api)
- [`docs/DESIGN.md`](docs/DESIGN.md) — the design and its reasoning
- [`crates/typesafe-api/examples/`](crates/typesafe-api/examples) — runnable programs
- [TypeSafe documentation](https://docs.typesafe.ai) — the model, the
  primitives, and the patterns this SDK is shaped around

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Issues and pull requests are welcome.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.

This is a community project. It is not affiliated with or endorsed by TypeSafe.
