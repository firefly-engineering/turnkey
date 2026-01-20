//! rustfeatures-gen - Generate rust-features.toml from workspace Cargo.toml files
//!
//! This tool scans Cargo workspace members and extracts explicit feature requirements
//! for dependencies, generating a rust-features.toml file with the necessary overrides.
//!
//! Usage:
//!   rustfeatures-gen [--cargo-toml path/to/Cargo.toml] [-o rust-features.toml]

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "rustfeatures-gen")]
#[command(about = "Generate rust-features.toml from workspace Cargo.toml files")]
struct Args {
    /// Path to root Cargo.toml (workspace)
    #[arg(long = "cargo-toml", default_value = "Cargo.toml")]
    cargo_toml: PathBuf,

    /// Output file (stdout if not specified)
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    /// Include comments explaining where each feature came from
    #[arg(long = "annotate", default_value = "true")]
    annotate: bool,

    /// Crates to skip (comma-separated)
    #[arg(long = "skip")]
    skip: Option<String>,
}

/// Collected features for a crate
#[derive(Debug, Default)]
struct CrateFeatures {
    features: BTreeSet<String>,
    /// Which workspace members requested this crate with features
    sources: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let skip_crates: BTreeSet<String> = args
        .skip
        .as_deref()
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Parse root Cargo.toml
    let root_content =
        fs::read_to_string(&args.cargo_toml).context("Failed to read root Cargo.toml")?;
    let root_toml: toml::Value =
        toml::from_str(&root_content).context("Failed to parse root Cargo.toml")?;

    let root_dir = args
        .cargo_toml
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    // Get workspace members
    let members = get_workspace_members(&root_toml, &root_dir)?;

    // Collect features from all workspace members
    let mut crate_features: BTreeMap<String, CrateFeatures> = BTreeMap::new();

    for member_path in &members {
        let cargo_toml_path = member_path.join("Cargo.toml");
        if !cargo_toml_path.exists() {
            eprintln!(
                "Warning: {} does not exist, skipping",
                cargo_toml_path.display()
            );
            continue;
        }

        let content =
            fs::read_to_string(&cargo_toml_path).context("Failed to read member Cargo.toml")?;
        let member_toml: toml::Value =
            toml::from_str(&content).context("Failed to parse member Cargo.toml")?;

        // Get member name for annotation
        let member_name = member_toml
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or_else(|| {
                member_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            })
            .to_string();

        // Extract features from dependencies
        if let Some(deps) = member_toml.get("dependencies") {
            collect_dependency_features(deps, &member_name, &skip_crates, &mut crate_features);
        }

        // Also check dev-dependencies
        if let Some(deps) = member_toml.get("dev-dependencies") {
            collect_dependency_features(deps, &member_name, &skip_crates, &mut crate_features);
        }

        // Also check build-dependencies
        if let Some(deps) = member_toml.get("build-dependencies") {
            collect_dependency_features(deps, &member_name, &skip_crates, &mut crate_features);
        }
    }

    // Also check workspace.dependencies in root Cargo.toml
    if let Some(workspace) = root_toml.get("workspace")
        && let Some(deps) = workspace.get("dependencies")
    {
        collect_dependency_features(deps, "workspace", &skip_crates, &mut crate_features);
    }

    // Filter to only crates that have explicit features
    let crates_with_features: BTreeMap<_, _> = crate_features
        .into_iter()
        .filter(|(_, cf)| !cf.features.is_empty())
        .collect();

    // Generate output
    let output = generate_output(&crates_with_features, args.annotate);

    // Write output
    if let Some(output_path) = &args.output {
        fs::write(output_path, &output).context("Failed to write output file")?;
        eprintln!("Wrote rust-features.toml to {}", output_path.display());
    } else {
        print!("{}", output);
    }

    Ok(())
}

/// Get workspace members from root Cargo.toml
fn get_workspace_members(root_toml: &toml::Value, root_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut members = Vec::new();

    if let Some(workspace) = root_toml.get("workspace")
        && let Some(member_list) = workspace.get("members")
        && let Some(arr) = member_list.as_array()
    {
        for member in arr {
            if let Some(member_str) = member.as_str() {
                // Handle glob patterns
                if member_str.contains('*') {
                    let pattern = root_dir.join(member_str);
                    // Simple glob expansion (just handle basic patterns)
                    if let Some(parent) = pattern.parent()
                        && let Ok(entries) = fs::read_dir(parent)
                    {
                        for entry in entries.flatten() {
                            if entry.path().is_dir() {
                                members.push(entry.path());
                            }
                        }
                    }
                } else {
                    members.push(root_dir.join(member_str));
                }
            }
        }
    }

    Ok(members)
}

/// Collect features from a dependencies table
fn collect_dependency_features(
    deps: &toml::Value,
    source_name: &str,
    skip_crates: &BTreeSet<String>,
    crate_features: &mut BTreeMap<String, CrateFeatures>,
) {
    if let Some(deps_table) = deps.as_table() {
        for (crate_name, dep_value) in deps_table {
            // Normalize crate name (replace hyphens with underscores for consistency)
            let normalized_name = crate_name.replace('-', "_");

            // Skip if in skip list
            if skip_crates.contains(&normalized_name) || skip_crates.contains(crate_name) {
                continue;
            }

            // Extract features
            let features = extract_features(dep_value);

            if !features.is_empty() {
                let entry = crate_features
                    .entry(crate_name.clone())
                    .or_default();
                entry.features.extend(features);
                entry.sources.push(source_name.to_string());
            }
        }
    }
}

/// Extract features from a dependency value
fn extract_features(dep_value: &toml::Value) -> Vec<String> {
    match dep_value {
        // Inline table: { version = "1", features = ["derive"] }
        toml::Value::Table(t) => {
            if let Some(features) = t.get("features")
                && let Some(arr) = features.as_array()
            {
                return arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
            Vec::new()
        }
        // Simple version string: "1.0"
        toml::Value::String(_) => Vec::new(),
        _ => Vec::new(),
    }
}

/// Generate the rust-features.toml output
fn generate_output(crate_features: &BTreeMap<String, CrateFeatures>, annotate: bool) -> String {
    let mut output = String::new();

    output.push_str("# Auto-generated by rustfeatures-gen\n");
    output.push_str("# This file declares feature overrides for vendored Rust crates.\n");
    output.push_str("#\n");
    output.push_str("# Features listed here are added to the computed unified features\n");
    output.push_str("# from Cargo.lock, ensuring local workspace members have access\n");
    output.push_str("# to the features they need.\n");
    output.push_str("#\n");
    output.push_str("# Regenerate with: rustfeatures-gen --cargo-toml Cargo.toml -o rust-features.toml\n");
    output.push_str("\n[overrides]\n");

    for (crate_name, cf) in crate_features {
        if annotate && !cf.sources.is_empty() {
            let sources = cf.sources.join(", ");
            output.push_str(&format!("# Used by: {}\n", sources));
        }

        let features: Vec<_> = cf.features.iter().map(|s| format!("\"{}\"", s)).collect();
        output.push_str(&format!(
            "{} = {{ add = [{}] }}\n",
            crate_name,
            features.join(", ")
        ));

        if annotate {
            output.push('\n');
        }
    }

    output
}
