//! Builds a support-ticket triage request, checks it, and reads a response.
//!
//! No network and no API key: this shows the shape of a call while the HTTP
//! client is still being built. Run it with `cargo run --example triage`.

use serde::Serialize;
use typesafe_api::{
    Choice, IntoState, JEV_LATEST, Limits, Noul, Request, Response, Score, Verdict, questions,
    validate,
};

/// The state is your own struct. Nothing to convert by hand.
#[derive(Serialize)]
struct Ticket {
    message: &'static str,
    plan: &'static str,
    open_orders: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ticket = Ticket {
        message: "Hi, my Stripe integration has been failing for 3 days. I am losing sales.",
        plan: "growth",
        open_orders: 2,
    };

    let request = Request {
        state: ticket.into_state()?,
        model: JEV_LATEST.to_owned(),
        questions: questions! {
            "is_urgent" => Noul::new("The message conveys urgency or time-sensitivity"),
            "wants_human" => Noul::new("Is the customer asking for a human agent?")
                .yes("Asks for a person, an agent, or someone to call them")
                .no("No request for a human"),
            "department" => Choice::new("Which team should handle this?")
                .option("billing", "Payment or subscription issues")
                .option("technical", "Bugs or integration problems")
                .option("sales", "Pricing or account questions"),
            "frustration" => Score::new("How frustrated the customer appears")
                .levels([
                    "Calm, just stating facts",
                    "Frustrated but civil",
                    "Very angry, strong language or threatening to leave",
                ]),
        },
        extra: serde_json::Map::new(),
    };

    // Catch a malformed body here rather than as a 422 with a network round trip.
    validate::check(&request, Limits::default())?;

    println!("--- request ---");
    println!("{}", serde_json::to_string_pretty(&request)?);

    // A recorded response, so the example runs without a key.
    let response: Response = serde_json::from_str(RECORDED_RESPONSE)?;

    println!("\n--- routing ---");
    println!("model: {}", response.model);

    let department = response.choice("department")?;
    if department.confidence < 0.3 {
        println!("department is unclear; send it to a person");
    } else {
        println!(
            "assign to {} (confidence {:.2})",
            department.choice, department.confidence
        );
        for (team, probability) in department.runners_up(0.25) {
            println!("copy in {team} ({probability:.2} share)");
        }
    }

    match response.noul("wants_human")?.verdict(0.2, 0.8) {
        Verdict::Yes => println!("escalate to an agent"),
        Verdict::No => println!("the bot can take this"),
        Verdict::Unsure => println!("unclear either way; let a person decide"),
    }

    let frustration = response.score("frustration")?;
    let urgency = response.noul("is_urgent")?.noul;

    // Normalize before weighting: scales of different lengths are not
    // comparable until they are on the same range.
    let priority = 0.6 * frustration.normalized() + 0.4 * urgency;
    println!(
        "frustration {:.2} of {} ({}), priority {priority:.2}",
        frustration.score,
        frustration.top_level(),
        frustration
            .describe(frustration.nearest_level())
            .and_then(|entry| match entry {
                typesafe_api::Entry::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or("?"),
    );

    if let Some(usage) = response.usage.input_tokens {
        println!("billed for {usage} input tokens");
    }

    Ok(())
}

const RECORDED_RESPONSE: &str = r#"{
  "model": "jev-1.13.0",
  "answers": {
    "is_urgent": { "type": "noul", "noul": 1.0 },
    "wants_human": { "type": "noul", "noul": 0.4 },
    "department": {
      "type": "choice",
      "choice": "technical",
      "confidence": 0.78,
      "probabilities": { "technical": 0.6, "billing": 0.35, "sales": 0.05 }
    },
    "frustration": {
      "type": "score",
      "score": 1.28,
      "confidence": 0.58,
      "legend": {
        "0": "Calm, just stating facts",
        "1": "Frustrated but civil",
        "2": "Very angry, strong language or threatening to leave"
      },
      "probabilities": { "0": 0.0, "1": 0.72, "2": 0.28 }
    }
  },
  "usage": { "input_tokens": 392, "output_tokens": 65 }
}"#;
