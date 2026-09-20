//! Errors, arranged so the cause is readable without unwrapping a chain.

use serde_json::Value;

use crate::entry::InvalidEntry;
use crate::state::InvalidState;
use crate::validate::ValidationError;

/// Everything this crate can fail with.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No API key was supplied and the environment variable was unset.
    #[error("no API key: set {env} or pass Client::builder().api_key(..)")]
    MissingApiKey {
        /// The environment variable that was checked.
        env: &'static str,
    },

    /// The client was configured with values that cannot work together.
    #[error("invalid client configuration: {0}")]
    Config(String),

    /// The request was rejected locally, before anything was sent.
    #[error(transparent)]
    Invalid(#[from] ValidationError),

    /// The state could not be used.
    #[error("invalid state: {0}")]
    State(#[from] InvalidState),

    /// A description could not be used.
    #[error("invalid entry: {0}")]
    Entry(#[from] InvalidEntry),

    /// The server returned an unsuccessful response.
    ///
    /// Boxed so that `Result<T, Error>` stays small on the happy path.
    #[error(transparent)]
    Api(Box<ApiError>),

    /// The request never reached the server, or the body was cut short.
    #[error("could not reach {url}: {source}")]
    Transport {
        /// The endpoint that was being called.
        url: String,
        /// The underlying transport failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The request exceeded its timeout, including any retries.
    #[error("request to {url} timed out after {elapsed_ms}ms ({attempts} attempt(s))")]
    Timeout {
        /// The endpoint that was being called.
        url: String,
        /// Wall time spent before giving up.
        elapsed_ms: u128,
        /// How many attempts were made.
        attempts: u32,
    },

    /// The response was valid HTTP but not a body this crate could read.
    #[error("could not decode the response from {url}{}: {message}", request_id_suffix(.request_id.as_deref()))]
    Decode {
        /// The endpoint that was being called.
        url: String,
        /// The request id, for a support ticket.
        request_id: Option<String>,
        /// What went wrong, including the field path when serde reported one.
        message: String,
    },

    /// A question id was read that the response does not contain.
    #[error("no answer named `{id}`; the response has: {}", available.join(", "))]
    AnswerNotFound {
        /// The id that was asked for.
        id: String,
        /// The ids that are present.
        available: Vec<String>,
    },

    /// A Choice answer named an option the target enum does not declare.
    #[error("answer `{id}`: {source}")]
    UnknownOption {
        /// The question id whose answer could not be read.
        id: String,
        /// What came back and what was expected.
        #[source]
        source: Box<crate::typed::UnknownOption>,
    },

    /// An answer was read as the wrong kind.
    #[error("answer `{id}` is a {found}, not a {expected}")]
    AnswerKind {
        /// The id that was asked for.
        id: String,
        /// The kind that was expected.
        expected: &'static str,
        /// The kind that was found.
        found: String,
    },
}

/// An unsuccessful HTTP response, with everything needed to act on it.
#[derive(Debug, thiserror::Error)]
#[error("{kind} ({status}) from {url}{}{}", request_id_suffix(.request_id.as_deref()), message_suffix(.message.as_deref()))]
pub struct ApiError {
    /// The HTTP status code.
    pub status: u16,
    /// The status read as a category worth branching on.
    pub kind: ApiErrorKind,
    /// The endpoint that was being called.
    pub url: String,
    /// The `x-typesafe-request-id` header, when present.
    pub request_id: Option<String>,
    /// The human-readable message parsed out of the body, when there was one.
    pub message: Option<String>,
    /// How long the server asked the caller to wait, from `Retry-After`.
    pub retry_after: Option<std::time::Duration>,
    /// The raw body, parsed as JSON when possible.
    pub body: Option<Value>,
}

/// The categories of API failure worth branching on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ApiErrorKind {
    /// 400. The request was malformed.
    BadRequest,
    /// 401. The API key is missing or invalid.
    Unauthorized,
    /// 403. The key is valid but not allowed to do this.
    Forbidden,
    /// 404. No such endpoint or resource.
    NotFound,
    /// 422. The body failed server-side validation.
    Unprocessable,
    /// 429. The rate limit was exceeded. Retryable.
    RateLimited,
    /// 529. The service is temporarily overloaded. Retryable.
    Overloaded,
    /// Any other 5xx. Retryable.
    Server,
    /// A status this crate does not categorise.
    Other,
}

impl ApiErrorKind {
    /// Reads a status code as a category.
    pub fn from_status(status: u16) -> Self {
        match status {
            400 => Self::BadRequest,
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            404 => Self::NotFound,
            422 => Self::Unprocessable,
            429 => Self::RateLimited,
            529 => Self::Overloaded,
            500..=599 => Self::Server,
            _ => Self::Other,
        }
    }

    /// Whether retrying the same request could plausibly succeed.
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::Overloaded | Self::Server)
    }
}

impl std::fmt::Display for ApiErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::BadRequest => "bad request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not found",
            Self::Unprocessable => "unprocessable entity",
            Self::RateLimited => "rate limited",
            Self::Overloaded => "overloaded",
            Self::Server => "server error",
            Self::Other => "error",
        };
        f.write_str(text)
    }
}

impl From<ApiError> for Error {
    fn from(error: ApiError) -> Self {
        Self::Api(Box::new(error))
    }
}

impl Error {
    /// The API failure behind this error, when there was one.
    pub fn api(&self) -> Option<&ApiError> {
        match self {
            Self::Api(api) => Some(api),
            _ => None,
        }
    }

    /// Whether retrying the same request could plausibly succeed.
    ///
    /// The client already retries these by default; this is for callers who
    /// run their own loop on top.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api(api) => api.kind.is_retryable(),
            Self::Transport { .. } | Self::Timeout { .. } => true,
            _ => false,
        }
    }

    /// The request id, when the failure carried one.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Api(api) => api.request_id.as_deref(),
            Self::Decode { request_id, .. } => request_id.as_deref(),
            _ => None,
        }
    }

    /// The HTTP status, when the failure came from a response.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api(api) => Some(api.status),
            _ => None,
        }
    }
}

fn request_id_suffix(request_id: Option<&str>) -> String {
    request_id.map_or_else(String::new, |id| format!(" [request {id}]"))
}

fn message_suffix(message: Option<&str>) -> String {
    message.map_or_else(String::new, |m| format!(": {m}"))
}
