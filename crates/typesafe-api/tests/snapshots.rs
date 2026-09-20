//! Snapshots of the JSON that goes on the wire.
//!
//! The wire format is the contract with the API, so a change to it should be a
//! reviewable diff rather than a surprise in production. Review the diff, then
//! accept it with `cargo insta accept`.

use typesafe_api::{Choice, Entry, JEV_LATEST, Noul, Request, Score, State, questions};

#[test]
fn a_mixed_request_serializes_as_expected() {
    let request = Request {
        state: State::text(
            "Shoes arrived two weeks late and in the wrong size. Also I see two charges of $120.",
        ),
        model: JEV_LATEST.to_owned(),
        questions: questions! {
            "department" => Choice::new("Which team should handle this?")
                .option("returns", "Exchanges, wrong or damaged items")
                .option("shipping", "Delivery status, delays, lost packages")
                .option("billing", "Charges, invoices, payment problems"),
            "tone" => Choice::new("What is the customer's tone?")
                .plain_options(["calm", "frustrated", "angry"]),
            "frustration" => Score::new("How frustrated is the customer?")
                .levels(["Calm", "Frustrated but civil", "Very angry"]),
            "is_urgent" => Noul::new("The message conveys urgency")
                .yes("Explicitly time-sensitive")
                .no("No urgency expressed"),
        },
        extra: serde_json::Map::new(),
    };

    insta::assert_json_snapshot!(request);
}

#[test]
fn structured_instructions_and_criteria_serialize_as_expected() {
    let request = Request {
        state: State::text("I sent the shoes back a week ago. When do I get my money?"),
        model: JEV_LATEST.to_owned(),
        questions: questions! {
            "return_topic" => Choice::new(Entry::fields([
                ("question", Entry::text("Which returns topic is the customer asking about?")),
                ("focus", Entry::text("Classify the information the customer wants.")),
            ]))
            .option(
                "return_policy",
                Entry::fields([
                    ("what", Entry::text("Whether and how an item can be returned")),
                    ("not_for", Entry::text("Progress of a return already sent")),
                    ("examples", Entry::list([Entry::text("How long do I have to return an order?")])),
                ]),
            )
            .option(
                "return_status",
                Entry::fields([
                    ("what", Entry::text("Progress of a return already sent")),
                    ("not_for", Entry::text("Whether and how an item can be returned")),
                ]),
            ),
        },
        extra: serde_json::Map::new(),
    };

    insta::assert_json_snapshot!(request);
}
