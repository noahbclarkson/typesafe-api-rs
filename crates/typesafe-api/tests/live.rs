//! Smoke tests against the real API.
//!
//! These cost money, so they are `#[ignore]`d and never run in CI. Run them
//! before a release, or when a change touches the wire format:
//!
//! ```sh
//! TYPESAFE_API_KEY=... cargo test --all-features -- --ignored
//! ```
//!
//! They assert on shape and calibration direction, not on exact values. Model
//! answers move; a test that pins one to three decimal places is a test that
//! fails for no reason.

#![cfg(feature = "derive")]

use typesafe_api::{ChoiceOf, Client, Evaluation, Levels, NoulAnswer, Options, ScoreOf};

#[derive(Options, Debug, PartialEq)]
enum Department {
    /// Payments, invoicing, refunds
    Billing,
    /// Bugs, outages, integrations
    Technical,
    /// Pricing, upgrades, new accounts
    Sales,
}

#[derive(Levels, Debug, PartialEq)]
enum Frustration {
    /// Calm, just stating facts
    Calm,
    /// Frustrated but civil
    Frustrated,
    /// Very angry, strong language or threatening to leave
    VeryAngry,
}

#[derive(Evaluation, Debug)]
struct Triage {
    /// The message conveys urgency or time-sensitivity
    is_urgent: NoulAnswer,
    /// Which team should handle this?
    department: ChoiceOf<Department>,
    /// How frustrated the customer appears
    frustration: ScoreOf<Frustration>,
}

fn client() -> Client {
    Client::from_env().expect("set TYPESAFE_API_KEY to run the live tests")
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn a_clear_ticket_routes_the_obvious_way() {
    let triage: Triage = client()
        .evaluate_as(
            "My Stripe integration has been failing for 3 days and I am losing sales. \
             This is the third time I have written in.",
        )
        .await
        .expect("the call succeeds");

    assert_eq!(
        triage.department.value,
        Department::Technical,
        "{triage:#?}"
    );
    assert!(triage.is_urgent.noul > 0.5, "{triage:#?}");
    assert!(triage.frustration.score > 0.5, "{triage:#?}");

    let total: f64 = triage.department.probabilities.values().sum();
    assert!(
        (total - 1.0).abs() < 0.05,
        "probabilities should sum to about 1, got {total}"
    );
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn a_calm_question_reads_as_calm() {
    let triage: Triage = client()
        .evaluate_as("How do I reset my password?")
        .await
        .expect("the call succeeds");

    assert!(triage.is_urgent.noul < 0.5, "{triage:#?}");
    assert_eq!(
        triage.frustration.nearest(),
        Some(Frustration::Calm),
        "{triage:#?}"
    );
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn a_response_reports_the_versioned_model_and_usage() {
    let response = client()
        .evaluate("anything at all", Triage::questions())
        .await
        .expect("the call succeeds");

    assert!(response.model.starts_with("jev-"), "{}", response.model);
    assert!(response.usage.input_tokens.is_some_and(|tokens| tokens > 0));
    assert!(
        response.request_id.is_some(),
        "the API should return a request id"
    );
    assert_eq!(
        response.unknown().count(),
        0,
        "unmodelled answer kinds: {:?}",
        response.answers
    );
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn the_model_list_includes_the_default_alias() {
    let models = client().models().await.expect("the call succeeds");
    assert!(
        models
            .iter()
            .any(|model| model.name == typesafe_api::JEV_LATEST),
        "{models:#?}"
    );
}

#[tokio::test]
#[ignore = "calls the live API"]
async fn a_bad_key_is_reported_as_unauthorized() {
    use typesafe_api::{ApiErrorKind, Noul, questions};

    let client = Client::new("sk-definitely-not-a-real-key").expect("client builds");
    let error = client
        .evaluate(
            "anything",
            questions! { "q" => Noul::new("Is this a test?") },
        )
        .await
        .expect_err("a bad key should not be accepted");

    assert_eq!(
        error.api().map(|api| api.kind),
        Some(ApiErrorKind::Unauthorized),
        "{error}"
    );
}
