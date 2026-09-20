//! The same call without a runtime.
//!
//! `cargo run --example blocking_triage --features blocking`
//!
//! Every type here except the client is shared with the asynchronous side.

use std::time::Duration;

use typesafe_api::{Choice, Noul, RetryPolicy, Score, blocking::Client, questions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(client) = Client::builder()
        .timeout(Duration::from_secs(5))
        .deadline(Duration::from_secs(20))
        .retry(RetryPolicy::default().max_retries(3))
        .build_blocking()
    else {
        eprintln!("set TYPESAFE_API_KEY to run this example");
        return Ok(());
    };

    let response = client
        .evaluate(
            "My running shoes arrived in the wrong size. Can I swap them for a size 10?",
            questions! {
                "department" => Choice::new("Which team should handle this?")
                    .option("returns", "Exchanges, wrong or damaged items")
                    .option("shipping", "Delivery status, delays, lost packages")
                    .option("billing", "Charges, invoices, payment problems"),
                "wants_exchange" => Noul::new("Does the customer want an exchange rather than a refund?"),
                "urgency" => Score::new("How urgent is this?")
                    .levels(["Can wait", "This week", "Today"]),
            },
        )
        .send()?;

    let department = response.choice("department")?;
    println!(
        "department: {} ({:.2})",
        department.choice, department.confidence
    );
    println!("exchange:   {:.2}", response.noul("wants_exchange")?.noul);

    let urgency = response.score("urgency")?;
    println!(
        "urgency:    {:.2} of {}",
        urgency.score,
        urgency.top_level()
    );

    if let Some(id) = &response.request_id {
        println!("request:    {id}");
    }

    Ok(())
}
