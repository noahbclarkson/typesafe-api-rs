//! A Rust client for the [TypeSafe](https://typesafe.ai) System One API.
//!
//! System One models answer typed questions about a state and return
//! structured values: a probability for a yes/no [`Noul`], a chosen option and
//! a distribution for a [`Choice`], a position on a rubric for a [`Score`].
//! Nothing is generated and nothing needs parsing.
//!
//! # Status
//!
//! The data model is complete and stable. The HTTP client lands next; see
//! `docs/DESIGN.md` for the surface it will expose.
//!
//! # The shape of a request
//!
//! ```
//! use system_one::{Choice, Noul, Score, questions};
//!
//! let questions = questions! {
//!     "is_urgent" => Noul::new("The message conveys urgency or time-sensitivity"),
//!     "department" => Choice::new("Which team should handle this?")
//!         .option("billing", "Payments, invoicing, refunds")
//!         .option("technical", "Bugs, outages, integrations")
//!         .option("sales", "Pricing, upgrades, new accounts"),
//!     "frustration" => Score::new("How frustrated is the customer?")
//!         .levels(["Calm", "Frustrated", "Very angry"]),
//! };
//!
//! assert_eq!(questions.len(), 3);
//! ```
//!
//! Every question is evaluated independently and in parallel against the same
//! state, so asking more of them costs tokens rather than time. Ask everything
//! the code might need in one call and ignore the answers it does not use.

#![cfg_attr(docsrs, feature(doc_cfg))]

mod answer;
mod entry;
mod error;
mod macros;
mod model;
mod question;
mod request;
mod response;
mod state;
pub mod validate;

pub use answer::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, Verdict};
pub use entry::{Entry, InvalidEntry};
pub use error::{ApiError, ApiErrorKind, Error};
pub use model::{JEV_LATEST, JEV_PREVIEW, ModelCard, ModelList};
pub use question::{Choice, Noul, NoulCriteria, Question, Score};
pub use request::{IntoQuestions, Questions, Request};
pub use response::{Response, Usage};
pub use state::{IntoState, InvalidState, State};
pub use validate::{Issue, Limits, ValidationError};

/// A [`Result`] whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
