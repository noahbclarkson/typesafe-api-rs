//! The request body, and the map of questions that goes in it.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::question::Question;
use crate::state::State;

/// The question ids you choose, mapped to the questions themselves.
///
/// Ids are yours; the same ids come back on the answers. They are not sent to
/// the model and take no part in inference.
pub type Questions = IndexMap<String, Question>;

/// The body of a `POST /v1/systemone` call.
///
/// Built for you by the client. Reach for it directly when you want to inspect
/// or log exactly what will be sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The content to evaluate.
    pub state: State,
    /// The model that handles the request.
    pub model: String,
    /// One entry per question, keyed by an id you choose.
    pub questions: Questions,
    /// Top-level fields this crate version predates, merged over the body.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// Anything that can become a [`Questions`] map.
///
/// Implemented for every iterable of `(id, question)` pairs, which covers a
/// built map, a `Vec`, and an array literal. When the questions are of mixed
/// types, reach for [`questions!`](crate::questions) so each value is converted
/// for you.
pub trait IntoQuestions {
    /// Converts into the map form the wire expects.
    fn into_questions(self) -> Questions;
}

impl<I, K, Q> IntoQuestions for I
where
    I: IntoIterator<Item = (K, Q)>,
    K: Into<String>,
    Q: Into<Question>,
{
    fn into_questions(self) -> Questions {
        self.into_iter()
            .map(|(id, question)| (id.into(), question.into()))
            .collect()
    }
}
