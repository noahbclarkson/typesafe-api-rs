//! [`Answer`]: the typed results that come back, one per question.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::entry::Entry;

/// One answer, matched to the type of the question that produced it.
///
/// A kind this SDK version does not know is kept as [`Answer::Unknown`] with
/// its payload intact rather than dropped, so a newer API stays readable.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// The answer to a [`crate::Noul`].
    Noul(NoulAnswer),
    /// The answer to a [`crate::Choice`].
    Choice(ChoiceAnswer),
    /// The answer to a [`crate::Score`].
    Score(ScoreAnswer),
    /// An answer kind this version does not model.
    Unknown {
        /// The value of the `type` field, or an empty string when absent.
        kind: String,
        /// The untouched JSON payload.
        raw: Box<Value>,
    },
}

impl Answer {
    /// The wire name of this answer kind.
    pub fn kind(&self) -> &str {
        match self {
            Self::Noul(_) => "noul",
            Self::Choice(_) => "choice",
            Self::Score(_) => "score",
            Self::Unknown { kind, .. } => kind,
        }
    }

    /// The Noul payload, if this is a Noul answer.
    pub fn as_noul(&self) -> Option<&NoulAnswer> {
        match self {
            Self::Noul(answer) => Some(answer),
            _ => None,
        }
    }

    /// The Choice payload, if this is a Choice answer.
    pub fn as_choice(&self) -> Option<&ChoiceAnswer> {
        match self {
            Self::Choice(answer) => Some(answer),
            _ => None,
        }
    }

    /// The Score payload, if this is a Score answer.
    pub fn as_score(&self) -> Option<&ScoreAnswer> {
        match self {
            Self::Score(answer) => Some(answer),
            _ => None,
        }
    }

    /// The confidence reported for this answer.
    ///
    /// Choice and Score carry one. A Noul does not: its single probability
    /// already describes the whole two-outcome distribution.
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Choice(answer) => Some(answer.confidence),
            Self::Score(answer) => Some(answer.confidence),
            Self::Noul(_) | Self::Unknown { .. } => None,
        }
    }
}

/// The answer to a yes/no question.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// Probability that the answer is yes, from 0 (no) to 1 (yes).
    pub noul: f64,
}

impl NoulAnswer {
    /// Whether the value clears `threshold`.
    ///
    /// Pick the threshold from the cost of being wrong, not from a default.
    pub fn is_yes(self, threshold: f64) -> bool {
        self.noul >= threshold
    }

    /// Splits the value three ways so unsure cases can take their own path.
    ///
    /// ```
    /// use typesafe_api::{NoulAnswer, Verdict};
    ///
    /// let answer = NoulAnswer { noul: 0.4 };
    /// assert_eq!(answer.verdict(0.2, 0.8), Verdict::Unsure);
    /// ```
    pub fn verdict(self, no_below: f64, yes_above: f64) -> Verdict {
        if self.noul <= no_below {
            Verdict::No
        } else if self.noul >= yes_above {
            Verdict::Yes
        } else {
            Verdict::Unsure
        }
    }
}

/// A three-way reading of a [`NoulAnswer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Below the lower threshold.
    No,
    /// Between the thresholds. Worth a person, not a branch.
    Unsure,
    /// Above the upper threshold.
    Yes,
}

/// The answer to a pick-one-option question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    /// The option with the highest probability.
    pub choice: String,
    /// How concentrated the distribution is, from 0 to 1.
    pub confidence: f64,
    /// Every option mapped to its probability. The values sum to about 1.
    pub probabilities: IndexMap<String, f64>,
}

impl ChoiceAnswer {
    /// The probability assigned to one option, if the option was offered.
    pub fn probability(&self, option: &str) -> Option<f64> {
        self.probabilities.get(option).copied()
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
    ///
    /// Useful for copying a second team in, as the routing examples do.
    pub fn runners_up(&self, at_least: f64) -> Vec<(&str, f64)> {
        self.probabilities
            .iter()
            .filter(|(name, p)| name.as_str() != self.choice && **p >= at_least)
            .map(|(name, p)| (name.as_str(), *p))
            .collect()
    }
}

/// The answer to a rate-against-a-rubric question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    /// Probability-weighted position across the levels. May land between them.
    pub score: f64,
    /// How concentrated the distribution is, from 0 to 1.
    pub confidence: f64,
    /// Each level number mapped back to the description that was sent.
    pub legend: BTreeMap<u32, Entry>,
    /// Each level mapped to its probability. The values sum to about 1.
    pub probabilities: BTreeMap<u32, f64>,
}

impl ScoreAnswer {
    /// The highest level number in the rubric.
    pub fn top_level(&self) -> u32 {
        self.legend.keys().copied().max().unwrap_or(0)
    }

    /// The score rescaled to 0..=1 by dividing by the top level number.
    ///
    /// Scales of different lengths are not comparable until they are
    /// normalized, so do this before combining scores with weights.
    pub fn normalized(&self) -> f64 {
        let top = f64::from(self.top_level());
        if top == 0.0 { 0.0 } else { self.score / top }
    }

    /// The level the score is closest to.
    pub fn nearest_level(&self) -> u32 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rounded = self.score.round().max(0.0) as u32;
        rounded.min(self.top_level())
    }

    /// The probability assigned to one level.
    pub fn probability(&self, level: u32) -> Option<f64> {
        self.probabilities.get(&level).copied()
    }

    /// The description that was sent for one level.
    pub fn describe(&self, level: u32) -> Option<&Entry> {
        self.legend.get(&level)
    }
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        match kind.as_str() {
            "noul" => serde_json::from_value(value)
                .map(Self::Noul)
                .map_err(D::Error::custom),
            "choice" => serde_json::from_value(value)
                .map(Self::Choice)
                .map_err(D::Error::custom),
            "score" => serde_json::from_value(value)
                .map(Self::Score)
                .map_err(D::Error::custom),
            _ => Ok(Self::Unknown {
                kind,
                raw: Box::new(value),
            }),
        }
    }
}

impl Serialize for Answer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = match self {
            Self::Unknown { raw, .. } => (**raw).clone(),
            Self::Noul(answer) => tagged("noul", answer).map_err(S::Error::custom)?,
            Self::Choice(answer) => tagged("choice", answer).map_err(S::Error::custom)?,
            Self::Score(answer) => tagged("score", answer).map_err(S::Error::custom)?,
        };
        value.serialize(serializer)
    }
}

fn tagged(kind: &str, answer: &impl Serialize) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(answer)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("type".to_owned(), Value::String(kind.to_owned()));
    }
    Ok(value)
}
