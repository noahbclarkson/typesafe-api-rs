# system-one

[![CI](https://github.com/OWNER/system-one-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/system-one-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/system-one.svg)](https://crates.io/crates/system-one)
[![docs.rs](https://img.shields.io/docsrs/system-one)](https://docs.rs/system-one)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](https://releases.rs)

A Rust client for the [TypeSafe](https://typesafe.ai) System One API.

System One models answer typed questions about a state and hand back structured
values — a probability, a chosen option, a position on a rubric — with no text
to parse. Rust already speaks in enums and structs, so this SDK closes the loop:
you describe the judgement once as a type, and the same type is both the request
you send and the answer you get back.

> **Status: pre-release.** The data model is complete and covered by tests
> against the published payloads. The HTTP client is next. See
> [`docs/DESIGN.md`](docs/DESIGN.md) for the full plan and API surface.

## Install

```sh
cargo add system-one
```

## Three ways to ask

Each layer is the one below it with more types. Nothing is hidden: you can drop
a level at any point and still see exactly what goes on the wire.

### 1. Questions and answers by name

Closest to the HTTP API. Good for questions built at runtime.

```rust
use system_one::{Choice, Client, Noul, Score, questions};

let client = Client::from_env()?;

let answers = client
    .evaluate(
        "Hi, my Stripe integration has been failing for 3 days. I'm losing sales.",
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

### 2. Options and levels as enums

A Choice is a closed set, which is what an enum is. The derive writes the
criteria from your doc comments and parses the answer back into the variant.

```rust
use system_one::Options;

#[derive(Options)]
enum Department {
    /// Payment or subscription issues
    Billing,
    /// Bugs or integration problems
    Technical,
    /// Pricing or account questions
    Sales,
}

let department = answers.choice_as::<Department>("department")?;

match department.value {
    Department::Technical => assign(ticket, Team::Engineering),
    Department::Billing => assign(ticket, Team::Finance),
    Department::Sales => assign(ticket, Team::Sales),
}

for (team, probability) in department.runners_up(0.25) {
    notify(ticket, team);
}
```

### 3. The whole evaluation as one struct

The struct declares the questions and receives the answers. One definition, one
request, one typed result.

```rust
use system_one::{Evaluation, Levels, Options};

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
    is_urgent: Noul,

    /// Which team should handle this?
    department: Choice<Department>,

    /// How frustrated the customer appears
    frustration: Score<Frustration>,
}

let triage: Triage = client.evaluate_as(&ticket).await?;

let priority = 0.6 * triage.frustration.normalized() + 0.4 * f64::from(triage.is_urgent);
```

Every question in one request is evaluated independently and in parallel against
the same state, so adding questions costs tokens rather than latency. Ask
everything your code might need and ignore the answers it does not use.

## Choosing a question type

| Your question | Primitive | You get back |
| --- | --- | --- |
| Is this true? | [`Noul`] | One probability, 0 to 1 |
| Which one of these? | [`Choice`] | The pick, every probability, confidence |
| Where on this scale? | [`Score`] | A weighted position, every probability, confidence |

A Noul carries no separate confidence: with two outcomes, the single value
already describes the distribution.

## Configuration

```rust
use std::time::Duration;
use system_one::{Client, RetryPolicy};

let client = Client::builder()
    .api_key(std::env::var("TYPESAFE_API_KEY")?)   // or TYPESAFE_API_KEY, read by from_env
    .model("jev-1.13.0")                           // pin a version rather than an alias
    .timeout(Duration::from_secs(10))
    .retry(RetryPolicy::default().max_retries(3))
    .build()?;
```

Explicit values win over environment variables, which win over defaults.

| Variable | Sets | Default |
| --- | --- | --- |
| `TYPESAFE_API_KEY` | The API key | required |
| `TYPESAFE_BASE_URL` | API root | `https://api.typesafe.ai` |
| `TYPESAFE_DEFAULT_MODEL` | Default model | `jev-latest` |

## Errors

Failures say what happened, where, and whether trying again could help.

```rust
match client.evaluate(&state, questions).await {
    Ok(answers) => route(answers),
    Err(error) if error.is_retryable() => defer(job),
    Err(error) => {
        tracing::error!(request_id = error.request_id(), %error, "evaluation failed");
    }
}
```

Requests are checked locally before they are sent, so a malformed body comes
back as a list of paths rather than a 422:

```text
request rejected before sending (2 problem(s))
  - questions.tone.criteria: a choice must offer at least one option
  - questions.severity.criteria: 12 levels exceeds the limit of 10
```

## Features

| Feature | Default | What it adds |
| --- | --- | --- |
| `rustls-tls` | yes | TLS via rustls |
| `native-tls` | no | TLS via the platform stack |
| `blocking` | no | A synchronous client for scripts and sync codebases |
| `derive` | no | `#[derive(Evaluation)]`, `#[derive(Options)]`, `#[derive(Levels)]` |
| `stream` | no | Bounded-concurrency fan-out over many states |
| `preserve-order` | no | Keeps authored key order inside every nested JSON object |

## Documentation

- [API reference on docs.rs](https://docs.rs/system-one)
- [`docs/DESIGN.md`](docs/DESIGN.md) — the design and its reasoning
- [`examples/`](crates/system-one/examples) — runnable programs
- [TypeSafe documentation](https://docs.typesafe.ai) — the model, the
  primitives, and the patterns this SDK is shaped around

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Issues and pull requests are welcome.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.

This is a community project. It is not affiliated with or endorsed by TypeSafe.

[`Noul`]: https://docs.rs/system-one/latest/system_one/struct.Noul.html
[`Choice`]: https://docs.rs/system-one/latest/system_one/struct.Choice.html
[`Score`]: https://docs.rs/system-one/latest/system_one/struct.Score.html
