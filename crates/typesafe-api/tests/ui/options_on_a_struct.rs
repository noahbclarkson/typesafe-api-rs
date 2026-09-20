use typesafe_api::Options;

#[derive(Options)]
struct Bad {
    field: u32,
}

fn main() {}
