use typesafe_api::{Evaluation, NoulAnswer};

#[derive(Evaluation)]
struct Bad {
    is_urgent: NoulAnswer,
}

fn main() {}
