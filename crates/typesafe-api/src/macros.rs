//! Declarative helpers for building a request by hand.

/// Builds a [`Questions`](crate::Questions) map from mixed question types.
///
/// An array literal cannot hold a `Noul` and a `Choice` at once; this converts
/// each value so they can sit side by side, and keeps the order you wrote.
///
/// ```
/// use typesafe_api::{Choice, Noul, questions};
///
/// let questions = questions! {
///     "is_urgent" => Noul::new("Does this convey urgency?"),
///     "tone" => Choice::new("What is the tone?").plain_options(["calm", "angry"]),
/// };
///
/// assert_eq!(questions.keys().collect::<Vec<_>>(), ["is_urgent", "tone"]);
/// ```
#[macro_export]
macro_rules! questions {
    ($($id:expr => $question:expr),* $(,)?) => {{
        let mut map = $crate::Questions::new();
        $(
            map.insert(
                ::std::string::String::from($id),
                $crate::Question::from($question),
            );
        )*
        map
    }};
}
