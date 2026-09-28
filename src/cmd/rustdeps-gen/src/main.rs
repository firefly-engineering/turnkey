//! rustdeps-gen: Generate rust-deps.toml from Cargo.lock
//!
//! This tool parses Cargo.lock and generates a TOML file with Nix-compatible
//! hashes for use with turnkey's Rust deps cell (nix/buck2/languages.nix),
//! and each crate's package slice, as Cargo's feature resolver resolves it
//! (slices.rs).
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
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod requested;
mod slices;

use slices::{Dependency, Feature, Platform};

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

    /// A platform to resolve the package slices for, as
    /// <name>=<rust target triple> (e.g. macos-arm64=aarch64-apple-darwin);
    /// repeat for each platform
    #[arg(long = "platform", required = true)]
    platforms: Vec<Platform>,

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
    /// The platforms the package slices were resolved for. A feature or a
    /// dependency with no `platforms` applies on every one of them that
    /// builds the crate.
    platforms: Vec<String>,
    /// Every workspace member's Cargo.toml, relative to the project root: a
    /// change to any of them can change the slices, so tk sync counts them
    /// as sources of this file (the Rust sync rule's `target_sources`)
    manifests: BTreeSet<PathBuf>,
    /// Keyed "name@version", to hold several versions of a crate
    deps: BTreeMap<String, Crate>,
    /// The workspace members' registry dependency specs, from their
    /// Cargo.toml files: where feature unification starts
    requested: Vec<requested::Request>,
}

/// A crate with its Nix hash and its package slice
#[derive(Debug, PartialEq, Serialize)]
struct Crate {
    name: String,
    version: String,
    /// Empty when prefetching failed
    hash: String,
    /// The features Cargo enables, each with the platforms it applies on
    #[serde(skip_serializing_if = "Vec::is_empty")]
    features: Vec<Feature>,
    /// Its normal dependencies, as cargo resolves them. A crate no
    /// platform builds (Windows-only, say) has neither.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<Dependency>,
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

    // Each crate's package slice: Cargo resolves the workspace once per
    // platform, and its lock file graph names the dependencies
    let trees = args
        .platforms
        .iter()
        .map(|platform| Ok((platform.clone(), slices::cargo_tree(&cargo_toml, platform)?)))
        .collect::<Result<Vec<_>>>()?;
    let metadata = slices::cargo_metadata(&cargo_toml)?;
    let mut slices = slices::merge(&trees, &metadata)?;
    let project_root = std::env::current_dir().context("Failed to read the current directory")?;

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
        schema_version: 2,
        platforms: args.platforms.iter().map(|p| p.name.clone()).collect(),
        manifests: slices
            .manifests
            .iter()
            .map(|m| relative_to(m, &project_root))
            .collect(),
        deps: crates
            .into_iter()
            .map(|mut c| {
                let key = format!("{}@{}", c.name, c.version);
                if let Some(slice) = slices.crates.remove(&key) {
                    c.features = slice.features;
                    c.dependencies = slice.dependencies;
                }
                (key, c)
            })
            .collect(),
        requested,
    };
    args.output.write_text(&render(&doc)?)
}

/// rust-deps.toml as text: each crate's features and dependencies are
/// written as arrays of inline tables, one per line, rather than one
/// `[[deps."name@version".features]]` table per entry
fn render(doc: &RustDeps) -> Result<String> {
    let body = toml::to_string_pretty(doc).context("Failed to serialize to TOML")?;
    let mut toml: toml_edit::DocumentMut = body.parse().context("Failed to reparse the TOML")?;
    if let Some(deps) = toml.get_mut("deps").and_then(|d| d.as_table_mut()) {
        for (_, krate) in deps.iter_mut() {
            let Some(krate) = krate.as_table_mut() else {
                continue;
            };
            for key in ["features", "dependencies"] {
                let Some(tables) = krate.get(key).and_then(|v| v.as_array_of_tables()) else {
                    continue;
                };
                let mut array = tables.clone().into_array();
                for value in array.iter_mut() {
                    if let Some(table) = value.as_inline_table_mut() {
                        for (_, field) in table.iter_mut() {
                            if let Some(list) = field.as_array_mut() {
                                list.fmt();
                            }
                        }
                        table.fmt();
                    }
                    value.decor_mut().set_prefix("\n    ");
                }
                array.set_trailing("\n");
                array.set_trailing_comma(true);
                krate.insert(key, toml_edit::value(array));
            }
        }
    }
    Ok(format!(
        "{}{toml}",
        deps_gen_kit::header("rustdeps-gen", "Cargo.lock")
    ))
}

