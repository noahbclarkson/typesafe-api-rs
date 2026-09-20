//! Model names and the `GET /v1/models` payload.

use serde::{Deserialize, Serialize};

/// The most recent stable release. The default when no model is set.
pub const JEV_LATEST: &str = "jev-latest";

/// The most recent release, preview builds included.
pub const JEV_PREVIEW: &str = "jev-preview";

/// One model or alias the account may send in `model`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCard {
    /// The id or alias, as accepted by the `model` field.
    pub name: String,
    /// What the model is for.
    pub description: String,
    /// Release date, formatted as `YYYY-MM-DD`.
    pub release_date: String,
}

/// The body of `GET /v1/models`.
///
/// Aliases are listed; versioned ids such as `jev-1.13.0` are accepted by the
/// `model` field whether or not they appear here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelList {
    /// One entry per model or alias.
    pub models: Vec<ModelCard>,
}
