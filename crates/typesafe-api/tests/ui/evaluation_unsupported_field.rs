use typesafe_api::Evaluation;

#[derive(Evaluation)]
struct Bad {
    /// A question
    answer: String,
}

fn main() {}