/// A path relative to the project root, or as it is when outside it
fn relative_to(path: &Path, root: &Path) -> PathBuf {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let path = canonical(path);
    path.strip_prefix(canonical(root))
        .map(Path::to_path_buf)
        .unwrap_or(path)
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
            features: Vec::new(),
            dependencies: Vec::new(),
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
                features: Vec::new(),
                dependencies: Vec::new(),
            }]
        );
        assert_eq!(fake.calls, vec![(url, true)]);
    }

    #[test]
    fn writes_deps_slices_and_requested() {
        let doc = RustDeps {
            schema_version: 2,
            platforms: vec!["linux-x86_64".into(), "macos-arm64".into()],
            manifests: [PathBuf::from("src/app/Cargo.toml")].into(),
            deps: BTreeMap::from([(
                "serde@1.0.228".to_string(),
                Crate {
                    name: "serde".into(),
                    version: "1.0.228".into(),
                    hash: "".into(),
                    features: vec![
                        Feature {
                            name: "default".into(),
                            platforms: None,
                        },
                        Feature {
                            name: "std".into(),
                            platforms: Some(["linux-x86_64".to_string()].into()),
                        },
                    ],
                    dependencies: vec![
                        Dependency {
                            package: "serde_core@1.0.228".into(),
                            rename: None,
                            platforms: None,
                        },
                        Dependency {
                            package: "serde_derive@1.0.228".into(),
                            rename: Some("derive".into()),
                            platforms: Some(["macos-arm64".to_string()].into()),
                        },
                    ],
                },
            )]),
            requested: vec![requested::Request {
                name: "serde".into(),
                version: Some("1.0".into()),
                default_features: false,
                features: ["derive".to_string()].into(),
            }],
        };
        let out = render(&doc).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();

        // One inline table per feature and dependency, one per line
        assert!(
            out.contains(
                r#"[deps."serde@1.0.228"]
name = "serde"
version = "1.0.228"
hash = ""
features = [
    { name = "default" },
    { name = "std", platforms = ["linux-x86_64"] },
]
dependencies = [
    { package = "serde_core@1.0.228" },
    { package = "serde_derive@1.0.228", rename = "derive", platforms = ["macos-arm64"] },
]
"#
            ),
            "{out}"
        );

        assert_eq!(parsed["schema_version"].as_integer(), Some(2));
        assert_eq!(parsed["platforms"][1].as_str(), Some("macos-arm64"));
        assert_eq!(parsed["manifests"][0].as_str(), Some("src/app/Cargo.toml"));
        let serde = &parsed["deps"]["serde@1.0.228"];
        assert_eq!(serde["hash"].as_str(), Some(""));
        assert_eq!(serde["features"][1]["name"].as_str(), Some("std"));
        let deps = serde["dependencies"].as_array().unwrap();
        assert_eq!(deps[0].get("rename"), None);
        assert_eq!(deps[0].get("platforms"), None);
        assert_eq!(deps[1]["package"].as_str(), Some("serde_derive@1.0.228"));
        assert_eq!(deps[1]["rename"].as_str(), Some("derive"));
        assert_eq!(deps[1]["platforms"][0].as_str(), Some("macos-arm64"));
        let req = &parsed["requested"][0];
        assert_eq!(req["name"].as_str(), Some("serde"));
        assert_eq!(req["default-features"].as_bool(), Some(false));
        assert_eq!(req["features"][0].as_str(), Some("derive"));
    }
}
