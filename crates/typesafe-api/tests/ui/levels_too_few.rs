use typesafe_api::Levels;

#[derive(Levels)]
enum Bad {
    /// The only level
    Only,
}

fn main() {}
