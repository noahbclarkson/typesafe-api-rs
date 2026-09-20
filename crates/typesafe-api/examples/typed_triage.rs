//! The typed layer: one struct that is both the request and the response.
//!
//! `cargo run --example typed_triage --features derive`
//!
//! Set `TYPESAFE_API_KEY` to send it for real. Without a key it prints the
//! request it would send and reads a recorded response instead, so the routing
//! logic is exercised either way.

use typesafe_api::{
    ChoiceOf, Client, Composite, Evaluation, Gate, Levels, NoulAnswer, Options, Response, ScoreOf,
};

/// Which team owns a ticket. The doc comments are what the model sees.
#[derive(Options, Debug, PartialEq)]
enum Department {
    /// Payments, invoicing, refunds, subscriptions
    Billing,
    /// Bugs, outages, integrations
    Technical,
    /// Pricing, upgrades, new accounts
    Sales,
    /// A request that fits none of the above
    Other,
}

/// How annoyed the customer sounds, lowest level first.
#[derive(Levels, Debug, PartialEq)]
enum Frustration {
    /// Calm, just stating facts
    Calm,
    /// Frustrated but civil
    Frustrated,
    /// Very angry, strong language or threatening to leave
    VeryAngry,
}

/// How much an engineer has to go on.
#[derive(Levels, Debug, PartialEq)]
enum ReportQuality {
    /// No detail; just says something is broken
    None,
    /// Names the feature but no steps or environment
    FeatureOnly,
    /// Steps to reproduce or environment, but not both
    Partial,
    /// Steps to reproduce and environment
    Full,
}

/// One declaration produces the questions and receives the answers.
#[derive(Evaluation, Debug)]
struct Triage {
    /// The message conveys urgency or time-sensitivity
    is_urgent: NoulAnswer,

    #[question(
        instructions = "Is the customer asking for a human agent?",
        yes = "Asks for a person, an agent, or someone to call them",
        no = "No request for a human"
    )]
    wants_human: NoulAnswer,

    /// Which team should handle this?
    department: ChoiceOf<Department>,

    /// How frustrated the customer appears
    frustration: ScoreOf<Frustration>,

    /// How much does the report give an engineer to work with?
    report_quality: ScoreOf<ReportQuality>,
}

const TICKET: &str = "Export to PDF fails with a spinner that never finishes. Some of our team \
     say CSV export still works, others say it fails too. This is the third time I am writing in \
     and honestly I am done. Steps: open any report, click Export, choose PDF. Chrome 128 on \
     macOS.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let triage = match Client::from_env() {
        Ok(client) => {
            println!("sending to {}\n", client.base_url());
            client.evaluate_as::<Triage, _>(TICKET).await?
        }
        Err(error) => {
            println!("no live call ({error}); using a recorded response\n");
            let client = Client::new("offline")?;
            let request = client.request(TICKET, Triage::questions())?;
            println!("{}\n", serde_json::to_string_pretty(&request)?);

            let response: Response = serde_json::from_str(RECORDED)?;
            response.extract::<Triage>()?
        }
    };

    println!("{triage:#?}\n");

    // A low-confidence route is a reason to ask, not to guess.
    match triage.department.gate(0.4, 0.75) {
        Gate::Low => println!("routing unclear; send to manual triage"),
        Gate::Medium => println!("probably {}; confirm before assigning", triage.department),
        Gate::High => println!("assign to {:?}", triage.department.value),
    }

    for (team, share) in triage.department.runners_up(0.25) {
        println!(
            "copy in {team:?} ({share:.0}% share)",
            share = share * 100.0
        );
    }

    if triage.wants_human.is_yes(0.8) {
        println!("escalate to an agent");
    }

    // Normalize first: a four-level scale and a three-level scale are not
    // comparable until both are on 0..=1.
    let priority = Composite::new()
        .weigh(0.5, triage.frustration.normalized())
        .weigh(0.3, triage.is_urgent.noul)
        .weigh(0.2, triage.report_quality.normalized());

    println!(
        "\nfrustration: {} -> {:?}",
        triage.frustration,
        triage.frustration.nearest().unwrap_or(Frustration::Calm)
    );
    println!(
        "report quality: {} -> {:?}",
        triage.report_quality,
        triage
            .report_quality
            .nearest()
            .unwrap_or(ReportQuality::None)
    );
    println!("priority: {:.2}", priority.sum());

    Ok(())
}

const RECORDED: &str = r#"{
  "model": "jev-1.13.0",
  "answers": {
    "is_urgent": { "type": "noul", "noul": 0.71 },
    "wants_human": { "type": "noul", "noul": 0.32 },
    "department": {
      "type": "choice",
      "choice": "technical",
      "confidence": 0.82,
      "probabilities": { "billing": 0.04, "technical": 0.88, "sales": 0.0, "other": 0.08 }
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
    },
    "report_quality": {
      "type": "score",
      "score": 3.0,
      "confidence": 1.0,
      "legend": {
        "0": "No detail; just says something is broken",
        "1": "Names the feature but no steps or environment",
        "2": "Steps to reproduce or environment, but not both",
        "3": "Steps to reproduce and environment"
      },
      "probabilities": { "0": 0.0, "1": 0.0, "2": 0.0, "3": 1.0 }
    }
  },
  "usage": { "input_tokens": 468, "output_tokens": 43 }
}"#;
