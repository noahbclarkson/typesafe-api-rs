//! The composition patterns the API documentation recommends, as plain types.
//!
//! Nothing here talks to the network. These are the small pieces of arithmetic
//! that turn calibrated probabilities into decisions, kept in one place so the
//! thresholds live in your code where you can see and change them.

use crate::answer::{ChoiceAnswer, ScoreAnswer};

/// A three-way reading of a confidence value.
///
/// The documentation suggests splitting confidence into act, verify, and
/// escalate rather than thresholding once. Where the boundaries go depends on
/// what a wrong answer costs, so they are always arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gate {
    /// Below the lower bound. Do not act; route to a person or a larger model.
    Low,
    /// Between the bounds. Act with confirmation, or gather more first.
    Medium,
    /// Above the upper bound. Act.
    High,
}

/// Reads a confidence value as a [`Gate`].
///
/// ```
/// use typesafe_api::{Gate, gate};
///
/// assert_eq!(gate(0.95, 0.5, 0.9), Gate::High);
/// assert_eq!(gate(0.70, 0.5, 0.9), Gate::Medium);
/// assert_eq!(gate(0.20, 0.5, 0.9), Gate::Low);
/// ```
pub fn gate(confidence: f64, review_below: f64, act_above: f64) -> Gate {
    if confidence < review_below {
        Gate::Low
    } else if confidence >= act_above {
        Gate::High
    } else {
        Gate::Medium
    }
}

impl ChoiceAnswer {
    /// A three-way reading of this answer's confidence.
    pub fn gate(&self, review_below: f64, act_above: f64) -> Gate {
        gate(self.confidence, review_below, act_above)
    }
}

impl ScoreAnswer {
    /// A three-way reading of this answer's confidence.
    pub fn gate(&self, review_below: f64, act_above: f64) -> Gate {
        gate(self.confidence, review_below, act_above)
    }
}

/// Combines several normalized judgements into one number.
///
/// Scores from rubrics of different lengths are not comparable until they are
/// normalized, so feed this [`ScoreAnswer::normalized`] rather than raw scores.
/// The weights are yours: when the ranking does not match what your team would
/// decide, change a coefficient here rather than rewriting a question.
///
/// ```
/// use typesafe_api::Composite;
///
/// let priority = Composite::new()
///     .weigh(0.6, 0.62)  // severity
///     .weigh(0.3, 0.64)  // frustration
///     .weigh(0.1, 1.0);  // report quality
///
/// assert!((priority.sum() - 0.664).abs() < 1e-9);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Composite {
    total: f64,
    weight: f64,
}

impl Composite {
    /// An empty composite.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one weighted term.
    #[must_use]
    pub fn weigh(mut self, weight: f64, value: f64) -> Self {
        self.total += weight * value;
        self.weight += weight;
        self
    }

    /// The weighted sum. Equals a 0..=1 score when the weights sum to 1.
    pub fn sum(self) -> f64 {
        self.total
    }

    /// The weighted mean, which stays on 0..=1 whatever the weights sum to.
    pub fn mean(self) -> f64 {
        if self.weight == 0.0 {
            0.0
        } else {
            self.total / self.weight
        }
    }

    /// The total weight added so far, for checking that it sums to 1.
    pub fn weight(self) -> f64 {
        self.weight
    }
}
