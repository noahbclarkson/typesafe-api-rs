//! A Rust client for the [TypeSafe](https://typesafe.ai) System One API.
//!
//! System One models answer typed questions about a state and return
//! structured values: a probability for a yes/no [`Noul`], a chosen option and
//! a distribution for a [`Choice`], a position on a rubric for a [`Score`].
//! Nothing is generated and nothing needs parsing.
//!
//! Rust already speaks in enums and structs, so this crate closes the loop:
//! describe a judgement once as a type, and that type is both the request you
//! send and the answer you get back.
//!
//! # A first call
//!
//! ```no_run
//! use typesafe_api::{Choice, Client, Noul, Score, questions};
//!
//! # async fn run() -> Result<(), typesafe_api::Error> {
//! let client = Client::from_env()?;
//!
//! let answers = client
//!     .evaluate(
//!         "Hi, my Stripe integration has been failing for 3 days. I am losing sales.",
//!         questions! {
//!             "is_urgent" => Noul::new("The message conveys urgency or time-sensitivity"),
//!             "department" => Choice::new("Which team should handle this?")
//!                 .option("billing", "Payment or subscription issues")
//!                 .option("technical", "Bugs or integration problems"),
//!             "frustration" => Score::new("How frustrated the customer appears")
//!                 .levels(["Calm", "Frustrated but civil", "Very angry"]),
//!         },
//!     )
//!     .await?;
//!
//! if answers.noul("is_urgent")?.is_yes(0.9) {
//!     // page someone
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Every question in one request is evaluated independently and in parallel
//! against the same state, so asking more of them costs tokens rather than
//! time. Ask everything the code might need and ignore the answers it does not
//! use.
//!
//! # Layers
//!
//! Each layer is built from the public layer below it, and nothing is hidden:
//! [`Client::request`] hands back the exact [`Request`], and
//! [`Client::send_raw`] returns the response body untouched.
//!
//! 1. **Named questions and answers**, as above. Closest to the HTTP API, and
//!    the right choice when questions are assembled at runtime.
//! 2. **Options and levels as enums**, with `#[derive(Options)]` and
//!    `#[derive(Levels)]`. The descriptions come from doc comments, and the
//!    answer is parsed back into a variant.
//! 3. **A whole evaluation as one struct**, with `#[derive(Evaluation)]`. The
//!    struct declares the questions and receives the answers.
//!
//! Layers 2 and 3 need the `derive` feature for the macros. The traits and
//! answer wrappers they target are always available, so [`Options`] and
//! [`Levels`] can also be written by hand for descriptions a doc comment
//! cannot express.
//!
//! ```
//! # #[cfg(feature = "derive")] {
//! use typesafe_api::{ChoiceOf, Evaluation, Levels, NoulAnswer, Options, ScoreOf};
//!
//! #[derive(Options)]
//! enum Department {
//!     /// Payment or subscription issues
//!     Billing,
//!     /// Bugs or integration problems
//!     Technical,
//! }
//!
//! #[derive(Levels)]
//! enum Frustration {
//!     /// Calm, just stating facts
//!     Calm,
//!     /// Frustrated but civil
//!     Frustrated,
//!     /// Very angry, strong language or threatening to leave
//!     VeryAngry,
//! }
//!
//! #[derive(Evaluation)]
//! struct Triage {
//!     /// The message conveys urgency or time-sensitivity
//!     is_urgent: NoulAnswer,
//!     /// Which team should handle this?
//!     department: ChoiceOf<Department>,
//!     /// How frustrated the customer appears
//!     frustration: ScoreOf<Frustration>,
//! }
//!
//! # use typesafe_api::Evaluation as _;
//! assert_eq!(Triage::questions().len(), 3);
//! # }
//! ```
//!
//! # Choosing a question type
//!
//! | The question | Primitive | What comes back |
//! | --- | --- | --- |
//! | Is this true? | [`Noul`] | one probability, 0 to 1 |
//! | Which one of these? | [`Choice`] | the pick, every probability, confidence |
//! | Where on this scale? | [`Score`] | a weighted position, every probability, confidence |
//!
//! A Noul carries no separate confidence: with two outcomes, the single value
//! already describes the distribution. See [`Gate`] and [`Composite`] for
//! turning those numbers into decisions.
//!
//! # Features
//!
//! | Feature | Default | What it adds |
//! | --- | --- | --- |
//! | `rustls-tls` | yes | TLS via rustls |
//! | `native-tls` | no | TLS via the platform stack |
//! | `blocking` | no | [`blocking::Client`], for code without a runtime |
//! | `derive` | no | the `Evaluation`, `Options`, and `Levels` derive macros |
//! | `stream` | no | [`Client::evaluate_many`], bounded fan-out |
//! | `preserve-order` | no | authored key order inside every nested object |

