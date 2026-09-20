//! The asynchronous client.

use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::config::{ClientBuilder, Config};
use crate::error::Error;
use crate::model::{ModelCard, ModelList};
use crate::request::{IntoQuestions, Questions, Request};
use crate::response::Response;
use crate::retry::RetryPolicy;
use crate::state::IntoState;
use crate::transport::{self, Attempts, Call, CallOptions, EVALUATE_PATH, MODELS_PATH};
use crate::validate::{self, Limits};

/// An asynchronous client for the System One API.
///
/// Cloning is cheap: clones share one connection pool and one configuration.
/// Build one per process and pass it around.
///
/// ```no_run
/// use typesafe_api::{Client, Noul, questions};
///
/// # async fn run() -> Result<(), typesafe_api::Error> {
/// let client = Client::from_env()?;
///
/// let answers = client
///     .evaluate("My card was charged twice.", questions! {
///         "billing" => Noul::new("Is this about billing?"),
///     })
///     .await?;
///
/// assert!(answers.noul("billing")?.is_yes(0.8));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: Config,
    http: reqwest::Client,
}

impl Client {
    /// Builds a client from `TYPESAFE_API_KEY` and the other environment
    /// variables, with defaults for everything else.
    pub fn from_env() -> Result<Self, Error> {
        ClientBuilder::new().build()
    }

    /// Builds a client with an explicit API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        ClientBuilder::new().api_key(api_key).build()
    }

    /// Starts configuring a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    pub(crate) fn from_parts(config: Config, http: reqwest::Client) -> Self {
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

    /// Evaluates `state` against `questions`.
    ///
    /// The returned value is awaited directly for the common case, and carries
    /// per-call overrides for everything else.
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
    ///
    /// Useful for logging, for a snapshot test, or for handing to
    /// [`Client::send`] later.
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
    pub async fn send(&self, request: &Request) -> Result<Response, Error> {
        self.post(request, &CallOptions::default()).await
    }

    /// Sends a request and returns the response body untouched.
    ///
    /// The escape hatch for anything this crate does not model yet. Retries,
    /// timeouts, and error handling still apply.
    pub async fn send_raw(&self, request: &Request) -> Result<Value, Error> {
        let options = CallOptions::default();
        let call = options.resolve(&self.inner.config, EVALUATE_PATH)?;
        validate::check(request, options.limits(&self.inner.config))?;
        self.run(&call, Some(request), transport::decode).await
    }

    /// Evaluates `state` against the questions declared by `T`, and reads the
    /// answers back into `T`.
    ///
    /// The same call split in two is [`Client::evaluate`] with
    /// `T::questions()`, then [`Response::extract`].
    pub fn evaluate_as<T: crate::typed::Evaluation, S: IntoState>(
        &self,
        state: S,
    ) -> EvaluateAs<'_, T> {
        EvaluateAs {
            inner: self.evaluate(state, T::questions()),
            target: PhantomData,
        }
    }

    /// Lists the models this account may use.
    pub fn models(&self) -> Models<'_> {
        Models {
            client: self,
            options: CallOptions::default(),
        }
    }

    pub(crate) async fn post(
        &self,
        request: &Request,
        options: &CallOptions,
    ) -> Result<Response, Error> {
        let call = options.resolve(&self.inner.config, EVALUATE_PATH)?;
        validate::check(request, options.limits(&self.inner.config))?;
        self.run(&call, Some(request), |bytes, url, id| {
            let mut response: Response = transport::decode(bytes, url, id.clone())?;
            response.request_id = id;
            Ok(response)
        })
        .await
    }

    async fn run<T, F>(&self, call: &Call, body: Option<&Request>, parse: F) -> Result<T, Error>
    where
        F: Fn(&[u8], &str, Option<String>) -> Result<T, Error>,
    {
        let authorization = transport::authorization(&self.inner.config)?;
        let mut attempts = Attempts::new(call.retry.clone(), call.deadline);

        loop {
            let attempt = attempts.begin();
            transport::log_attempt(&call.url, attempt, body);

            let outcome = self
                .attempt(
                    call,
                    body,
                    &authorization,
                    attempts.attempt_timeout(call.timeout),
                    &parse,
                )
                .await;

            match outcome {
                Ok(value) => return Ok(value),
                Err(error) => match attempts.next_wait(&error) {
                    Some(wait) => {
                        transport::log_retry(&call.url, attempt, wait, &error);
                        tokio::time::sleep(wait).await;
                    }
                    None => {
                        return Err(transport::finalize(error, attempts.elapsed(), attempt));
                    }
                },
            }
        }
    }

    async fn attempt<T, F>(
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
            .await
            .map_err(|err| transport::transport_error(&call.url, err))?;

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .bytes()
            .await
            .map_err(|err| transport::transport_error(&call.url, err))?;

        if !status.is_success() {
            return Err(transport::api_error(status, &call.url, &headers, &bytes));
        }
        parse(&bytes, &call.url, transport::request_id(&headers))
    }
}

