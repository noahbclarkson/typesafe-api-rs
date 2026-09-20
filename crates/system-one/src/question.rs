//! [`Question`]: the three typed questions a request can ask.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::entry::Entry;

/// One typed question, tagged by `type` on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// A yes/no question.
    Noul(Noul),
    /// A pick-one-option question.
    Choice(Choice),
    /// A rate-against-a-rubric question.
    Score(Score),
}

impl Question {
    /// The wire name of this question type.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Noul(_) => "noul",
            Self::Choice(_) => "choice",
            Self::Score(_) => "score",
        }
    }
}

/// A yes/no question. The answer is the probability that the answer is yes.
///
/// ```
/// use system_one::Noul;
///
/// let q = Noul::new("Has the customer contacted support about this before?")
///     .yes("Mentions a prior attempt, ticket, or that they have asked before")
///     .no("No sign of any previous contact");
/// # let _ = q;
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Noul {
    /// The yes/no question, or a statement for the model to judge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Entry>,
    /// Optional descriptions of what a yes and a no mean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

/// What a yes and a no mean for a [`Noul`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    /// What a value near 1 means. Serialized as `true`.
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub yes: Option<Entry>,
    /// What a value near 0 means. Serialized as `false`.
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub no: Option<Entry>,
}

impl Noul {
    /// A Noul with the given question.
    pub fn new(instructions: impl Into<Entry>) -> Self {
        Self {
            instructions: Some(instructions.into()),
            criteria: None,
        }
    }

    /// Describes what a yes means.
    #[must_use]
    pub fn yes(mut self, description: impl Into<Entry>) -> Self {
        self.criteria.get_or_insert_default().yes = Some(description.into());
        self
    }

    /// Describes what a no means.
    #[must_use]
    pub fn no(mut self, description: impl Into<Entry>) -> Self {
        self.criteria.get_or_insert_default().no = Some(description.into());
        self
    }
}

/// A pick-one-option question. The answer is the chosen option plus the full
/// probability distribution over the options.
///
/// ```
/// use system_one::Choice;
///
/// let q = Choice::new("Which team should handle this?")
///     .option("billing", "Payments, invoicing, refunds")
///     .option("technical", "Bugs, outages, integrations")
///     .plain_option("sales");
/// # let _ = q;
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    /// What the model should decide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Entry>,
    /// Option name to description. `None` leaves an option undescribed.
    pub criteria: IndexMap<String, Option<Entry>>,
}

impl Choice {
    /// A Choice with the given instructions and no options yet.
    pub fn new(instructions: impl Into<Entry>) -> Self {
        Self {
            instructions: Some(instructions.into()),
            criteria: IndexMap::new(),
        }
    }

    /// Adds an option with a description.
    #[must_use]
    pub fn option(mut self, name: impl Into<String>, description: impl Into<Entry>) -> Self {
        self.criteria.insert(name.into(), Some(description.into()));
        self
    }

    /// Adds an option whose name speaks for itself.
    #[must_use]
    pub fn plain_option(mut self, name: impl Into<String>) -> Self {
        self.criteria.insert(name.into(), None);
        self
    }

    /// Adds many described options at once.
    #[must_use]
    pub fn options<I, K, V>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<Entry>,
    {
        for (name, description) in options {
            self.criteria.insert(name.into(), Some(description.into()));
        }
        self
    }

    /// Adds many undescribed options at once.
    #[must_use]
    pub fn plain_options<I, K>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        for name in names {
            self.criteria.insert(name.into(), None);
        }
        self
    }
}

/// A rate-against-a-rubric question. The answer is a probability-weighted
/// position across the levels.
///
/// ```
/// use system_one::Score;
///
/// let q = Score::new("How severe is the reported issue?").levels([
///     "Cosmetic; no impact to functionality",
///     "Broken or degraded feature, but workaround exists",
///     "Blocking issue; no workaround exists",
/// ]);
/// # let _ = q;
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// What the model should rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Entry>,
    /// Level descriptions, lowest first. The index is the level number.
    pub criteria: Vec<Entry>,
}

impl Score {
    /// A Score with the given instructions and no levels yet.
    pub fn new(instructions: impl Into<Entry>) -> Self {
        Self {
            instructions: Some(instructions.into()),
            criteria: Vec::new(),
        }
    }

    /// Appends one level above the levels added so far.
    #[must_use]
    pub fn level(mut self, description: impl Into<Entry>) -> Self {
        self.criteria.push(description.into());
        self
    }

    /// Appends many levels, lowest first.
    #[must_use]
    pub fn levels<I, V>(mut self, levels: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Entry>,
    {
        self.criteria.extend(levels.into_iter().map(Into::into));
        self
    }

    /// The highest level number, which is the top of the returned score range.
    pub fn top_level(&self) -> usize {
        self.criteria.len().saturating_sub(1)
    }
}

impl From<Noul> for Question {
    fn from(value: Noul) -> Self {
        Self::Noul(value)
    }
}

impl From<Choice> for Question {
    fn from(value: Choice) -> Self {
        Self::Choice(value)
    }
}

impl From<Score> for Question {
    fn from(value: Score) -> Self {
        Self::Score(value)
    }
}
