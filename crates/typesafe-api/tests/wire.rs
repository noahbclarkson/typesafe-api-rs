//! Round-trips against the exact payloads published in the API reference.
//!
//! These are the contract. If the API changes shape, these fail first.

use serde_json::{Value, json};
use typesafe_api::{
    Answer, Choice, Entry, JEV_LATEST, Limits, Noul, Request, Response, Score, State, Verdict,
    questions, validate,
};

fn request(state: &str, questions: typesafe_api::Questions) -> Request {
    Request {
        state: State::text(state),
        model: JEV_LATEST.to_owned(),
        questions,
        extra: serde_json::Map::new(),
    }
}

#[test]
fn noul_request_matches_the_documented_body() {
    let body = request(
        "Help! My payouts have been failing for 3 days.",
        questions! {
            "is_urgent" => Noul::new("Does this convey urgency?")
                .yes("Explicitly time-sensitive")
                .no("No urgency expressed"),
        },
    );

    assert_eq!(
        serde_json::to_value(&body).unwrap(),
        json!({
            "state": "Help! My payouts have been failing for 3 days.",
            "model": "jev-latest",
            "questions": {
                "is_urgent": {
                    "type": "noul",
                    "instructions": "Does this convey urgency?",
                    "criteria": {
                        "true": "Explicitly time-sensitive",
                        "false": "No urgency expressed"
                    }
                }
            }
        })
    );
}

#[test]
fn choice_and_score_requests_match_the_documented_body() {
    let body = request(
        "Help! My payouts have been failing for 3 days.",
        questions! {
            "department" => Choice::new("Which team should handle this?")
                .option("billing", "Payments, invoicing, refunds")
                .option("technical", "Bugs, outages, integrations")
                .option("sales", "Pricing, upgrades, new accounts"),
            "frustration" => Score::new("How frustrated is the customer?")
                .levels(["Calm", "Frustrated", "Very angry"]),
        },
    );

    let value = serde_json::to_value(&body).unwrap();
    assert_eq!(
        value["questions"]["department"],
        json!({
            "type": "choice",
            "instructions": "Which team should handle this?",
            "criteria": {
                "billing": "Payments, invoicing, refunds",
                "technical": "Bugs, outages, integrations",
                "sales": "Pricing, upgrades, new accounts"
            }
        })
    );
    assert_eq!(
        value["questions"]["frustration"],
        json!({
            "type": "score",
            "instructions": "How frustrated is the customer?",
            "criteria": ["Calm", "Frustrated", "Very angry"]
        })
    );
}

#[test]
fn an_undescribed_choice_option_serializes_as_null() {
    let body = request(
        "anything",
        questions! {
            "tone" => Choice::new("What is the customer's tone?")
                .plain_options(["calm", "frustrated", "angry"]),
        },
    );

    let value = serde_json::to_value(&body).unwrap();
    assert_eq!(
        value["questions"]["tone"]["criteria"],
        json!({
            "calm": null, "frustrated": null, "angry": null
        })
    );
}

#[test]
fn structured_instructions_keep_their_authored_order() {
    let body = request(
        "a resume",
        questions! {
            "same_as_record_18" => Noul::new(Entry::fields([
                ("potential_duplicate", Entry::fields([
                    ("name", Entry::text("Jon Smith")),
                    ("location", Entry::text("Oakland, CA")),
                ])),
                ("question", Entry::text("Is the resume for the same person as `potential_duplicate`?")),
            ])),
        },
    );

    let text = serde_json::to_string(&body).unwrap();
    let duplicate = text.find("potential_duplicate").unwrap();
    let question = text.find("\"question\"").unwrap();
    assert!(
        duplicate < question,
        "authored field order should survive serialization"
    );
}

#[test]
fn extra_fields_merge_into_the_top_level_body() {
    let mut body = request("anything", questions! { "q" => Noul::new("ok?") });
    body.extra.insert("beam_width".to_owned(), json!(4));

    let value = serde_json::to_value(&body).unwrap();
    assert_eq!(value["beam_width"], json!(4));
}

#[test]
fn state_accepts_a_domain_struct() {
    #[derive(serde::Serialize)]
    struct Ticket {
        subject: &'static str,
        messages: Vec<&'static str>,
    }

    use typesafe_api::IntoState;
    let state = Ticket {
        subject: "Duplicate charge",
        messages: vec!["I was charged twice"],
    }
    .into_state()
    .unwrap();

    assert!(matches!(state, State::Object(_)));
}