/// A pending evaluation, which is also a builder for per-call overrides.
///
/// Await it, or call [`Evaluate::send`]. Either way nothing happens until then.
#[must_use = "an evaluation does nothing until it is awaited"]
pub struct Evaluate<'a> {
    client: &'a Client,
    parts: Result<(crate::state::State, Questions), crate::state::InvalidState>,
    options: CallOptions,
    extra: serde_json::Map<String, Value>,
}

macro_rules! call_overrides {
    ($name:ident) => {
        impl $name<'_> {
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

            /// Adds a header to this call only.
            pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
                self.options.headers.push((name.into(), value.into()));
                self
            }
        }
    };
}

call_overrides!(Evaluate);
call_overrides!(Models);

impl Evaluate<'_> {
    /// Overrides which local checks run for this call only.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.options.limits = Some(limits);
        self
    }

    /// Adds a top-level field to the request body.
    ///
    /// The forward-compatibility hatch: send a field the API has gained and
    /// this crate has not. Values are merged over the body, so a name that
    /// collides with `state`, `model`, or `questions` replaces it.
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
    pub async fn send(self) -> Result<Response, Error> {
        let request = self.to_request()?;
        self.client.post(&request, &self.options).await
    }
}

impl<'a> IntoFuture for Evaluate<'a> {
    type Output = Result<Response, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

/// A pending model listing, which is also a builder for per-call overrides.
#[must_use = "a listing does nothing until it is awaited"]
pub struct Models<'a> {
    client: &'a Client,
    options: CallOptions,
}

impl Models<'_> {
    /// Sends the call.
    pub async fn send(self) -> Result<Vec<ModelCard>, Error> {
        let call = self
            .options
            .resolve(&self.client.inner.config, MODELS_PATH)?;
        let list: ModelList = self
            .client
            .run(&call, None, transport::decode::<ModelList>)
            .await?;
        Ok(list.models)
    }
}

impl<'a> IntoFuture for Models<'a> {
    type Output = Result<Vec<ModelCard>, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

/// A pending typed evaluation, which is also a builder for per-call overrides.
#[must_use = "an evaluation does nothing until it is awaited"]
pub struct EvaluateAs<'a, T> {
    inner: Evaluate<'a>,
    target: PhantomData<fn() -> T>,
}

impl<T: crate::typed::Evaluation> EvaluateAs<'_, T> {
    /// Overrides the model for this call only.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.inner = self.inner.model(model);
        self
    }

    /// Overrides the per-attempt timeout for this call only.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.inner = self.inner.timeout(timeout);
        self
    }

    /// Caps the wall time of this call, retries included.
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.inner = self.inner.deadline(deadline);
        self
    }

    /// Overrides the retry policy for this call only.
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.inner = self.inner.retry(retry);
        self
    }

    /// Adds a header to this call only.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner = self.inner.header(name, value);
        self
    }

    /// The exact request this call will send.
    pub fn to_request(&self) -> Result<Request, Error> {
        self.inner.to_request()
    }

    /// Sends the call and reads the answers into `T`.
    pub async fn send(self) -> Result<T, Error> {
        self.inner.send().await?.extract()
    }
}

impl<'a, T: crate::typed::Evaluation + 'a> IntoFuture for EvaluateAs<'a, T> {
    type Output = Result<T, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}
