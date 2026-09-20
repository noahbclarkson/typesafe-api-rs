//! Bounded-concurrency fan-out over many states.
//!
//! One request answers many questions about one state. To ask about many
//! states, send many requests, and the only real question is how many at once.
//! Too few wastes time; too many earns a `429`.

use std::time::Duration;

use futures_util::stream::{Stream, StreamExt as _};

use crate::client::Client;
use crate::error::Error;
use crate::request::{IntoQuestions, Questions};
use crate::response::Response;
use crate::retry::RetryPolicy;
use crate::state::IntoState;

/// How many requests are in flight at once when nothing says otherwise.
pub const DEFAULT_CONCURRENCY: usize = 8;

/// A prepared fan-out. Nothing is sent until it is streamed or collected.
#[must_use = "a fan-out does nothing until it is streamed or collected"]
pub struct FanOut<'a, I> {
    client: &'a Client,
    states: I,
    questions: Questions,
    concurrency: usize,
    model: Option<String>,
    timeout: Option<Duration>,
    retry: Option<RetryPolicy>,
}

impl Client {
    /// Evaluates the same questions against many states.
    ///
    /// ```no_run
    /// use futures_util::StreamExt;
    /// use typesafe_api::{Client, Noul, questions};
    ///
    /// # async fn run() -> Result<(), typesafe_api::Error> {
    /// let client = Client::from_env()?;
    /// let tickets = vec!["I was charged twice", "How do I reset my password?"];
    ///
    /// let mut results = client
    ///     .evaluate_many(tickets, questions! {
    ///         "billing" => Noul::new("Is this about billing?"),
    ///     })
    ///     .concurrency(16)
    ///     .stream();
    ///
    /// while let Some(result) = results.next().await {
    ///     println!("{:?}", result?.noul("billing")?.noul);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
    pub fn evaluate_many<I, Q>(&self, states: I, questions: Q) -> FanOut<'_, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: IntoState,
        Q: IntoQuestions,
    {
        FanOut {
            client: self,
            states: states.into_iter(),
            questions: questions.into_questions(),
            concurrency: DEFAULT_CONCURRENCY,
            model: None,
            timeout: None,
            retry: None,
        }
    }
}

impl<'a, I> FanOut<'a, I>
where
    I: Iterator + 'a,
    I::Item: IntoState,
{
    /// Sets how many requests may be in flight at once.
    ///
    /// Keep this under the account rate limit. Retries are per request, so a
    /// burst that trips a `429` costs latency rather than answers.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Overrides the model for every request in this fan-out.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Overrides the per-attempt timeout for every request in this fan-out.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Overrides the retry policy for every request in this fan-out.
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Streams the results in the order the states were given.
    pub fn stream(self) -> impl Stream<Item = Result<Response, Error>> + 'a {
        let Self {
            client,
            states,
            questions,
            concurrency,
            model,
            timeout,
            retry,
        } = self;

        futures_util::stream::iter(states)
            .map(move |state| {
                let mut call = client.evaluate(state, questions.clone());
                if let Some(model) = model.clone() {
                    call = call.model(model);
                }
                if let Some(timeout) = timeout {
                    call = call.timeout(timeout);
                }
                if let Some(retry) = retry.clone() {
                    call = call.retry(retry);
                }
                call.send()
            })
            .buffered(concurrency)
    }

    /// Collects every result, in order, stopping at the first failure.
    pub async fn try_collect(self) -> Result<Vec<Response>, Error> {
        let mut stream = Box::pin(self.stream());
        let mut collected = Vec::new();
        while let Some(result) = stream.next().await {
            collected.push(result?);
        }
        Ok(collected)
    }

    /// Collects every result, in order, keeping the failures.
    ///
    /// Prefer this over a corpus: one bad state should not discard the rest.
    pub async fn collect_all(self) -> Vec<Result<Response, Error>> {
        let mut stream = Box::pin(self.stream());
        let mut collected = Vec::new();
        while let Some(result) = stream.next().await {
            collected.push(result);
        }
        collected
    }
}
