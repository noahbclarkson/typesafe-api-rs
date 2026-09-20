//! The synchronous client, against the same server as the asynchronous one.
//!
//! `reqwest::blocking` refuses to run inside a runtime, so the mock server gets
//! a runtime of its own and the client runs on a separate thread.

#![cfg(feature = "blocking")]

use std::time::Duration;

use serde_json::json;
use typesafe_api::{ApiErrorKind, Noul, Questions, RetryPolicy, blocking::Client, questions};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ANSWER: &str = r#"{
  "model": "jev-1.13.0",
  "answers": { "billing": { "type": "noul", "noul": 0.95 } },
  "usage": { "input_tokens": 10, "output_tokens": 2 }
}"#;

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_raw(ANSWER, "application/json")
        .insert_header("x-typesafe-request-id", "req_sync")
}

fn billing() -> Questions {
    questions! { "billing" => Noul::new("Is this about billing?") }
}

/// Runs `body` off the runtime thread, which is the only place a blocking
/// client may be called.
fn with_server<T: Send>(mocks: Vec<Mock>, body: impl FnOnce(String) -> T + Send) -> T {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        for mock in mocks {
            server.register(mock).await;
        }
        server
    });

    let uri = server.uri();
    std::thread::scope(|scope| scope.spawn(move || body(uri)).join()).expect("test thread panicked")
}

fn client(uri: &str) -> Client {
    Client::builder()
        .api_key("sk-test")
        .base_url(uri)
        .timeout(Duration::from_secs(5))
        .retry(RetryPolicy::none())
        .build_blocking()
        .expect("client builds")
}

#[test]
fn sends_a_request_and_reads_the_answer() {
    with_server(
        vec![
            Mock::given(method("POST"))
                .and(path("/v1/systemone"))
                .and(header("authorization", "Bearer sk-test"))
                .respond_with(ok())
                .expect(1),
        ],
        |uri| {
            let response = client(&uri)
                .evaluate("I was charged twice.", billing())
                .send()
                .unwrap();
            assert!(response.noul("billing").unwrap().is_yes(0.9));
            assert_eq!(response.request_id.as_deref(), Some("req_sync"));
        },
    );
}

#[test]
fn an_error_response_carries_the_same_detail_as_the_async_client() {
    with_server(
        vec![Mock::given(method("POST")).respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "message": "invalid api key" })),
        )],
        |uri| {
            let error = client(&uri)
                .evaluate("anything", billing())
                .send()
                .unwrap_err();
            assert_eq!(error.api().unwrap().kind, ApiErrorKind::Unauthorized);
            assert!(error.to_string().contains("invalid api key"), "{error}");
        },
    );
}

#[test]
fn retries_apply_to_the_blocking_client_too() {
    with_server(
        vec![
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(429).insert_header("retry-after-ms", "10"))
                .up_to_n_times(1)
                .expect(1),
            Mock::given(method("POST")).respond_with(ok()).expect(1),
        ],
        |uri| {
            let client = Client::builder()
                .api_key("sk-test")
                .base_url(&uri)
                .retry(RetryPolicy::default().max_retries(2))
                .build_blocking()
                .unwrap();

            let response = client.evaluate("anything", billing()).send().unwrap();
            assert_eq!(response.model, "jev-1.13.0");
        },
    );
}

#[test]
fn models_are_listed() {
    with_server(
        vec![
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "models": [{
                        "name": "jev-latest",
                        "description": "flagship",
                        "release_date": "2026-09-01"
                    }]
                }))),
        ],
        |uri| {
            let models = client(&uri).models().unwrap();
            assert_eq!(models[0].name, "jev-latest");
        },
    );
}

#[test]
fn a_built_request_can_be_sent_and_inspected() {
    with_server(
        vec![Mock::given(method("POST")).respond_with(ok())],
        |uri| {
            let client = client(&uri);
            let request = client.request("anything", billing()).unwrap();
            assert_eq!(request.model, "jev-latest");

            let raw = client.send_raw(&request).unwrap();
            assert_eq!(raw["answers"]["billing"]["noul"], json!(0.95));
        },
    );
}
