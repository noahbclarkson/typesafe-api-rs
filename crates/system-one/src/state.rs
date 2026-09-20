//! [`State`]: the content a request evaluates.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entry::json_kind;

/// The content to evaluate: text, a JSON object, or a JSON array.
///
/// You rarely name this type. Every client method takes `impl IntoState`, so
/// your own `#[derive(Serialize)]` structs go straight through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum State {
    /// A message, article, or passage.
    Text(String),
    /// Named fields, related records, or application state.
    Object(IndexMap<String, Value>),
    /// A sequence of messages or records.
    Array(Vec<Value>),
}

/// Why a value could not be used as a [`State`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidState {
    /// The value serialized to JSON, but not to a string, object, or array.
    #[error("state must be a string, object, or array; found {found}")]
    Shape {
        /// The JSON kind that was found instead.
        found: &'static str,
    },
    /// The value could not be serialized to JSON at all.
    #[error("state could not be serialized to JSON: {0}")]
    Serialize(String),
}

impl State {
    /// Wraps a string.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// The JSON kind of this state, for diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "string",
            Self::Object(_) => "object",
            Self::Array(_) => "array",
        }
    }
}

/// Anything that can become a [`State`].
///
/// Blanket-implemented for every [`Serialize`] type, so a request can take a
/// domain struct without a conversion step.
pub trait IntoState {
    /// Serializes and validates the value.
    fn into_state(self) -> Result<State, InvalidState>;
}

impl<T: Serialize> IntoState for T {
    fn into_state(self) -> Result<State, InvalidState> {
        let value =
            serde_json::to_value(self).map_err(|err| InvalidState::Serialize(err.to_string()))?;
        match value {
            Value::String(text) => Ok(State::Text(text)),
            Value::Object(map) => Ok(State::Object(map.into_iter().collect())),
            Value::Array(items) => Ok(State::Array(items)),
            other => Err(InvalidState::Shape {
                found: json_kind(&other),
            }),
        }
    }
}
