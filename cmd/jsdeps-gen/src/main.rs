//! jsdeps-gen: Generate js-deps.toml from pnpm-lock.yaml
//!
//! This tool parses pnpm-lock.yaml and generates a TOML file with package
//! information for use with turnkey's js-deps-cell.nix.
//!
//! pnpm lockfiles contain integrity hashes (SHA512) which we convert to
//! the format expected by Nix's fetchurl with SRI hashes.

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Generate js-deps.toml from pnpm-lock.yaml for Buck2/Nix integration
#[derive(Parser, Debug)]
#[command(name = "jsdeps-gen")]
#[command(about = "Generate js-deps.toml from pnpm-lock.yaml")]
struct Args {
    /// Path to pnpm-lock.yaml file
    #[arg(long, default_value = "pnpm-lock.yaml")]
    lock: PathBuf,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Include dev dependencies
    #[arg(long, default_value = "false")]
    include_dev: bool,
}

/// Represents a package in the output TOML
#[derive(Debug, Serialize)]
struct OutputPackage {
    name: String,
    version: String,
    /// NPM tarball URL
    url: String,
    /// SRI hash (sha512-...)
    integrity: String,
    /// Dependencies of this package
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
}

/// pnpm lockfile structure (v9+)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmLockfile {
    lockfile_version: String,
    #[serde(default)]
    packages: BTreeMap<String, PnpmPackage>,
    #[serde(default)]
    snapshots: BTreeMap<String, PnpmSnapshot>,
}

/// Package entry in pnpm-lock.yaml
#[derive(Debug, Deserialize)]
struct PnpmPackage {
    resolution: Option<PnpmResolution>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    dev: bool,
}

/// Snapshot entry in pnpm-lock.yaml (v9+)
#[derive(Debug, Deserialize)]
struct PnpmSnapshot {
    #[serde(default)]
    dependencies: BTreeMap<String, PnpmSnapshotDep>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, PnpmSnapshotDep>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PnpmSnapshotDep {
    Simple(String),
    Complex { version: String },
}

/// Resolution info for a package
#[derive(Debug, Deserialize)]
struct PnpmResolution {
    integrity: Option<String>,
    tarball: Option<String>,
}

/// Output TOML structure
#[derive(Debug, Serialize)]
struct OutputToml {
    /// Generator metadata
    meta: OutputMeta,
    /// Packages map
    #[serde(rename = "package")]
    packages: Vec<OutputPackage>,
}

#[derive(Debug, Serialize)]
struct OutputMeta {
    generator: String,
    lockfile_version: String,
}

/// Parse package specifier like "@types/node@22.10.10" or "lodash@4.17.21"
fn parse_package_spec(spec: &str) -> Option<(String, String)> {
    // Handle scoped packages: @scope/name@version
    if spec.starts_with('@') {
        // Find the second @ which separates name from version
        let parts: Vec<&str> = spec.splitn(3, '@').collect();
        if parts.len() >= 3 {
            let name = format!("@{}", parts[1]);
            let version = parts[2].split('(').next().unwrap_or(parts[2]);
            return Some((name.to_string(), version.to_string()));
        }
    } else {
        // Non-scoped package: name@version
        if let Some(at_pos) = spec.find('@') {
            let name = &spec[..at_pos];
            let version = spec[at_pos + 1..].split('(').next().unwrap_or(&spec[at_pos + 1..]);
            return Some((name.to_string(), version.to_string()));
        }
    }
    None
}

/// Generate NPM tarball URL for a package
fn npm_tarball_url(name: &str, version: &str) -> String {
    // Scoped packages have a different URL structure
    if name.starts_with('@') {
        let encoded_name = name.replace('/', "%2f");
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            encoded_name,
            name.split('/').last().unwrap_or(name),
            version
        )
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, version
        )
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Read and parse lockfile
    let lockfile_content = fs::read_to_string(&args.lock)
        .with_context(|| format!("Failed to read {}", args.lock.display()))?;

    let lockfile: PnpmLockfile = serde_yaml::from_str(&lockfile_content)
        .with_context(|| format!("Failed to parse {}", args.lock.display()))?;

    eprintln!(
        "Parsed pnpm-lock.yaml (version {})",
        lockfile.lockfile_version
    );

    // Collect packages
    let mut output_packages: Vec<OutputPackage> = Vec::new();

    for (spec, pkg) in &lockfile.packages {
        // Skip dev dependencies if not requested
        if pkg.dev && !args.include_dev {
            continue;
        }

        // Parse package name and version from the spec
        let (name, version) = match parse_package_spec(spec) {
            Some(nv) => nv,
            None => {
                eprintln!("warning: could not parse package spec: {}", spec);
                continue;
            }
        };

        // Get integrity hash
        let integrity = match &pkg.resolution {
            Some(res) => res.integrity.clone().unwrap_or_default(),
            None => String::new(),
        };

        if integrity.is_empty() {
            eprintln!("warning: no integrity hash for {}", spec);
            continue;
        }

        // Get tarball URL (use default npm URL if not specified)
        let url = match &pkg.resolution {
            Some(res) => res
                .tarball
                .clone()
                .unwrap_or_else(|| npm_tarball_url(&name, &version)),
            None => npm_tarball_url(&name, &version),
        };

        // Collect dependencies
        let dependencies: Vec<String> = pkg
            .dependencies
            .keys()
            .map(|k| k.to_string())
            .collect();

        output_packages.push(OutputPackage {
            name,
            version,
            url,
            integrity,
            dependencies,
        });
    }

    // Sort packages by name for deterministic output
    output_packages.sort_by(|a, b| a.name.cmp(&b.name));

    eprintln!("Found {} packages", output_packages.len());

    // Create output TOML
    let output = OutputToml {
        meta: OutputMeta {
            generator: format!("jsdeps-gen {}", env!("CARGO_PKG_VERSION")),
            lockfile_version: lockfile.lockfile_version,
        },
        packages: output_packages,
    };

    // Serialize to TOML
    let toml_str = toml::to_string_pretty(&output).context("Failed to serialize to TOML")?;

    // Write output
    if let Some(output_path) = args.output {
        let mut file = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        file.write_all(toml_str.as_bytes())?;
        eprintln!("Wrote {}", output_path.display());
    } else {
        print!("{}", toml_str);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_spec_simple() {
        let (name, version) = parse_package_spec("lodash@4.17.21").unwrap();
        assert_eq!(name, "lodash");
        assert_eq!(version, "4.17.21");
    }

    #[test]
    fn test_parse_package_spec_scoped() {
        let (name, version) = parse_package_spec("@types/node@22.10.10").unwrap();
        assert_eq!(name, "@types/node");
        assert_eq!(version, "22.10.10");
    }

    #[test]
    fn test_parse_package_spec_with_peer() {
        let (name, version) = parse_package_spec("@babel/core@7.26.0(@swc/core@1.10.14)").unwrap();
        assert_eq!(name, "@babel/core");
        assert_eq!(version, "7.26.0");
    }

    #[test]
    fn test_npm_tarball_url_simple() {
        let url = npm_tarball_url("lodash", "4.17.21");
        assert_eq!(
            url,
            "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
        );
    }

    #[test]
    fn test_npm_tarball_url_scoped() {
        let url = npm_tarball_url("@types/node", "22.10.10");
        assert_eq!(
            url,
            "https://registry.npmjs.org/@types%2fnode/-/node-22.10.10.tgz"
        );
    }
}