#[test]
fn state_rejects_a_shape_the_api_cannot_take() {
    use typesafe_api::IntoState;
    let error = 42_u32.into_state().unwrap_err();
    assert_eq!(
        error.to_string(),
        "state must be a string, object, or array; found number"
    );
}

#[test]
fn documented_response_deserializes_with_typed_accessors() {
    let raw = json!({
        "model": "jev-1.13.0",
        "answers": {
            "department": {
                "type": "choice",
                "choice": "technical",
                "confidence": 0.78,
                "probabilities": { "technical": 0.85, "sales": 0.0, "billing": 0.15 }
            },
            "frustration": {
                "type": "score",
                "score": 1.0,
                "confidence": 1.0,
                "legend": {
                    "0": "Calm, just stating facts",
                    "1": "Frustrated but civil",
                    "2": "Very angry, strong language"
                },
                "probabilities": { "0": 0.0, "1": 1.0, "2": 0.0 }
            },
            "is_urgent": { "type": "noul", "noul": 1.0 }
        },
        "usage": { "input_tokens": 392, "output_tokens": 65 }
    });

    let response: Response = serde_json::from_value(raw).unwrap();

    assert_eq!(response.model, "jev-1.13.0");
    assert_eq!(response.usage.input_tokens, Some(392));

    let department = response.choice("department").unwrap();
    assert_eq!(department.choice, "technical");
    assert_eq!(department.probability("billing"), Some(0.15));
    assert_eq!(department.ranked()[0], ("technical", 0.85));
    assert_eq!(department.runners_up(0.1), vec![("billing", 0.15)]);

    let frustration = response.score("frustration").unwrap();
    assert_eq!(frustration.top_level(), 2);
    assert!((frustration.normalized() - 0.5).abs() < f64::EPSILON);
    assert_eq!(frustration.nearest_level(), 1);
    assert_eq!(
        frustration.describe(2),
        Some(&Entry::text("Very angry, strong language"))
    );

    assert_eq!(
        response.noul("is_urgent").unwrap().verdict(0.2, 0.8),
        Verdict::Yes
    );
}

#[test]
fn reading_an_answer_as_the_wrong_kind_says_what_was_found() {
    let response: Response = serde_json::from_value(json!({
        "model": "jev-1.13.0",
        "answers": { "is_urgent": { "type": "noul", "noul": 0.95 } },
        "usage": {}
    }))
    .unwrap();

    assert_eq!(
        response.choice("is_urgent").unwrap_err().to_string(),
        "answer `is_urgent` is a noul, not a choice"
    );
    assert_eq!(
        response.noul("urgency").unwrap_err().to_string(),
        "no answer named `urgency`; the response has: is_urgent"
    );
}

#[test]
fn an_unknown_answer_kind_is_kept_rather_than_dropped() {
    let response: Response = serde_json::from_value(json!({
        "model": "jev-2.0.0",
        "answers": {
            "ranking": { "type": "ranking", "order": ["a", "b"] },
            "is_urgent": { "type": "noul", "noul": 0.95 }
        },
        "usage": {}
    }))
    .unwrap();

    assert_eq!(response.nouls().count(), 1);
    let (id, answer) = response.unknown().next().unwrap();
    assert_eq!(id, "ranking");
    assert_eq!(answer.kind(), "ranking");
    let Answer::Unknown { raw, .. } = answer else {
        panic!("expected an unknown answer")
    };
    assert_eq!(raw["order"], json!(["a", "b"]));

    let round_tripped: Value = serde_json::to_value(answer).unwrap();
    assert_eq!(round_tripped["order"], json!(["a", "b"]));
}

#[test]
fn validation_reports_every_problem_with_a_path() {
    let body = Request {
        state: State::text("anything"),
        model: String::new(),
        questions: questions! {
            "tone" => Choice::new("tone?"),
            "severity" => Score::new("how bad?").levels(["only one level"]),
        },
        extra: serde_json::Map::new(),
    };

    let error = validate::check(&body, Limits::default()).unwrap_err();
    let paths: Vec<&str> = error.issues.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "model",
            "questions.tone.criteria",
            "questions.severity.criteria"
        ]
    );
    assert!(error.to_string().contains("questions.severity.criteria"));

    let lenient = Request {
        questions: questions! { "severity" => Score::new("how bad?").levels(["one"]) },
        ..body
    };
    assert!(
        validate::check(&lenient, Limits::unbounded()).is_err(),
        "model is still blank"
    );
}
