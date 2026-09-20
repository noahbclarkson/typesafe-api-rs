//! What the client does against a real HTTP server: headers, status handling,
//! retries, deadlines, and the escape hatches.

use std::time::Duration;

use serde_json::json;
use typesafe_api::{
    ApiErrorKind, Choice, Client, Error, JEV_PREVIEW, Limits, Noul, RetryPolicy, Score, questions,
};
use wiremock::matchers::{body_json_string, header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ANSWER: &str = r#"{
  "model": "jev-1.13.0",
  "answers": { "billing": { "type": "noul", "noul": 0.95 } },
  "usage": { "input_tokens": 10, "output_tokens": 2 }
}"#;

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_raw(ANSWER, "application/json")
        .insert_header("x-typesafe-request-id", "req_123")
}

fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .timeout(Duration::from_secs(5))
        .retry(RetryPolicy::none())
        .build()
        .expect("client builds")
}

fn billing() -> typesafe_api::Questions {
    questions! { "billing" => Noul::new("Is this about billing?") }
}

#[tokio::test]
async fn sends_the_documented_request_and_reads_the_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(header("authorization", "Bearer sk-test"))
        .and(header("content-type", "application/json"))
        .and(body_json_string(
            r#"{"state":"I was charged twice.","model":"jev-latest","questions":{"billing":{"type":"noul","instructions":"Is this about billing?"}}}"#,
        ))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let response = client
        .evaluate("I was charged twice.", billing())
        .await
        .unwrap();

    assert_eq!(response.model, "jev-1.13.0");
    assert_eq!(response.request_id.as_deref(), Some("req_123"));
    assert!(response.noul("billing").unwrap().is_yes(0.9));
    assert_eq!(response.usage.input_tokens, Some(10));
}

#[tokio::test]
async fn per_call_overrides_reach_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("x-team", "payments"))
        .and(header("x-call", "one"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .header("x-team", "payments")
        .retry(RetryPolicy::none())
        .build()
        .unwrap();

    let sent = client
        .evaluate("anything", billing())
        .model(JEV_PREVIEW)
        .header("x-call", "one")
        .extra_field("beam_width", 4);

    let request = sent.to_request().unwrap();
    assert_eq!(request.model, JEV_PREVIEW);
    assert_eq!(request.extra["beam_width"], json!(4));

    sent.await.unwrap();
}

#[tokio::test]
async fn an_unauthorized_response_carries_status_message_and_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({ "message": "invalid api key" }))
                .insert_header("x-typesafe-request-id", "req_bad"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let error = client.evaluate("anything", billing()).await.unwrap_err();

    assert_eq!(error.status(), Some(401));
    assert_eq!(error.request_id(), Some("req_bad"));
    assert!(!error.is_retryable());
    assert_eq!(error.api().unwrap().kind, ApiErrorKind::Unauthorized);
    assert!(error.to_string().contains("invalid api key"), "{error}");
    assert!(error.to_string().contains("req_bad"), "{error}");
}

#[tokio::test]
async fn a_422_keeps_the_offending_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "detail": [{ "loc": ["body", "questions", "tone"], "msg": "field required" }]
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let error = client.evaluate("anything", billing()).await.unwrap_err();

    assert_eq!(error.api().unwrap().kind, ApiErrorKind::Unprocessable);
    assert!(error.to_string().contains("field required"), "{error}");
    assert!(error.api().unwrap().body.is_some());
}

#[tokio::test]
async fn overloaded_and_rate_limited_are_retryable() {
    for status in [429_u16, 529, 503] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let error = client.evaluate("anything", billing()).await.unwrap_err();
        assert!(
            error.is_retryable(),
            "{status} should be retryable, got {error}"
        );
    }
}

#[tokio::test]
async fn retries_then_succeeds_and_honours_retry_after() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after-ms", "20")
                .set_body_json(json!({ "message": "slow down" })),
        )
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .retry(RetryPolicy::default().max_retries(3))
        .build()
        .unwrap();

    let started = std::time::Instant::now();
    let response = client.evaluate("anything", billing()).await.unwrap();

    assert_eq!(response.model, "jev-1.13.0");
    // Two waits of about 20ms each, rather than the 500ms the schedule would
    // have used, which is what honouring the header means.
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn retries_stop_at_the_limit_and_report_the_last_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(529))
        .expect(3)
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .retry(
            RetryPolicy::default()
                .max_retries(2)
                .backoff_initial(Duration::from_millis(1))
                .backoff_max(Duration::from_millis(2)),
        )
        .build()
        .unwrap();

    let error = client.evaluate("anything", billing()).await.unwrap_err();
    assert_eq!(error.api().unwrap().kind, ApiErrorKind::Overloaded);
}

