use typesafe_api::Options;

#[derive(Options)]
enum Bad {
    /// One
    Billing,
    /// Two
    #[options(name = "billing")]
    Payments,
}

fn main() {}
