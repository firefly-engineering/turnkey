//! pydeps-cell writes one vendored Python distribution's rules.star, with
//! its dependencies evaluated per platform.
//!
//! It runs inside the distribution's own derivation
//! (nix/lib/deps-cell/adapters/python.nix), and reads nothing of any other
//! distribution: its package slice, the platforms and the Python toolchain's
//! version (ADR 0010). So a distribution's store path changes only when its
//! own inputs do (ADR 0004). It prints the rules.star, and writes its target
//! name for the cell index.
//!
//! It was ported from Go (#212). The library it writes holds the whole
//! unpacked wheel, its non-Python files as resources (ADR 0013).

mod cell;
mod decode;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "pydeps-cell",
    about = "Write a vendored Python distribution's rules.star from its package slice"
)]
struct Args {
    /// The distribution's key in python-deps.toml, and its target's name
    #[arg(long)]
    name: String,

    /// The distribution's package slice, as JSON: its dependencies, the
    /// extras some package or workspace member asks it for and those
    /// extras' dependencies, each naming only distributions the cell holds
    #[arg(long)]
    slice: PathBuf,

    /// The platforms, as nix/buck2/platforms.nix's conditions in JSON
    #[arg(long)]
    platforms: String,

    /// The Python toolchain's full version the markers are evaluated for,
    /// e.g. "3.13.12"
    #[arg(long)]
    python_version: String,

    /// Where to write the target name
    #[arg(long)]
    targets_out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut cfg: cell::Config =
        serde_json::from_str(&deps_gen_kit::gojson::text(args.platforms.as_bytes()))
            .context("--platforms")?;
    cfg.python_version = args.python_version;
    let slice: cell::Package = serde_json::from_str(
        &std::fs::read_to_string(&args.slice).context("Failed to read the slice")?,
    )
    .context("Failed to parse the slice")?;

    let rules = cell::render(&args.name, &slice, &cfg)?;
    std::fs::write(&args.targets_out, format!("{}\n", args.name))
        .with_context(|| format!("write {}", args.targets_out.display()))?;
    print!("{rules}");
    Ok(())
}
