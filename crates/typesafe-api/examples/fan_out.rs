//! Bounded fan-out over a corpus.
//!
//! `cargo run --example fan_out --features "derive stream"`
//!
//! One request answers many questions about one document. To sweep a corpus,
//! send many requests at a bounded concurrency and keep the failures rather
//! than discarding the batch.

use std::time::Duration;

use typesafe_api::{ChoiceOf, Client, Evaluation, NoulAnswer, Options, RetryPolicy};

#[derive(Options, Debug)]
enum Topic {
    /// Payments, invoicing, refunds
    Billing,
    /// Bugs, outages, integrations
    Technical,
    /// Anything else
    Other,
}

#[derive(Evaluation, Debug)]
struct Screen {
    /// Does the message request a refund or credit?
    refund_requested: NoulAnswer,

    /// Does the message contain personal data such as a name, address, or card number?
    contains_pii: NoulAnswer,

    /// What is this message about?
    topic: ChoiceOf<Topic>,
}

const CORPUS: [&str; 5] = [
    "I was charged twice for order A-104. Please refund the duplicate.",
    "How do I reset my password?",
    "The export button crashes the settings page in Safari.",
    "My name is Jane Doe, card ending 4242, and I want my money back.",
    "Do you offer an annual plan?",
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(client) = Client::from_env() else {
        eprintln!("set TYPESAFE_API_KEY to run this example");
        return Ok(());
    };

    // Keep concurrency under the account rate limit: a burst that trips a 429
    // costs latency, not answers, but it costs it on every request at once.
    let results = client
        .evaluate_many(CORPUS, Screen::questions())
        .concurrency(4)
        .timeout(Duration::from_secs(5))
        .retry(RetryPolicy::default().max_retries(4))
        .collect_all()
        .await;

    let mut flagged = 0;
    for (message, result) in CORPUS.iter().zip(results) {
        match result {
            Ok(response) => {
                let screen: Screen = response.extract()?;
                let pii = if screen.contains_pii.is_yes(0.7) {
                    " [PII]"
                } else {
                    ""
                };
                if screen.refund_requested.is_yes(0.7) {
                    flagged += 1;
                }
                println!(
                    "{:<12} refund {:.2}{pii}  {message}",
                    format!("{:?}", screen.topic.value),
                    screen.refund_requested.noul,
                );
            }
            // One bad document should not discard the rest of the sweep.
            Err(error) => println!("failed: {error}\n  on: {message}"),
        }
    }

    println!("\n{flagged} of {} need a refund decision", CORPUS.len());
    Ok(())
}
