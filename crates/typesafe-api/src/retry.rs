//! [`RetryPolicy`]: when a failed attempt is worth repeating, and how long to
//! wait first.

use std::time::Duration;

use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};

use crate::error::Error;

/// How the client retries a failed attempt.
///
/// The defaults follow the guidance in the API reference: back off
/// exponentially on `429` and `5xx`, honour `Retry-After` when the server sends
/// one, and give up after a couple of tries rather than hammering a service
/// that is already struggling.
///
/// ```
/// use std::time::Duration;
/// use typesafe_api::RetryPolicy;
///
/// // Patient: more attempts, longer waits.
/// let patient = RetryPolicy::default().max_retries(5).backoff_max(Duration::from_secs(30));
///
/// // Impatient: fail fast and let the caller decide.
/// let none = RetryPolicy::none();
/// assert_eq!(none.max_retries, 0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct RetryPolicy {
    /// Attempts after the first. `0` disables retrying.
    pub max_retries: u32,
    /// The first backoff delay. Each later delay doubles it.
    pub backoff_initial: Duration,
    /// The ceiling on any single backoff delay.
    pub backoff_max: Duration,
    /// Fraction of each delay randomly removed, to spread out a thundering herd.
    pub backoff_jitter: f64,
    /// Whether `Retry-After` and `retry-after-ms` are obeyed.
    pub respect_retry_after: bool,
    /// The longest server-requested wait that is obeyed. Longer waits fall back
    /// to the backoff schedule, so a misbehaving header cannot stall a caller.
    pub max_retry_after: Duration,
    /// Whether a request that never reached the server is retried.
    pub retry_connect_errors: bool,
    /// Whether an attempt that exceeded its timeout is retried.
    pub retry_timeouts: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            backoff_initial: Duration::from_millis(500),
            backoff_max: Duration::from_secs(5),
            backoff_jitter: 0.25,
            respect_retry_after: true,
            max_retry_after: Duration::from_secs(60),
            retry_connect_errors: true,
            retry_timeouts: true,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }

    /// Sets the number of attempts after the first.
    #[must_use]
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Sets the first backoff delay.
    #[must_use]
    pub fn backoff_initial(mut self, delay: Duration) -> Self {
        self.backoff_initial = delay;
        self
    }

    /// Sets the ceiling on any single backoff delay.
    #[must_use]
    pub fn backoff_max(mut self, delay: Duration) -> Self {
        self.backoff_max = delay;
        self
    }

    /// Sets the fraction of each delay that is randomly removed, from 0 to 1.
    #[must_use]
    pub fn backoff_jitter(mut self, jitter: f64) -> Self {
        self.backoff_jitter = jitter.clamp(0.0, 1.0);
        self
    }

    /// Sets whether `Retry-After` is obeyed.
    #[must_use]
    pub fn respect_retry_after(mut self, respect: bool) -> Self {
        self.respect_retry_after = respect;
        self
    }

    /// Sets the longest server-requested wait that is obeyed.
    #[must_use]
    pub fn max_retry_after(mut self, wait: Duration) -> Self {
        self.max_retry_after = wait;
        self
    }

    /// Whether this error is one this policy retries.
    pub fn allows(&self, error: &Error) -> bool {
        match error {
            Error::Api(api) => api.kind.is_retryable(),
            Error::Transport { .. } => self.retry_connect_errors,
            Error::Timeout { .. } => self.retry_timeouts,
            _ => false,
        }
    }

    /// The doubling, capped delay schedule, one entry per allowed retry.
    ///
    /// Jitter is applied separately by [`RetryPolicy::wait`], because this
    /// policy expresses it as a fraction of the delay rather than as a flag.
    pub(crate) fn schedule(&self) -> ExponentialBackoff {
        ExponentialBuilder::default()
            .with_min_delay(self.backoff_initial)
            .with_max_delay(self.backoff_max)
            .with_max_times(usize::try_from(self.max_retries).unwrap_or(usize::MAX))
            .build()
    }

    /// The wait before the next attempt.
    ///
    /// A sane, permitted `Retry-After` is taken as given. Otherwise the next
    /// step of the backoff schedule is used, with jitter subtracted.
    pub(crate) fn wait(&self, requested: Option<Duration>, scheduled: Duration) -> Duration {
        match requested {
            Some(wait) if self.respect_retry_after && wait <= self.max_retry_after => wait,
            _ => scheduled.mul_f64(1.0 - self.backoff_jitter * random_unit()),
        }
    }
}

/// A random number in `[0, 1)`.
///
/// `RandomState` is seeded randomly per process and advances on every
/// construction, which is all jitter needs. Pulling in a random number
/// generator for this would not earn its compile time.
#[allow(
    clippy::cast_precision_loss,
    reason = "53 bits is exactly what an f64 mantissa holds"
)]
fn random_unit() -> f64 {
    use std::hash::{BuildHasher, Hasher};

    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}
