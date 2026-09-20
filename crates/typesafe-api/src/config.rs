//! Client configuration: the builder, the environment variables, the defaults.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::SecretString;

use crate::error::Error;
use crate::model::JEV_LATEST;
use crate::retry::RetryPolicy;
use crate::validate::Limits;

/// The environment variable holding the API key.
pub const API_KEY_ENV: &str = "TYPESAFE_API_KEY";
/// The environment variable overriding the API root.
pub const BASE_URL_ENV: &str = "TYPESAFE_BASE_URL";
/// The environment variable overriding the default model.
pub const DEFAULT_MODEL_ENV: &str = "TYPESAFE_DEFAULT_MODEL";

/// The API root used when nothing overrides it.
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
/// The timeout applied to a single attempt when nothing overrides it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Everything a client needs, resolved from explicit values, then the
/// environment, then defaults.
///
/// `SecretString` keeps the key out of the `Debug` output by construction, so
/// a logged client config cannot leak it.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) api_key: SecretString,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) timeout: Option<Duration>,
    pub(crate) deadline: Option<Duration>,
    pub(crate) retry: RetryPolicy,
    pub(crate) limits: Limits,
    pub(crate) headers: HeaderMap,
}

/// Builds a client.
///
/// Explicit values win over environment variables, which win over defaults. An
/// empty or whitespace-only environment value is treated as unset.
///
/// ```no_run
/// use std::time::Duration;
/// use typesafe_api::{Client, RetryPolicy};
///
/// let client = Client::builder()
///     .model("jev-1.13.0")
///     .timeout(Duration::from_secs(5))
///     .deadline(Duration::from_secs(20))
///     .retry(RetryPolicy::default().max_retries(4))
///     .header("x-team", "payments")
///     .build()?;
/// # Ok::<_, typesafe_api::Error>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    #[allow(
        clippy::option_option,
        reason = "three states: inherit the default, set a timeout, or have none"
    )]
    timeout: Option<Option<Duration>>,
    deadline: Option<Duration>,
    retry: Option<RetryPolicy>,
    limits: Option<Limits>,
    headers: Vec<(String, String)>,
    http: Option<reqwest::Client>,
    #[cfg(feature = "blocking")]
    blocking_http: Option<reqwest::blocking::Client>,
}

impl ClientBuilder {
    /// A builder with nothing set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the API key, in place of `TYPESAFE_API_KEY`.
    #[must_use]
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Sets the API root, in place of `TYPESAFE_BASE_URL`.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Sets the model used when a call does not name one.
    ///
    /// Pin a version such as `jev-1.13.0` when confidence thresholds have been
    /// tuned against it; an alias moves when a new model ships.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the timeout for a single attempt.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(Some(timeout));
        self
    }

    /// Removes the per-attempt timeout.
    #[must_use]
    pub fn no_timeout(mut self) -> Self {
        self.timeout = Some(None);
        self
    }

    /// Caps the wall time of a whole call, retries and backoff included.
    ///
    /// Without one, a call can take up to `timeout * (max_retries + 1)` plus
    /// the backoff waits.
    #[must_use]
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Sets the retry policy.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Sets which local checks run before a request is sent.
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Adds a header to every request.
    ///
    /// `authorization` and `content-type` are set by the client and cannot be
    /// replaced here.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Uses a caller-supplied HTTP client, for a shared pool, a proxy, or a
    /// test transport.
    #[must_use]
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// Uses a caller-supplied blocking HTTP client.
    #[cfg(feature = "blocking")]
    #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
    #[must_use]
    pub fn blocking_http_client(mut self, client: reqwest::blocking::Client) -> Self {
        self.blocking_http = Some(client);
        self
    }

    /// Builds the asynchronous client.
    pub fn build(self) -> Result<crate::client::Client, Error> {
        let http = match &self.http {
            Some(client) => client.clone(),
            None => reqwest::Client::builder()
                .build()
                .map_err(|err| Error::Config(format!("could not build an HTTP client: {err}")))?,
        };
        Ok(crate::client::Client::from_parts(self.resolve()?, http))
    }

    /// Builds the synchronous client.
    #[cfg(feature = "blocking")]
    #[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
    pub fn build_blocking(self) -> Result<crate::blocking::Client, Error> {
        let http = match &self.blocking_http {
            Some(client) => client.clone(),
            None => reqwest::blocking::Client::builder()
                .build()
                .map_err(|err| {
                    Error::Config(format!("could not build a blocking HTTP client: {err}"))
                })?,
        };
        Ok(crate::blocking::Client::from_parts(self.resolve()?, http))
    }

    fn resolve(self) -> Result<Config, Error> {
        let api_key = self
            .api_key
            .or_else(|| env(API_KEY_ENV))
            .ok_or(Error::MissingApiKey { env: API_KEY_ENV })?;
        if api_key.trim().is_empty() {
            return Err(Error::MissingApiKey { env: API_KEY_ENV });
        }

        let base_url = self
            .base_url
            .or_else(|| env(BASE_URL_ENV))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let base_url = base_url.trim_end_matches('/').to_owned();
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(Error::Config(format!(
                "base URL must start with http:// or https://; got {base_url}"
            )));
        }

        let model = self
            .model
            .or_else(|| env(DEFAULT_MODEL_ENV))
            .unwrap_or_else(|| JEV_LATEST.to_owned());

        let timeout = self.timeout.unwrap_or(Some(DEFAULT_TIMEOUT));
        if let Some(timeout) = timeout
            && timeout.is_zero()
        {
            return Err(Error::Config(
                "timeout must be greater than zero".to_owned(),
            ));
        }
        if let (Some(deadline), Some(timeout)) = (self.deadline, timeout)
            && deadline < timeout
        {
            return Err(Error::Config(format!(
                "deadline ({deadline:?}) is shorter than the per-attempt timeout ({timeout:?}), \
                 so no attempt could finish"
            )));
        }

        let mut headers = HeaderMap::new();
        for (name, value) in self.headers {
            let header = HeaderName::try_from(name.as_str())
                .map_err(|_| Error::Config(format!("{name} is not a valid HTTP header name")))?;
            if header == reqwest::header::AUTHORIZATION || header == reqwest::header::CONTENT_TYPE {
                return Err(Error::Config(format!(
                    "{header} is set by the client and cannot be overridden"
                )));
            }
            let value = HeaderValue::try_from(value.as_str()).map_err(|_| {
                Error::Config(format!(
                    "the value for {header} is not a valid header value"
                ))
            })?;
            headers.insert(header, value);
        }

        Ok(Config {
            api_key: SecretString::from(api_key),
            base_url,
            model,
            timeout,
            deadline: self.deadline,
            retry: self.retry.unwrap_or_default(),
            limits: self.limits.unwrap_or_default(),
            headers,
        })
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
