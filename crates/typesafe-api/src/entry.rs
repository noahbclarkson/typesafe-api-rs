//! [`Entry`]: the JSON shapes accepted by `instructions` and `criteria`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Text, a JSON object, or a JSON array.
///
/// The API accepts these three shapes anywhere a description is expected:
/// `instructions`, Choice option descriptions, Score level descriptions, and
/// the `true` / `false` arms of Noul criteria. A bare number or boolean is not
/// a valid description, so [`Entry`] cannot hold one.
///
/// Absence is modelled with `Option<Entry>` rather than a `Null` variant, so a
/// missing description and an explicitly null one are the same value.
///
/// ```
/// use typesafe_api::Entry;
///
/// let plain: Entry = "Does this convey urgency?".into();
/// let structured = Entry::fields([
///     ("question", Entry::text("Is this the same person as `candidate`?")),
///     ("candidate", Entry::fields([("name", Entry::text("John Smith"))])),
/// ]);
/// assert!(matches!(structured, Entry::Object(_)));
/// # let _ = plain;
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    /// A plain string, the right choice for most descriptions.
    Text(String),
    /// A JSON object whose field names label the values for the model.
    Object(IndexMap<String, Value>),
    /// A JSON array, useful for a list of things to check or compare.
    Array(Vec<Value>),
}

/// Why a value could not be used as an [`Entry`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidEntry {
    /// The value serialized to JSON, but not to a string, object, or array.
    #[error("expected a string, object, or array; found {found}")]
    Shape {
        /// The JSON kind that was found instead.
        found: &'static str,
    },
    /// The value could not be serialized to JSON at all.
    #[error("value could not be serialized to JSON: {0}")]
    Serialize(String),
}

impl Entry {
    /// Wraps a string.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Serializes any [`Serialize`] value and checks that it is a valid shape.
    ///
    /// Use this to point a question at a record from your own code.
    ///
    /// ```
    /// use typesafe_api::Entry;
    /// # #[derive(serde::Serialize)]
    /// # struct Candidate { name: &'static str }
    /// let entry = Entry::json(Candidate { name: "John Smith" })?;
    /// assert!(matches!(entry, Entry::Object(_)));
    /// # Ok::<_, typesafe_api::InvalidEntry>(())
    /// ```
    pub fn json(value: impl Serialize) -> Result<Self, InvalidEntry> {
        let value =
            serde_json::to_value(value).map_err(|err| InvalidEntry::Serialize(err.to_string()))?;
        Self::try_from(value)
    }

    /// Builds an object entry from labelled parts, preserving the order given.
    pub fn fields<K, V, I>(fields: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<Self>,
    {
        Self::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), Value::from(value.into())))
                .collect(),
        )
    }

    /// Builds an array entry.
    pub fn list<I, V>(items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Self>,
    {
        Self::Array(
            items
                .into_iter()
                .map(|item| Value::from(item.into()))
                .collect(),
        )
    }

    /// The JSON kind of this entry, for diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "string",
            Self::Object(_) => "object",
            Self::Array(_) => "array",
        }
    }
}

/// Names the JSON kind of a value, for diagnostics.
pub(crate) fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

impl TryFrom<Value> for Entry {
    type Error = InvalidEntry;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(text) => Ok(Self::Text(text)),
            Value::Object(map) => Ok(Self::Object(map.into_iter().collect())),
            Value::Array(items) => Ok(Self::Array(items)),
            other => Err(InvalidEntry::Shape {
                found: json_kind(&other),
            }),
        }
    }
}

impl From<Entry> for Value {
    fn from(entry: Entry) -> Self {
        match entry {
            Entry::Text(text) => Self::String(text),
            Entry::Object(map) => Self::Object(map.into_iter().collect()),
            Entry::Array(items) => Self::Array(items),
        }
    }
}

impl From<&str> for Entry {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Entry {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&String> for Entry {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}

impl From<std::borrow::Cow<'_, str>> for Entry {
    fn from(value: std::borrow::Cow<'_, str>) -> Self {
        Self::Text(value.into_owned())
    }
}
