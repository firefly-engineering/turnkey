// Example demonstrating external Rust crate usage in Buck2
//
// This example would depend on the `clap` crate for CLI parsing.
// To make this work with Buck2, you need:
// 1. A third-party directory with Cargo.toml specifying dependencies
// 2. reindeer to generate BUCK files from Cargo dependencies
//
// See: https://github.com/facebookincubator/reindeer

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "hello")]
#[command(about = "A simple example with external dependencies")]
struct Args {
    /// Name to greet
    #[arg(short, long, default_value = "World")]
    name: String,
}

fn main() {
    let args = Args::parse();
    println!("Hello, {}!", args.name);
}