#![cfg_attr(docsrs, feature(doc_cfg))]

mod answer;
mod client;
mod config;
mod entry;
mod error;
mod macros;
mod model;
mod patterns;
mod question;
mod request;
mod response;
mod retry;
mod state;
mod transport;
mod typed;
pub mod validate;

#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
pub mod blocking;

#[cfg(feature = "stream")]
#[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
mod stream;

pub use answer::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, Verdict};
pub use client::{Client, Evaluate, EvaluateAs, Models};
pub use config::{
    API_KEY_ENV, BASE_URL_ENV, ClientBuilder, DEFAULT_BASE_URL, DEFAULT_MODEL_ENV, DEFAULT_TIMEOUT,
};
pub use entry::{Entry, InvalidEntry};
pub use error::{ApiError, ApiErrorKind, Error};
pub use model::{JEV_LATEST, JEV_PREVIEW, ModelCard, ModelList};
pub use patterns::{Composite, Gate, gate};
pub use question::{Choice, Noul, NoulCriteria, Question, Score};
pub use request::{IntoQuestions, Questions, Request};
pub use response::{Response, Usage};
pub use retry::RetryPolicy;
pub use state::{IntoState, InvalidState, State};
pub use validate::{Issue, Limits, ValidationError};

#[cfg(feature = "stream")]
#[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
pub use stream::{DEFAULT_CONCURRENCY, FanOut};

pub use typed::{ChoiceOf, Evaluation, Levels, Options, ScoreOf, UnknownOption};

/// Derives [`Options`] on a fieldless enum of Choice options.
///
/// Doc comments become the descriptions the model sees. Variant names become
/// the option names, converted to `snake_case` unless the container says
/// otherwise.
///
/// ```
/// use typesafe_api::Options;
///
/// #[derive(Options)]
/// #[options(rename_all = "kebab-case")]
/// enum ReturnReason {
///     /// The item does not fit
///     WrongSize,
///     /// A different product was delivered
///     WrongItem,
///     #[options(name = "other", describe = "A reason that fits none of the above")]
///     Other,
/// }
///
/// # use typesafe_api::Options as _;
/// assert_eq!(ReturnReason::variants(), ["wrong-size", "wrong-item", "other"]);
/// ```
#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use typesafe_api_macros::Options;

/// Derives [`Levels`] on a fieldless enum of Score levels, lowest first.
///
/// The declaration order is the level numbering, starting at 0. Doc comments
/// become the level descriptions.
///
/// ```
/// use typesafe_api::Levels;
///
/// #[derive(Levels)]
/// enum Severity {
///     /// Cosmetic; no impact to functionality
///     Cosmetic,
///     /// Broken or degraded feature, but workaround exists
///     Degraded,
///     /// Blocking issue; no workaround exists
///     Blocking,
/// }
///
/// # use typesafe_api::Levels as _;
/// assert_eq!(Severity::count(), 3);
/// assert_eq!(Severity::Blocking.level(), 2);
/// ```
#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use typesafe_api_macros::Levels;

/// Derives [`Evaluation`] on a struct of answer fields.
///
/// Each field is one question. The field name is the question id and the doc
/// comment is the instructions, unless `#[question(...)]` says otherwise.
/// Supported field types are [`NoulAnswer`], [`ChoiceOf<T>`], [`ScoreOf<T>`],
/// and `Option<_>` of any of them for an answer that may be absent.
///
/// ```
/// use typesafe_api::{Evaluation, NoulAnswer};
///
/// #[derive(Evaluation)]
/// struct Checks {
///     /// Does the message ask for a refund?
///     refund_requested: NoulAnswer,
///
///     #[question(
///         id = "pii",
///         instructions = "Does the message contain personal data?",
///         yes = "Names, addresses, card numbers, or identifiers",
///         no = "No personal data",
///     )]
///     contains_pii: NoulAnswer,
/// }
///
/// # use typesafe_api::Evaluation as _;
/// assert_eq!(Checks::questions().keys().collect::<Vec<_>>(), ["refund_requested", "pii"]);
/// ```
#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use typesafe_api_macros::Evaluation;

/// Re-exported so hand-written [`Options`] impls, and the code the derives
/// generate, do not need `indexmap` as a direct dependency.
pub use indexmap;

/// A [`Result`] whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
