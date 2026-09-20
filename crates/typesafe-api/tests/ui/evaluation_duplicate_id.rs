use typesafe_api::{Evaluation, NoulAnswer};

#[derive(Evaluation)]
struct Bad {
    /// One
    #[question(id = "same")]
    first: NoulAnswer,
    /// Two
    #[question(id = "same")]
    second: NoulAnswer,
}

fn main() {}
