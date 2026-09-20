//! The typed layer: options and levels as enums, evaluations as structs.
//!
//! A Choice offers a closed set of named outcomes, which is what a Rust enum
//! is. A Score walks an ordered rubric, which is what a fieldless enum in
//! declaration order is. These traits carry the descriptions alongside the
//! variants so one declaration serves as both the question and the answer.
//!
//! Implement them by hand for anything the derives cannot express, such as
//! structured object descriptions.

use std::collections::BTreeMap;
use std::fmt;

use indexmap::IndexMap;

use crate::answer::{ChoiceAnswer, ScoreAnswer};
use crate::entry::Entry;
use crate::error::Error;
use crate::request::Questions;
use crate::response::Response;

/// A closed set of Choice options, with the descriptions the model sees.
///
/// Derive it with `#[derive(Options)]` on a fieldless enum.
pub trait Options: Sized {
    /// The options and their descriptions, in declaration order.
    fn criteria() -> IndexMap<String, Option<Entry>>;

    /// Every option name, in declaration order.
    fn variants() -> &'static [&'static str];

    /// The wire name of this variant.
    fn option_name(&self) -> &'static str;

    /// Reads an option name back into a variant.
    fn from_option(name: &str) -> Result<Self, UnknownOption>;
}

/// An ordered Score rubric, with the descriptions the model sees.
///
/// Derive it with `#[derive(Levels)]` on a fieldless enum, lowest level first.
pub trait Levels: Sized {
    /// The level descriptions, lowest first. The index is the level number.
    fn criteria() -> Vec<Entry>;

    /// How many levels there are.
    fn count() -> usize;

    /// The level number of this variant.
    fn level(&self) -> u32;

    /// Reads a level number back into a variant.
    fn from_level(level: u32) -> Option<Self>;
}

/// A set of questions and the answers they produce, declared as one struct.
///
/// Derive it with `#[derive(Evaluation)]`.
pub trait Evaluation: Sized {
    /// The questions this evaluation asks.
    fn questions() -> Questions;

    /// Reads a response into this type.
    fn from_response(response: &Response) -> Result<Self, Error>;
}

/// The answer named an option that is not part of the enum.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the model answered `{answer}`, which is not one of: {}", expected.join(", "))]
pub struct UnknownOption {
    /// What came back.
    pub answer: String,
    /// The options the enum declares.
    pub expected: Vec<&'static str>,
}

/// A Choice answer whose chosen option has been read into `T`.
///
/// ```
/// # use indexmap::IndexMap;
/// # use typesafe_api::{ChoiceOf, Entry, Options, UnknownOption};
/// # #[derive(Debug, PartialEq)]
/// # enum Team { Billing, Technical }
/// # impl Options for Team {
/// #     fn criteria() -> IndexMap<String, Option<Entry>> { IndexMap::new() }
/// #     fn variants() -> &'static [&'static str] { &["billing", "technical"] }
/// #     fn option_name(&self) -> &'static str {
/// #         match self { Team::Billing => "billing", Team::Technical => "technical" }
/// #     }
/// #     fn from_option(name: &str) -> Result<Self, UnknownOption> {
/// #         match name {
/// #             "billing" => Ok(Team::Billing),
/// #             "technical" => Ok(Team::Technical),
/// #             other => Err(UnknownOption { answer: other.to_owned(), expected: Self::variants().to_vec() }),
/// #         }
/// #     }
/// # }
/// # let raw: typesafe_api::ChoiceAnswer = serde_json::from_str(
/// #     r#"{"choice":"billing","confidence":0.9,"probabilities":{"billing":0.9,"technical":0.1}}"#
/// # ).unwrap();
/// let answer: ChoiceOf<Team> = ChoiceOf::from_answer(&raw)?;
/// assert_eq!(answer.value, Team::Billing);
/// assert!((answer.probability(&Team::Technical) - 0.1).abs() < 1e-9);
/// # Ok::<_, UnknownOption>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceOf<T> {
    /// The option with the highest probability.
    pub value: T,
    /// How concentrated the distribution is, from 0 to 1.
    pub confidence: f64,
    /// Every option mapped to its probability, by wire name.
    pub probabilities: IndexMap<String, f64>,
}

impl<T: Options> ChoiceOf<T> {
    /// Reads an untyped answer into this form.
    pub fn from_answer(answer: &ChoiceAnswer) -> Result<Self, UnknownOption> {
        Ok(Self {
            value: T::from_option(&answer.choice)?,
            confidence: answer.confidence,
            probabilities: answer.probabilities.clone(),
        })
    }

