//! deps-extract: Extract dependencies from source files using tree-sitter
//!
//! This tool parses source files using tree-sitter grammars to extract
//! import statements and output them in the extraction protocol JSON format.
//!
//! Usage:
//!     deps-extract --lang python <dir>
//!     deps-extract --lang rust <dir>
//!     deps-extract --lang typescript <dir>
//!     deps-extract --lang solidity <dir>

mod extraction;
mod languages;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "deps-extract")]
#[command(about = "Extract dependencies from source files using tree-sitter")]
struct Args {
    /// Language to parse
    #[arg(short, long)]
    lang: String,

    /// Directory to analyze
    #[arg(default_value = ".")]
    dir: PathBuf,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Directory patterns to exclude (comma-separated)
    #[arg(long, default_value = "")]
    exclude: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let exclude_patterns: Vec<&str> = if args.exclude.is_empty() {
        Vec::new()
    } else {
        args.exclude.split(',').map(|s| s.trim()).collect()
    };

    let result = match args.lang.as_str() {
        "python" => languages::python::extract(&args.dir, &exclude_patterns)?,
        "rust" => languages::rust::extract(&args.dir, &exclude_patterns)?,
        "typescript" | "ts" | "javascript" | "js" => {
            languages::typescript::extract(&args.dir, &exclude_patterns)?
        }
        "solidity" | "sol" => languages::solidity::extract(&args.dir, &exclude_patterns)?,
        _ => {
            eprintln!("Unsupported language: {}", args.lang);
            eprintln!("Supported: python, rust, typescript, solidity");
            std::process::exit(1);
        }
    };

    let json = serde_json::to_string_pretty(&result)?;

    match args.output {
        Some(path) => std::fs::write(path, json)?,
        None => println!("{}", json),
    }

    Ok(())
}
