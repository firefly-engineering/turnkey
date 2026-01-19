//! soldeps-gen: Generate solidity-deps.toml from foundry.toml and package.json
//!
//! This tool parses Foundry's foundry.toml for git dependencies and optionally
//! package.json/pnpm-lock.yaml for npm Solidity packages (like @openzeppelin/contracts).
//! It generates a unified TOML file for use with turnkey's solidity-deps-cell.nix.

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Generate solidity-deps.toml from foundry.toml and package.json
#[derive(Parser, Debug)]
#[command(name = "soldeps-gen")]
#[command(about = "Generate solidity-deps.toml from foundry.toml and package.json")]
struct Args {
    /// Path to foundry.toml file
    #[arg(long, default_value = "foundry.toml")]
    foundry: PathBuf,

    /// Path to package.json file (optional, for npm deps)
    #[arg(long)]
    package_json: Option<PathBuf>,

    /// Path to pnpm-lock.yaml file (optional, for npm integrity hashes)
    #[arg(long)]
    pnpm_lock: Option<PathBuf>,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Fetch git commit hashes for dependencies (slower but more complete)
    #[arg(long, default_value = "false")]
    fetch_git_info: bool,
}

/// Represents a package in the output TOML
#[derive(Debug, Serialize)]
struct OutputPackage {
    name: String,
    version: String,
    source: String, // "git" or "npm"
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    /// Auto-generated remapping for this package
    #[serde(skip_serializing_if = "Option::is_none")]
    remapping: Option<String>,
}

/// Foundry configuration structure
#[derive(Debug, Deserialize)]
struct FoundryConfig {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    /// Profile configurations - parsed for schema completeness
    #[serde(default)]
    #[allow(dead_code)]
    profile: BTreeMap<String, FoundryProfile>,
}

/// Foundry profile configuration
/// Parsed for schema completeness; remappings may be used in future enhancements.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FoundryProfile {
    #[serde(default)]
    remappings: Vec<String>,
    #[serde(default)]
    libs: Vec<String>,
}

/// package.json structure (simplified)
#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    #[serde(rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, String>,
}

/// Output TOML structure
#[derive(Debug, Serialize)]
struct OutputToml {
    meta: OutputMeta,
    #[serde(rename = "package")]
    packages: Vec<OutputPackage>,
}

#[derive(Debug, Serialize)]
struct OutputMeta {
    generator: String,
}

/// Parse Foundry git dependency format
/// Examples:
///   "solady": "https://github.com/vectorized/solady"
///   "forge-std": "https://github.com/foundry-rs/forge-std@v1.8.0"
///   "openzeppelin": "openzeppelin/openzeppelin-contracts@v5.0.0"
fn parse_foundry_dep(name: &str, spec: &str) -> OutputPackage {
    let (repo, rev) = if spec.contains('@') {
        let parts: Vec<&str> = spec.splitn(2, '@').collect();
        (parts[0].to_string(), Some(parts[1].to_string()))
    } else {
        (spec.to_string(), None)
    };

    // Normalize GitHub shorthand
    let full_repo = if !repo.starts_with("http") && repo.contains('/') {
        format!("https://github.com/{}", repo)
    } else {
        repo
    };

    // Generate remapping
    let remapping = format!("{}=/", name);

    OutputPackage {
        name: name.to_string(),
        version: rev.clone().unwrap_or_else(|| "main".to_string()),
        source: "git".to_string(),
        url: None,
        integrity: None,
        repo: Some(full_repo),
        rev,
        remapping: Some(remapping),
    }
}

/// Check if a package is a Solidity-related npm package
fn is_solidity_package(name: &str) -> bool {
    // Common Solidity npm packages
    let solidity_prefixes = [
        "@openzeppelin/contracts",
        "@openzeppelin/contracts-upgradeable",
        "@chainlink/contracts",
        "@uniswap/",
        "solmate",
        "forge-std",
        "ds-test",
    ];

    solidity_prefixes.iter().any(|p| name.starts_with(p) || name == *p)
}