    /// The probability assigned to one option. Zero when the API did not
    /// report it.
    pub fn probability(&self, option: &T) -> f64 {
        self.probabilities
            .get(option.option_name())
            .copied()
            .unwrap_or(0.0)
    }

    /// Every option ordered by probability, highest first.
    pub fn ranked(&self) -> Vec<(&str, f64)> {
        let mut ranked: Vec<(&str, f64)> = self
            .probabilities
            .iter()
            .map(|(name, p)| (name.as_str(), *p))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    }

    /// Options other than the chosen one that still hold real probability.
    pub fn runners_up(&self, at_least: f64) -> Vec<(T, f64)> {
        let chosen = self.value.option_name();
        self.probabilities
            .iter()
            .filter(|(name, p)| name.as_str() != chosen && **p >= at_least)
            .filter_map(|(name, p)| Some((T::from_option(name).ok()?, *p)))
            .collect()
    }

    /// A three-way reading of the confidence, for gated routing.
    pub fn gate(&self, review_below: f64, act_above: f64) -> crate::patterns::Gate {
        crate::patterns::gate(self.confidence, review_below, act_above)
    }
}

/// A Score answer whose rubric has been read into `T`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreOf<T> {
    /// Probability-weighted position across the levels.
    pub score: f64,
    /// How concentrated the distribution is, from 0 to 1.
    pub confidence: f64,
    /// Each level number mapped to its probability.
    pub probabilities: BTreeMap<u32, f64>,
    /// Each level number mapped back to the description that was sent.
    pub legend: BTreeMap<u32, Entry>,
    levels: std::marker::PhantomData<fn() -> T>,
}

impl<T: Levels> ScoreOf<T> {
    /// Reads an untyped answer into this form.
    pub fn from_answer(answer: &ScoreAnswer) -> Self {
        Self {
            score: answer.score,
            confidence: answer.confidence,
            probabilities: answer.probabilities.clone(),
            legend: answer.legend.clone(),
            levels: std::marker::PhantomData,
        }
    }

    /// The highest level number in the rubric.
    pub fn top_level(&self) -> u32 {
        u32::try_from(T::count().saturating_sub(1)).unwrap_or(u32::MAX)
    }

    /// The score rescaled to 0..=1, so scales of different lengths compare.
    pub fn normalized(&self) -> f64 {
        let top = f64::from(self.top_level());
        if top == 0.0 { 0.0 } else { self.score / top }
    }

    /// The level the score is closest to.
    pub fn nearest(&self) -> Option<T> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rounded = self.score.round().max(0.0) as u32;
        T::from_level(rounded.min(self.top_level()))
    }

    /// The probability assigned to one level.
    pub fn probability(&self, level: &T) -> f64 {
        self.probabilities
            .get(&level.level())
            .copied()
            .unwrap_or(0.0)
    }

    /// The description that was sent for one level.
    pub fn describe(&self, level: &T) -> Option<&Entry> {
        self.legend.get(&level.level())
    }

    /// A three-way reading of the confidence, for gated routing.
    pub fn gate(&self, review_below: f64, act_above: f64) -> crate::patterns::Gate {
        crate::patterns::gate(self.confidence, review_below, act_above)
    }
}

impl<T> fmt::Display for ChoiceOf<T>
where
    T: Options,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({:.2} confidence)",
            self.value.option_name(),
            self.confidence
        )
    }
}

impl<T> fmt::Display for ScoreOf<T>
where
    T: Levels,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.2} of {} ({:.2} confidence)",
            self.score,
            self.top_level(),
            self.confidence
        )
    }
}

impl Response {
    /// The Choice answer under `id`, with its option read into `T`.
    pub fn choice_as<T: Options>(&self, id: &str) -> Result<ChoiceOf<T>, Error> {
        let answer = self.choice(id)?;
        ChoiceOf::from_answer(answer).map_err(|source| Error::UnknownOption {
            id: id.to_owned(),
            source: Box::new(source),
        })
    }

    /// The Score answer under `id`, tied to the rubric `T`.
    pub fn score_as<T: Levels>(&self, id: &str) -> Result<ScoreOf<T>, Error> {
        Ok(ScoreOf::from_answer(self.score(id)?))
    }

    /// Reads the whole response into an [`Evaluation`] type.
    pub fn extract<T: Evaluation>(&self) -> Result<T, Error> {
        T::from_response(self)
    }
}
