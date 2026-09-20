//! The parts of a call that do not care whether it is async or blocking:
//! effective options, header handling, status classification, retry pacing.

use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::Config;
use crate::error::{ApiError, ApiErrorKind, Error};
use crate::request::Request;
use crate::retry::RetryPolicy;
use crate::validate::Limits;

pub(crate) const EVALUATE_PATH: &str = "/v1/systemone";
pub(crate) const MODELS_PATH: &str = "/v1/models";
const REQUEST_ID_HEADER: &str = "x-typesafe-request-id";

/// Per-call overrides. Anything left unset falls back to the client.
#[derive(Clone, Debug, Default)]
pub(crate) struct CallOptions {
    pub(crate) model: Option<String>,
    #[allow(
        clippy::option_option,
        reason = "three states: inherit the client value, set a timeout, or have none"
    )]
    pub(crate) timeout: Option<Option<Duration>>,
    pub(crate) deadline: Option<Duration>,
    pub(crate) retry: Option<RetryPolicy>,
    pub(crate) limits: Option<Limits>,
    pub(crate) headers: Vec<(String, String)>,
}

/// A call's options after the client defaults have been folded in.
pub(crate) struct Call {
    pub(crate) url: String,
    pub(crate) timeout: Option<Duration>,
    pub(crate) deadline: Option<Duration>,
    pub(crate) retry: RetryPolicy,
    pub(crate) headers: HeaderMap,
}

impl CallOptions {
    pub(crate) fn resolve(&self, config: &Config, path: &str) -> Result<Call, Error> {
        let mut headers = config.headers.clone();
        for (name, value) in &self.headers {
            let name = HeaderName::try_from(name.as_str())
                .map_err(|_| Error::Config(format!("{name} is not a valid HTTP header name")))?;
            let value = HeaderValue::try_from(value.as_str()).map_err(|_| {
                Error::Config(format!("the value for {name} is not a valid header value"))
            })?;
            headers.insert(name, value);
        }

        Ok(Call {
            url: format!("{}{path}", config.base_url),
            timeout: self.timeout.unwrap_or(config.timeout),
            deadline: self.deadline.or(config.deadline),
            retry: self.retry.clone().unwrap_or_else(|| config.retry.clone()),
            headers,
        })
    }

    pub(crate) fn limits(&self, config: &Config) -> Limits {
        self.limits.unwrap_or(config.limits)
    }

    pub(crate) fn model<'a>(&'a self, config: &'a Config) -> &'a str {
        self.model.as_deref().unwrap_or(&config.model)
    }
}

/// Paces the attempts of one call.
pub(crate) struct Attempts {
    policy: RetryPolicy,
    schedule: backon::ExponentialBackoff,
    started: Instant,
    deadline: Option<Duration>,
    made: u32,
}

impl Attempts {
    pub(crate) fn new(policy: RetryPolicy, deadline: Option<Duration>) -> Self {
        Self {
            schedule: policy.schedule(),
            policy,
            started: Instant::now(),
            deadline,
            made: 0,
        }
    }

    pub(crate) fn begin(&mut self) -> u32 {
        self.made += 1;
        self.made
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The time left before the deadline, or `None` when there is no deadline.
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_sub(self.started.elapsed()))
    }

    /// The per-attempt timeout, shortened so an attempt cannot outlive the
    /// deadline.
    pub(crate) fn attempt_timeout(&self, configured: Option<Duration>) -> Option<Duration> {
        match (configured, self.remaining()) {
            (Some(timeout), Some(left)) => Some(timeout.min(left)),
            (Some(timeout), None) => Some(timeout),
            (None, left) => left,
        }
    }

    /// How long to wait before trying again, or `None` to give up.
    pub(crate) fn next_wait(&mut self, error: &Error) -> Option<Duration> {
        if !self.policy.allows(error) {
            return None;
        }
        let scheduled = self.schedule.next()?;
        let wait = self.policy.wait(retry_after_of(error), scheduled);

        if let Some(left) = self.remaining()
            && wait >= left
        {
            return None;
        }
        Some(wait)
    }
}

fn retry_after_of(error: &Error) -> Option<Duration> {
    error.api().and_then(|api| api.retry_after)
}

pub(crate) fn authorization(config: &Config) -> Result<HeaderValue, Error> {
    use secrecy::ExposeSecret as _;

    let mut value = HeaderValue::try_from(format!("Bearer {}", config.api_key.expose_secret()))
        .map_err(|_| {
            Error::Config("the API key contains characters that cannot go in a header".to_owned())
        })?;
    value.set_sensitive(true);
    Ok(value)
}

pub(crate) fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)?
        .to_str()
        .ok()
        .map(str::to_owned)
}

pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    if let Some(millis) = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        return Some(Duration::from_millis(millis));
    }
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(Duration::from_secs_f64)
}

pub(crate) fn api_error(status: StatusCode, url: &str, headers: &HeaderMap, body: &[u8]) -> Error {
    let parsed: Option<Value> = serde_json::from_slice(body).ok();
    let message = parsed
        .as_ref()
        .and_then(extract_message)
        .or_else(|| non_empty_text(body))
        .map(|text| truncate(&text, 600));

    Error::from(ApiError {
        status: status.as_u16(),
        kind: ApiErrorKind::from_status(status.as_u16()),
        url: url.to_owned(),
        request_id: request_id(headers),
        message,
        retry_after: retry_after(headers),
        body: parsed,
    })
}

/// Pulls a human-readable message out of the error bodies this API returns.
fn extract_message(body: &Value) -> Option<String> {
    for key in ["message", "detail", "error"] {
        match body.get(key) {
            Some(Value::String(text)) => return Some(text.clone()),
            Some(nested @ Value::Object(_)) => {
                if let Some(text) = extract_message(nested) {
                    return Some(text);
                }
            }
            // A 422 lists the offending fields; keep the whole list.
            Some(list @ Value::Array(_)) => return Some(list.to_string()),
            _ => {}
        }
    }
    None
}

fn non_empty_text(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}... (truncated)")
}

pub(crate) fn decode<T: DeserializeOwned>(
    body: &[u8],
    url: &str,
    request_id: Option<String>,
) -> Result<T, Error> {
    serde_json::from_slice(body).map_err(|err| Error::Decode {
        url: url.to_owned(),
        request_id,
        message: format!(
            "{err} (body: {})",
            truncate(&String::from_utf8_lossy(body), 300)
        ),
    })
}

pub(crate) fn transport_error(url: &str, error: reqwest::Error) -> Error {
    if error.is_timeout() {
        return Error::Timeout {
            url: url.to_owned(),
            elapsed_ms: 0,
            attempts: 1,
        };
    }
    Error::Transport {
        url: url.to_owned(),
        source: Box::new(error),
    }
}

/// Stamps the wall time and attempt count onto a timeout raised mid-call.
pub(crate) fn finalize(error: Error, elapsed: Duration, attempts: u32) -> Error {
    match error {
        Error::Timeout { url, .. } => Error::Timeout {
            url,
            elapsed_ms: elapsed.as_millis(),
            attempts,
        },
        other => other,
    }
}

pub(crate) fn log_attempt(url: &str, attempt: u32, request: Option<&Request>) {
    tracing::debug!(
        url,
        attempt,
        model = request.map(|r| r.model.as_str()),
        questions = request.map(|r| r.questions.len()),
        "sending request"
    );
}

pub(crate) fn log_retry(url: &str, attempt: u32, wait: Duration, error: &Error) {
    tracing::warn!(url, attempt, ?wait, %error, "attempt failed, retrying");
}
