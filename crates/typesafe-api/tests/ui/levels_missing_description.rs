use typesafe_api::Levels;

#[derive(Levels)]
enum Bad {
    /// Low
    Low,
    High,
}

fn main() {}
