//! rustdeps-gen: Generate rust-deps.toml from Cargo.lock
//!
//! This tool parses Cargo.lock and generates a TOML file with Nix-compatible
//! hashes for use with turnkey's Rust deps cell (nix/buck2/languages.nix).
//!
//! The checksums in Cargo.lock are for the .crate tarball, but Nix's fetchzip
//! computes hashes of the unpacked contents. Therefore, this tool prefetches
//! each crate to get the correct Nix hash.

use anyhow::{Context, Result};
use cargo_lock::{Lockfile, Package};
use clap::Parser;
use deps_gen_kit::{OutputArgs, PrefetchArgs, Prefetcher};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

mod requested;

/// Generate rust-deps.toml from Cargo.lock for Buck2/Nix integration
#[derive(Parser, Debug)]
#[command(name = "rustdeps-gen")]
#[command(about = "Generate rust-deps.toml from Cargo.lock")]
struct Args {
    /// Path to Cargo.lock file
    #[arg(long, default_value = "Cargo.lock")]
    cargo_lock: PathBuf,

    /// Path to the workspace's root Cargo.toml, whose members' dependency
    /// specs are recorded as [[requested]] (default: next to Cargo.lock)
    #[arg(long)]
    cargo_toml: Option<PathBuf>,

    #[command(flatten)]
    output: OutputArgs,

    #[command(flatten)]
    prefetch: PrefetchArgs,
}

/// rust-deps.toml
#[derive(Serialize)]
struct RustDeps {
    /// Schema version for forward compatibility
    schema_version: u32,
    /// Keyed "name@version", to hold several versions of a crate
    deps: BTreeMap<String, Crate>,
    /// The workspace members' registry dependency specs, from their
    /// Cargo.toml files: where feature unification starts
    requested: Vec<requested::Request>,
}

/// A crate with its Nix hash
#[derive(Debug, PartialEq, Serialize)]
struct Crate {
    name: String,
    version: String,
    /// Empty when prefetching failed
    hash: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Parse Cargo.lock
    let lockfile = Lockfile::load(&args.cargo_lock)
        .with_context(|| format!("Failed to load {}", args.cargo_lock.display()))?;

    // Feature unification in the cell starts from what the workspace asks for
    let cargo_toml = args
        .cargo_toml
        .clone()
        .unwrap_or_else(|| args.cargo_lock.with_file_name("Cargo.toml"));
    let requested = requested::workspace_requests(&cargo_toml)?;

    // Filter to crates.io packages only
    let crates_io_packages: Vec<&Package> = lockfile
        .packages
        .iter()
        .filter(|p| is_crates_io(p))
        .collect();

    eprintln!(
        "Found {} crates from crates.io in Cargo.lock",
        crates_io_packages.len()
    );

    let mut prefetcher = args.prefetch.prefetcher();
    let crates = collect_crates(
        &crates_io_packages,
        prefetcher.as_mut().map(|p| p as &mut dyn Prefetcher),
    );

    let doc = RustDeps {
        schema_version: 1,
        deps: crates
            .into_iter()
            .map(|c| (format!("{}@{}", c.name, c.version), c))
            .collect(),
        requested,
    };
    args.output.write("rustdeps-gen", "Cargo.lock", &doc)
}

/// Each crate with its Nix hash: prefetched, or with no prefetcher, the
/// Cargo.lock checksum
fn collect_crates(
    packages: &[&Package],
    mut prefetcher: Option<&mut dyn Prefetcher>,
) -> Vec<Crate> {
    if prefetcher.is_none() {
        eprintln!("WARNING: --no-prefetch produces incorrect hashes for fetchzip");
        eprintln!("The Cargo.lock checksum is for the tarball, not unpacked contents");
    } else {
        eprintln!("Prefetching {} crates from crates.io...", packages.len());
    }

    let mut crates = Vec::new();
    for (i, pkg) in packages.iter().enumerate() {
        let name = pkg.name.as_str();
        let version = pkg.version.to_string();

        let hash = match prefetcher.as_deref_mut() {
            None => pkg
                .checksum
                .as_ref()
                .and_then(|cs| convert_checksum_to_sri(&cs.to_string())),
            Some(prefetcher) => {
                eprintln!(
                    "[{}/{}] prefetching {}@{}...",
                    i + 1,
                    packages.len(),
                    name,
                    version
                );
                match prefetcher.prefetch(&crate_url(name, &version), true) {
                    Ok(hash) => Some(hash),
                    Err(e) => {
                        eprintln!("    warning: failed to prefetch: {}", e);
                        None
                    }
                }
            }
        };

        crates.push(Crate {
            name: name.to_string(),
            version,
            hash: hash.unwrap_or_default(),
        });
    }
    crates
}

/// Check if a package is from crates.io
fn is_crates_io(pkg: &Package) -> bool {
    pkg.source
        .as_ref()
        .map(|s| s.is_default_registry())
        .unwrap_or(false)
}

/// Convert hex checksum to SRI format (for --no-prefetch fallback)
fn convert_checksum_to_sri(hex: &str) -> Option<String> {
    if hex.len() != 64 {
        return None;
    }

    let bytes: Result<Vec<u8>, _> = (0..32)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
        .collect();

    bytes.ok().map(|b| {
        use base64::{Engine, engine::general_purpose::STANDARD};
        format!("sha256-{}", STANDARD.encode(&b))
    })
}

/// The crates.io download URL of a crate, the archive the Rust deps cell
/// fetches
fn crate_url(name: &str, version: &str) -> String {
    format!(
        "https://crates.io/api/v1/crates/{}/{}/download",
        name, version
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use deps_gen_kit::MemoryPrefetcher;

    const LOCK: &str = r#"
version = 4

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"

[[package]]
name = "local"
version = "0.1.0"
"#;

    #[test]
    fn prefetches_crates_io_packages_unpacked() {
        let lock: Lockfile = LOCK.parse().unwrap();
        let packages: Vec<&Package> = lock.packages.iter().filter(|p| is_crates_io(p)).collect();
        let url = crate_url("serde", "1.0.228");
        let mut fake = MemoryPrefetcher::default().with(&url, true, "sha256-serde");

        let crates = collect_crates(&packages, Some(&mut fake));

        assert_eq!(
            crates,
            vec![Crate {
                name: "serde".into(),
                version: "1.0.228".into(),
                hash: "sha256-serde".into(),
            }]
        );
        assert_eq!(fake.calls, vec![(url, true)]);
    }

    #[test]
    fn writes_deps_and_requested() {
        let doc = RustDeps {
            schema_version: 1,
            deps: BTreeMap::from([(
                "serde@1.0.228".to_string(),
                Crate {
                    name: "serde".into(),
                    version: "1.0.228".into(),
                    hash: "".into(),
                },
            )]),
            requested: vec![requested::Request {
                name: "serde".into(),
                version: Some("1.0".into()),
                default_features: false,
                features: ["derive".to_string()].into(),
            }],
        };
        let out = deps_gen_kit::render("rustdeps-gen", "Cargo.lock", &doc).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();

        assert_eq!(parsed["schema_version"].as_integer(), Some(1));
        assert_eq!(parsed["deps"]["serde@1.0.228"]["hash"].as_str(), Some(""));
        let req = &parsed["requested"][0];
        assert_eq!(req["name"].as_str(), Some("serde"));
        assert_eq!(req["default-features"].as_bool(), Some(false));
        assert_eq!(req["features"][0].as_str(), Some("derive"));
    }
}