#[tokio::test]
async fn a_deadline_stops_the_retry_loop() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .timeout(Duration::from_millis(80))
        .deadline(Duration::from_millis(150))
        .retry(
            RetryPolicy::default()
                .max_retries(20)
                .backoff_initial(Duration::from_millis(40)),
        )
        .build()
        .unwrap();

    let started = std::time::Instant::now();
    let error = client.evaluate("anything", billing()).await.unwrap_err();

    assert!(
        started.elapsed() < Duration::from_millis(600),
        "took {:?}",
        started.elapsed()
    );
    assert!(error.is_retryable(), "{error}");
}

#[tokio::test]
async fn a_malformed_body_names_the_endpoint_and_the_request_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("{ not json", "application/json")
                .insert_header("x-typesafe-request-id", "req_garbled"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let error = client.evaluate("anything", billing()).await.unwrap_err();

    assert!(matches!(error, Error::Decode { .. }));
    assert_eq!(error.request_id(), Some("req_garbled"));
    assert!(error.to_string().contains("/v1/systemone"), "{error}");
}

#[tokio::test]
async fn validation_runs_before_anything_is_sent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let error = client
        .evaluate(
            "anything",
            questions! {
                "tone" => Choice::new("What is the tone?"),
                "severity" => Score::new("How bad?").levels(["only one"]),
            },
        )
        .await
        .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("questions.tone.criteria"), "{message}");
    assert!(message.contains("questions.severity.criteria"), "{message}");
}

#[tokio::test]
async fn limits_can_be_lifted_for_a_single_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    client
        .evaluate(
            "anything",
            questions! { "severity" => Score::new("How bad?").levels(["one"]) },
        )
        .limits(Limits::unbounded())
        .await
        .unwrap();
}

#[tokio::test]
async fn send_raw_returns_the_body_untouched() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ok())
        .mount(&server)
        .await;

    let client = client_for(&server);
    let request = client.request("anything", billing()).unwrap();
    let raw = client.send_raw(&request).await.unwrap();

    assert_eq!(raw["answers"]["billing"]["noul"], json!(0.95));
}

#[tokio::test]
async fn models_are_listed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [
                { "name": "jev-latest", "description": "flagship", "release_date": "2026-09-01" }
            ]
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let models = client.models().await.unwrap();

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name, "jev-latest");
}

#[tokio::test]
async fn the_api_key_never_reaches_debug_output() {
    let client = Client::builder()
        .api_key("sk-super-secret-value")
        .base_url("https://example.invalid")
        .build()
        .unwrap();

    let rendered = format!("{client:?}");
    assert!(!rendered.contains("sk-super-secret-value"), "{rendered}");
}

#[test]
fn a_missing_api_key_says_which_variable_to_set() {
    let error = Client::builder()
        .base_url("https://example.invalid")
        .build();
    // The environment may legitimately hold a key on a developer machine.
    if std::env::var("TYPESAFE_API_KEY").is_err() {
        let message = error.unwrap_err().to_string();
        assert!(message.contains("TYPESAFE_API_KEY"), "{message}");
    }
}

#[test]
fn a_deadline_shorter_than_a_timeout_is_rejected() {
    let error = Client::builder()
        .api_key("sk-test")
        .timeout(Duration::from_secs(30))
        .deadline(Duration::from_secs(5))
        .build()
        .unwrap_err();

    assert!(error.to_string().contains("shorter than"), "{error}");
}

#[test]
fn reserved_headers_cannot_be_overridden() {
    let error = Client::builder()
        .api_key("sk-test")
        .header("authorization", "Bearer other")
        .build();
    assert!(
        error
            .unwrap_err()
            .to_string()
            .contains("cannot be overridden")
    );
}

#[tokio::test]
async fn a_request_can_be_built_inspected_and_sent_later() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["model"], json!("jev-latest"));
            ok()
        })
        .mount(&server)
        .await;

    let client = client_for(&server);
    let mut request = client.request("anything", billing()).unwrap();
    request.extra.insert("trace".to_owned(), json!("abc"));

    let response = client.send(&request).await.unwrap();
    assert!(response.noul("billing").is_ok());
}
