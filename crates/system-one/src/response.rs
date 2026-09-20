//! The response body and the accessors that read it.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::answer::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer};
use crate::error::Error;

/// One evaluation, with an answer under each question id you chose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The versioned model that answered, such as `jev-1.13.0`.
    pub model: String,
    /// One answer per question, keyed by the ids from the request.
    #[serde(default)]
    pub answers: IndexMap<String, Answer>,
    /// Token usage for the request.
    #[serde(default)]
    pub usage: Usage,
    /// The `x-typesafe-request-id` header, when the server sent one.
    #[serde(skip)]
    pub request_id: Option<String>,
}

/// Token counts for one request. Only input tokens are billed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens consumed, when reported.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Output tokens produced, when reported.
    #[serde(default)]
    pub output_tokens: Option<u64>,
}

impl Response {
    /// The answer under `id`, whatever its kind.
    pub fn get(&self, id: &str) -> Option<&Answer> {
        self.answers.get(id)
    }

    /// The Noul answer under `id`.
    ///
    /// Fails with the ids that are present when `id` is missing, and with the
    /// kind that was found when it is the wrong shape.
    pub fn noul(&self, id: &str) -> Result<&NoulAnswer, Error> {
        self.expect(id, "noul", Answer::as_noul)
    }

    /// The Choice answer under `id`.
    pub fn choice(&self, id: &str) -> Result<&ChoiceAnswer, Error> {
        self.expect(id, "choice", Answer::as_choice)
    }

    /// The Score answer under `id`.
    pub fn score(&self, id: &str) -> Result<&ScoreAnswer, Error> {
        self.expect(id, "score", Answer::as_score)
    }

    /// Every Noul answer, keyed by question id.
    pub fn nouls(&self) -> impl Iterator<Item = (&str, &NoulAnswer)> {
        self.answers
            .iter()
            .filter_map(|(id, a)| Some((id.as_str(), a.as_noul()?)))
    }

    /// Every Choice answer, keyed by question id.
    pub fn choices(&self) -> impl Iterator<Item = (&str, &ChoiceAnswer)> {
        self.answers
            .iter()
            .filter_map(|(id, a)| Some((id.as_str(), a.as_choice()?)))
    }

    /// Every Score answer, keyed by question id.
    pub fn scores(&self) -> impl Iterator<Item = (&str, &ScoreAnswer)> {
        self.answers
            .iter()
            .filter_map(|(id, a)| Some((id.as_str(), a.as_score()?)))
    }

    /// Answer kinds this SDK version did not recognise.
    ///
    /// Empty in normal operation. A non-empty result means the API returned
    /// something newer than this crate models.
    pub fn unknown(&self) -> impl Iterator<Item = (&str, &Answer)> {
        self.answers
            .iter()
            .filter(|(_, a)| matches!(a, Answer::Unknown { .. }))
            .map(|(id, a)| (id.as_str(), a))
    }

    fn expect<T>(
        &self,
        id: &str,
        expected: &'static str,
        project: impl Fn(&Answer) -> Option<&T>,
    ) -> Result<&T, Error> {
        let answer = self.answers.get(id).ok_or_else(|| Error::AnswerNotFound {
            id: id.to_owned(),
            available: self.answers.keys().cloned().collect(),
        })?;
        project(answer).ok_or_else(|| Error::AnswerKind {
            id: id.to_owned(),
            expected,
            found: answer.kind().to_owned(),
        })
    }
}
