//! Lists the models the account may use.
//!
//! `cargo run --example list_models`
//!
//! Aliases such as `jev-latest` are listed. Versioned ids such as `jev-1.13.0`
//! are accepted by the `model` field whether or not they appear here, so pin a
//! version once thresholds have been tuned against it.

use typesafe_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(client) = Client::from_env() else {
        eprintln!("set TYPESAFE_API_KEY to run this example");
        return Ok(());
    };

    println!("default model: {}\n", client.model());

    for model in client.models().await? {
        println!(
            "{:<14} {:<12} {}",
            model.name, model.release_date, model.description
        );
    }

    Ok(())
}