/// Generate NPM tarball URL for a package
fn npm_tarball_url(name: &str, version: &str) -> String {
    // Remove semver prefix (^, ~, etc.)
    let clean_version = version.trim_start_matches(|c| c == '^' || c == '~' || c == '=' || c == 'v');

    if name.starts_with('@') {
        let encoded_name = name.replace('/', "%2f");
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            encoded_name,
            name.split('/').next_back().unwrap_or(name),
            clean_version
        )
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, clean_version
        )
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut output_packages: Vec<OutputPackage> = Vec::new();

    // Parse foundry.toml if it exists
    if args.foundry.exists() {
        let foundry_content = fs::read_to_string(&args.foundry)
            .with_context(|| format!("Failed to read {}", args.foundry.display()))?;

        let foundry_config: FoundryConfig = toml::from_str(&foundry_content)
            .with_context(|| format!("Failed to parse {}", args.foundry.display()))?;

        eprintln!(
            "Parsed foundry.toml with {} dependencies",
            foundry_config.dependencies.len()
        );

        for (name, spec) in &foundry_config.dependencies {
            let pkg = parse_foundry_dep(name, spec);
            output_packages.push(pkg);
        }
    } else {
        eprintln!("Note: {} not found, skipping Foundry deps", args.foundry.display());
    }

    // Parse package.json if provided
    if let Some(package_json_path) = &args.package_json {
        if package_json_path.exists() {
            let package_json_content = fs::read_to_string(package_json_path)
                .with_context(|| format!("Failed to read {}", package_json_path.display()))?;

            let package_json: PackageJson = serde_json::from_str(&package_json_content)
                .with_context(|| format!("Failed to parse {}", package_json_path.display()))?;

            let all_deps: Vec<(&String, &String)> = package_json
                .dependencies
                .iter()
                .chain(package_json.dev_dependencies.iter())
                .filter(|(name, _)| is_solidity_package(name))
                .collect();

            eprintln!(
                "Found {} Solidity-related npm packages in package.json",
                all_deps.len()
            );

            for (name, version) in all_deps {
                let clean_version = version.trim_start_matches(|c| c == '^' || c == '~' || c == '=');

                // Generate remapping for npm package
                let remapping = if name.starts_with('@') {
                    format!("{}/=node_modules/{}/", name, name)
                } else {
                    format!("{}/=node_modules/{}/", name, name)
                };

                output_packages.push(OutputPackage {
                    name: name.clone(),
                    version: clean_version.to_string(),
                    source: "npm".to_string(),
                    url: Some(npm_tarball_url(name, version)),
                    integrity: None, // Would need pnpm-lock.yaml to get this
                    repo: None,
                    rev: None,
                    remapping: Some(remapping),
                });
            }
        } else {
            eprintln!("Note: {} not found, skipping npm deps", package_json_path.display());
        }
    }

    // Sort packages by name for deterministic output
    output_packages.sort_by(|a, b| a.name.cmp(&b.name));

    eprintln!("Total: {} packages", output_packages.len());

    // Create output TOML
    let output = OutputToml {
        meta: OutputMeta {
            generator: format!("soldeps-gen {}", env!("CARGO_PKG_VERSION")),
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
    fn test_parse_foundry_dep_simple() {
        let pkg = parse_foundry_dep("solady", "https://github.com/vectorized/solady");
        assert_eq!(pkg.name, "solady");
        assert_eq!(pkg.source, "git");
        assert_eq!(pkg.repo, Some("https://github.com/vectorized/solady".to_string()));
        assert_eq!(pkg.rev, None);
    }

    #[test]
    fn test_parse_foundry_dep_with_version() {
        let pkg = parse_foundry_dep("forge-std", "https://github.com/foundry-rs/forge-std@v1.8.0");
        assert_eq!(pkg.name, "forge-std");
        assert_eq!(pkg.repo, Some("https://github.com/foundry-rs/forge-std".to_string()));
        assert_eq!(pkg.rev, Some("v1.8.0".to_string()));
    }

    #[test]
    fn test_parse_foundry_dep_shorthand() {
        let pkg = parse_foundry_dep("openzeppelin", "openzeppelin/openzeppelin-contracts@v5.0.0");
        assert_eq!(pkg.repo, Some("https://github.com/openzeppelin/openzeppelin-contracts".to_string()));
        assert_eq!(pkg.rev, Some("v5.0.0".to_string()));
    }

    #[test]
    fn test_is_solidity_package() {
        assert!(is_solidity_package("@openzeppelin/contracts"));
        assert!(is_solidity_package("@openzeppelin/contracts-upgradeable"));
        assert!(is_solidity_package("@chainlink/contracts"));
        assert!(!is_solidity_package("lodash"));
        assert!(!is_solidity_package("typescript"));
    }

    #[test]
    fn test_npm_tarball_url() {
        let url = npm_tarball_url("@openzeppelin/contracts", "5.0.0");
        assert_eq!(
            url,
            "https://registry.npmjs.org/@openzeppelin%2fcontracts/-/contracts-5.0.0.tgz"
        );
    }
}
