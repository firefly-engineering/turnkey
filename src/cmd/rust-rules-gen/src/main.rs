//! rust-rules-gen: generate one vendored Rust crate's rules.star from its
//! package slice (ADR 0006), inside the crate's own derivation.
//!
//! It reads only the crate: its directory, its slice from rust-deps.toml, its
//! fixup, the platforms and the platform building it. So a crate's store path
//! changes only when its own inputs do (ADR 0004). It prints the rules.star,
//! and writes its target names, one per line, for the cell index.

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::PathBuf;

mod fixup;
mod manifest;
mod platforms;
mod starlark;

use fixup::{Fixup, Split};
use manifest::Manifest;
use platforms::{Platform, Platforms};

#[derive(Parser, Debug)]
#[command(
    name = "rust-rules-gen",
    about = "Generate a vendored crate's rules.star from its package slice"
)]
struct Args {
    /// The crate's directory
    #[arg(long)]
    crate_dir: PathBuf,

    /// The crate's package slice (its rust-deps.toml features and
    /// dependencies), as JSON
    #[arg(long)]
    slice: PathBuf,

    /// The crate's fixup (its gen part, nix/lib/fixups), as JSON
    #[arg(long)]
    fixup: PathBuf,

    /// The platforms, as nix/buck2/platforms.nix's conditions in JSON
    #[arg(long)]
    platforms: String,

    /// The platform building the crate, as JSON {"os", "cpu"}
    #[arg(long)]
    host: String,

    /// Where to write the target names
    #[arg(long)]
    targets_out: PathBuf,

    /// Fail with this message if the crate has a build script: no fixup
    /// accounts for it, and Buck2 never runs build.rs
    #[arg(long)]
    unaccounted: Option<String>,
}

/// A crate's package slice, as rust-deps.toml records it (rustdeps-gen)
#[derive(Debug, Default, Deserialize)]
struct Slice {
    #[serde(default)]
    features: Vec<Feature>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Feature {
    name: String,
    platforms: Option<BTreeSet<String>>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    package: String,
    rename: Option<String>,
    platforms: Option<BTreeSet<String>>,
}

/// Whether a slice entry applies on a platform: it names its platforms, or
/// applies on every one that builds the crate
fn applies(platforms: &Option<BTreeSet<String>>, platform: &Platform) -> bool {
    platforms
        .as_ref()
        .is_none_or(|on| on.contains(&platform.name()))
}

/// A vendored package's label: its versioned package in the cell, and the
/// target named after the crate
fn label(package: &str) -> String {
    let name = package.rsplit_once('@').map_or(package, |(name, _)| name);
    format!("rustdeps//vendor/{package}:{name}")
}

fn split<T: Clone + PartialEq + Ord>(
    platforms: &Platforms,
    items: impl Fn(&Platform) -> Vec<T>,
) -> Split<Vec<T>> {
    let (common, by_platform) = platforms.split(items);
    Split {
        common,
        by_platform,
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let manifest = Manifest::read(&args.crate_dir)?;
    if let Some(message) = &args.unaccounted
        && manifest.has_build_script()
    {
        bail!("{message}");
    }

    let platforms = Platforms::from_json(&args.platforms).context("--platforms")?;
    let host: Platform = serde_json::from_str(&args.host).context("--host")?;
    let slice: Slice = serde_json::from_str(
        &std::fs::read_to_string(&args.slice).context("Failed to read the slice")?,
    )
    .context("Failed to parse the slice")?;
    let fixup: Fixup = serde_json::from_str(
        &std::fs::read_to_string(&args.fixup).context("Failed to read the fixup")?,
    )
    .context("Failed to parse the fixup")?;

    let name = manifest.crate_name();

    // "default" only names other features, which the slice holds too
    let features = split(&platforms, |p| {
        let mut on: Vec<String> = slice
            .features
            .iter()
            .filter(|f| f.name != "default" && applies(&f.platforms, p))
            .map(|f| f.name.clone())
            .collect();
        on.sort();
        on
    });
    let deps = split(&platforms, |p| {
        let mut on: Vec<String> = slice
            .dependencies
            .iter()
            .filter(|d| d.rename.is_none() && applies(&d.platforms, p))
            .map(|d| label(&d.package))
            .collect();
        on.sort();
        on
    });
    let named_deps = split(&platforms, |p| {
        let mut on: Vec<(String, String)> = slice
            .dependencies
            .iter()
            .filter(|d| applies(&d.platforms, p))
            .filter_map(|d| Some((d.rename.clone()?, label(&d.package))))
            .collect();
        on.sort();
        on
    });

    // Cargo's variables, then the fixup's; OUT_DIR when its build script
    // generated output
    let fixup_env = fixup.env(&platforms);
    let mut env = manifest.cargo_env(&name);
    env.extend(fixup_env.common);
    if fixup.out_dir {
        env.insert("OUT_DIR".to_string(), "out_dir".to_string());
    }

    // Cap lints for vendored crates, as Cargo does for every non-local
    // dependency
    let mut rustc_flags = fixup.rustc_flags(&platforms);
    rustc_flags
        .common
        .splice(0..0, ["--cap-lints".to_string(), "allow".to_string()]);

    let krate = starlark::Crate {
        edition: manifest.edition(),
        crate_root: manifest.lib_path(),
        proc_macro: manifest.is_proc_macro(),
        features,
        deps,
        named_deps,
        env: Split {
            common: env,
            by_platform: fixup_env.by_platform,
        },
        rustc_flags,
        native_libraries: fixup.native_libraries.clone(),
        host_key: platforms.combined_key(&host),
        name,
    };
    std::fs::write(
        &args.targets_out,
        krate
            .target_names()
            .iter()
            .map(|t| format!("{t}\n"))
            .collect::<String>(),
    )
    .with_context(|| format!("Failed to write {}", args.targets_out.display()))?;
    print!("{}", krate.render());
    Ok(())
}
