//! The synchronous client, for scripts, CLIs, and codebases without a runtime.
//!
//! Every type except the client itself is shared with the asynchronous side, so
//! questions, answers, errors, and policies move between the two unchanged.
//!
//! Do not call this from inside an async runtime: it blocks the thread it runs
//! on, and `reqwest` will refuse to start its own runtime there. Use
//! [`crate::Client`] instead, or `spawn_blocking`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::config::{ClientBuilder, Config};
use crate::error::Error;
use crate::model::ModelCard;
use crate::model::ModelList;
use crate::request::{IntoQuestions, Questions, Request};
use crate::response::Response;
use crate::retry::RetryPolicy;
use crate::state::{IntoState, InvalidState, State};
use crate::transport::{self, Attempts, Call, CallOptions, EVALUATE_PATH, MODELS_PATH};
use crate::validate::{self, Limits};

/// A synchronous client for the System One API.
///
/// Cloning is cheap: clones share one connection pool and one configuration.
///
/// ```no_run
/// use typesafe_api::{Noul, blocking::Client, questions};
///
/// let client = Client::from_env()?;
/// let answers = client
///     .evaluate("My card was charged twice.", questions! {
///         "billing" => Noul::new("Is this about billing?"),
///     })
///     .send()?;
///
/// println!("{}", answers.noul("billing")?.noul);
/// # Ok::<_, typesafe_api::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: Config,
    http: reqwest::blocking::Client,
}

impl Client {
    /// Builds a client from `TYPESAFE_API_KEY` and the other environment
    /// variables, with defaults for everything else.
    pub fn from_env() -> Result<Self, Error> {
        ClientBuilder::new().build_blocking()
    }

    /// Builds a client with an explicit API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        ClientBuilder::new().api_key(api_key).build_blocking()
    }

    /// Starts configuring a client. Finish with
    /// [`ClientBuilder::build_blocking`].
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    pub(crate) fn from_parts(config: Config, http: reqwest::blocking::Client) -> Self {
        Self {
            inner: Arc::new(Inner { config, http }),
        }
    }

    /// The model used when a call does not name one.
    pub fn model(&self) -> &str {
        &self.inner.config.model
    }

    /// The API root this client talks to.
    pub fn base_url(&self) -> &str {
        &self.inner.config.base_url
    }

    /// Evaluates `state` against `questions`. Finish with [`Evaluate::send`].
    pub fn evaluate<S: IntoState, Q: IntoQuestions>(&self, state: S, questions: Q) -> Evaluate<'_> {
        Evaluate {
            client: self,
            parts: state
                .into_state()
                .map(|state| (state, questions.into_questions())),
            options: CallOptions::default(),
            extra: serde_json::Map::new(),
        }
    }

    /// Builds the request a call would send, without sending it.
    pub fn request<S: IntoState, Q: IntoQuestions>(
        &self,
        state: S,
        questions: Q,
    ) -> Result<Request, Error> {
        Ok(Request {
            state: state.into_state()?,
            model: self.inner.config.model.clone(),
            questions: questions.into_questions(),
            extra: serde_json::Map::new(),
        })
    }

    /// Sends a request that has already been built.
    pub fn send(&self, request: &Request) -> Result<Response, Error> {
        self.post(request, &CallOptions::default())
    }

    /// Sends a request and returns the response body untouched.
    pub fn send_raw(&self, request: &Request) -> Result<Value, Error> {
        let options = CallOptions::default();
        let call = options.resolve(&self.inner.config, EVALUATE_PATH)?;
        validate::check(request, options.limits(&self.inner.config))?;
        self.run(&call, Some(request), transport::decode)
    }

    /// Lists the models this account may use.
    pub fn models(&self) -> Result<Vec<ModelCard>, Error> {
        let options = CallOptions::default();
        let call = options.resolve(&self.inner.config, MODELS_PATH)?;
        let list: ModelList = self.run(&call, None, transport::decode::<ModelList>)?;
        Ok(list.models)
    }

    fn post(&self, request: &Request, options: &CallOptions) -> Result<Response, Error> {
        let call = options.resolve(&self.inner.config, EVALUATE_PATH)?;
        validate::check(request, options.limits(&self.inner.config))?;
        self.run(&call, Some(request), |bytes, url, id| {
            let mut response: Response = transport::decode(bytes, url, id.clone())?;
            response.request_id = id;
            Ok(response)
        })
    }

    fn run<T, F>(&self, call: &Call, body: Option<&Request>, parse: F) -> Result<T, Error>
    where
        F: Fn(&[u8], &str, Option<String>) -> Result<T, Error>,
    {
        let authorization = transport::authorization(&self.inner.config)?;
        let mut attempts = Attempts::new(call.retry.clone(), call.deadline);

        loop {
            let attempt = attempts.begin();
            transport::log_attempt(&call.url, attempt, body);

            let outcome = self.attempt(
                call,
                body,
                &authorization,
                attempts.attempt_timeout(call.timeout),
                &parse,
            );

            match outcome {
                Ok(value) => return Ok(value),
                Err(error) => match attempts.next_wait(&error) {
                    Some(wait) => {
                        transport::log_retry(&call.url, attempt, wait, &error);
                        std::thread::sleep(wait);
                    }
                    None => return Err(transport::finalize(error, attempts.elapsed(), attempt)),
                },
            }
        }
    }

    fn attempt<T, F>(
        &self,
        call: &Call,
        body: Option<&Request>,
        authorization: &reqwest::header::HeaderValue,
        timeout: Option<Duration>,
        parse: &F,
    ) -> Result<T, Error>
    where
        F: Fn(&[u8], &str, Option<String>) -> Result<T, Error>,
    {
        let method = if body.is_some() {
            reqwest::Method::POST
        } else {
            reqwest::Method::GET
        };
        let mut builder = self
            .inner
            .http
            .request(method, &call.url)
            .headers(call.headers.clone())
            .header(reqwest::header::AUTHORIZATION, authorization.clone());

        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(body) = body {
            builder = builder.json(body);
        }

        let response = builder
            .send()
            .map_err(|err| transport::transport_error(&call.url, err))?;

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .bytes()
            .map_err(|err| transport::transport_error(&call.url, err))?;

        if !status.is_success() {
            return Err(transport::api_error(status, &call.url, &headers, &bytes));
        }
        parse(&bytes, &call.url, transport::request_id(&headers))
    }
}

/// A prepared evaluation. Nothing is sent until [`Evaluate::send`].
#[must_use = "an evaluation does nothing until it is sent"]
pub struct Evaluate<'a> {
    client: &'a Client,
    parts: Result<(State, Questions), InvalidState>,
    options: CallOptions,
    extra: serde_json::Map<String, Value>,
}

impl Evaluate<'_> {
    /// Overrides the model for this call only.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.options.model = Some(model.into());
        self
    }

    /// Overrides the per-attempt timeout for this call only.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.options.timeout = Some(Some(timeout));
        self
    }

    /// Caps the wall time of this call, retries included.
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.options.deadline = Some(deadline);
        self
    }

    /// Overrides the retry policy for this call only.
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.options.retry = Some(retry);
        self
    }

    /// Overrides which local checks run for this call only.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.options.limits = Some(limits);
        self
    }

    /// Adds a header to this call only.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.headers.push((name.into(), value.into()));
        self
    }

    /// Adds a top-level field to the request body, for a field the API has
    /// gained and this crate has not.
    pub fn extra_field(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extra.insert(name.into(), value.into());
        self
    }

    /// The exact request this call will send.
    pub fn to_request(&self) -> Result<Request, Error> {
        let (state, questions) = self.parts.clone().map_err(Error::from)?;
        Ok(Request {
            state,
            model: self.options.model(&self.client.inner.config).to_owned(),
            questions,
            extra: self.extra.clone(),
        })
    }

    /// Sends the call.
    pub fn send(self) -> Result<Response, Error> {
        let request = self.to_request()?;
        self.client.post(&request, &self.options)
    }

    /// Sends the call and reads the answers into `T`.
    pub fn send_as<T: crate::typed::Evaluation>(self) -> Result<T, Error> {
        self.send()?.extract()
    }
}

impl Client {
    /// Evaluates `state` against the questions declared by `T`, and reads the
    /// answers back into `T`.
    pub fn evaluate_as<T: crate::typed::Evaluation, S: IntoState>(
        &self,
        state: S,
    ) -> Result<T, Error> {
        self.evaluate(state, T::questions()).send()?.extract()
    }
}
