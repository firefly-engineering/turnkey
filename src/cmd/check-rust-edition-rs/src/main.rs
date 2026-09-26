//! Check that Rust editions are consistent between Cargo.toml and rules.star files.
//!
//! This tool verifies:
//! 1. All workspace members use edition.workspace = true
//! 2. rules.star files have edition matching workspace.package.edition

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Find the workspace root (directory with Cargo.toml containing [workspace])
fn find_workspace_root() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let mut root = cwd.clone();

    while root.parent().is_some() {
        let cargo_toml = root.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = fs::read_to_string(&cargo_toml) {
                if let Ok(cargo) = content.parse::<toml::Table>() {
                    if cargo.contains_key("workspace") {
                        return Some(root);
                    }
                }
            }
        }
        root = root.parent()?.to_path_buf();
    }

    None
}

/// Extract the workspace edition from Cargo.toml
fn get_workspace_edition(cargo: &toml::Table) -> Option<String> {
    cargo
        .get("workspace")?
        .get("package")?
        .get("edition")?
        .as_str()
        .map(String::from)
}

/// Get workspace members from Cargo.toml
fn get_workspace_members(cargo: &toml::Table) -> Vec<String> {
    cargo
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract edition values from a rules.star file using tree-sitter
fn extract_buck_editions(buck_path: &Path) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(buck_path)
        .with_context(|| format!("Failed to read {}", buck_path.display()))?;

    let targets = starlark_parse::parse_targets(&content)?;
    let mut editions = Vec::new();

    for target in targets {
        // Only check Rust targets
        if target.rule == "rust_binary"
            || target.rule == "rust_library"
            || target.rule == "rust_test"
        {
            if let Some(edition) = target.args.get("edition") {
                if let Some(edition_str) = edition.as_str() {
                    editions.push((target.name.clone(), edition_str.to_string()));
                }
            }
        }
    }

    Ok(editions)
}

/// Check a workspace member's Cargo.toml and rules.star file
fn check_workspace_member(
    member_path: &Path,
    workspace_edition: &str,
    errors: &mut Vec<String>,
) -> Result<()> {
    let cargo_toml = member_path.join("Cargo.toml");
    let buck_file = member_path.join("rules.star");

    if !cargo_toml.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    let cargo: toml::Table = content
        .parse()
        .with_context(|| format!("Failed to parse {}", cargo_toml.display()))?;

    // Check edition in Cargo.toml
    if let Some(package) = cargo.get("package").and_then(|p| p.as_table()) {
        if let Some(edition) = package.get("edition") {
            if let Some(edition_table) = edition.as_table() {
                // edition = { workspace = true }
                if edition_table.get("workspace").and_then(|v| v.as_bool()) != Some(true) {
                    errors.push(format!(
                        "{}: edition should use 'edition.workspace = true'",
                        cargo_toml.display()
                    ));
                }
            } else if let Some(edition_str) = edition.as_str() {
                // edition = "2024"
                errors.push(format!(
                    "{}: should use 'edition.workspace = true' instead of 'edition = \"{}\"'",
                    cargo_toml.display(),
                    edition_str
                ));
            }
        } else {
            // No edition specified
            errors.push(format!(
                "{}: no edition specified, should use 'edition.workspace = true'",
                cargo_toml.display()
            ));
        }
    }

    // Check rules.star file if it exists
    if buck_file.exists() {
        match extract_buck_editions(&buck_file) {
            Ok(buck_editions) => {
                for (target_name, buck_edition) in buck_editions {
                    if buck_edition != workspace_edition {
                        errors.push(format!(
                            "{}: target '{}' has edition = \"{}\", expected \"{}\" (from workspace.package.edition)",
                            buck_file.display(),
                            target_name,
                            buck_edition,
                            workspace_edition
                        ));
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse {}: {}", buck_file.display(), e);
            }
        }
    }

    Ok(())
}

/// The outcome of checking a workspace: the edition every member must use,
/// and one message per member or target that doesn't
struct Report {
    workspace_edition: String,
    errors: Vec<String>,
}

/// Check every workspace member under `root` against the workspace edition
fn check_workspace(root: &Path) -> Result<Report> {
    let cargo_toml = root.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    let cargo: toml::Table = content
        .parse()
        .with_context(|| format!("Failed to parse {}", cargo_toml.display()))?;

    // Get workspace edition
    let workspace_edition = get_workspace_edition(&cargo).with_context(|| {
        format!(
            "{} missing [workspace.package] edition",
            cargo_toml.display()
        )
    })?;

    let mut errors = Vec::new();

    // Check each member
    for member_pattern in &get_workspace_members(&cargo) {
        if member_pattern.contains('*') {
            // Handle glob patterns
            let pattern = root.join(member_pattern);
            let pattern_str = pattern.to_string_lossy();
            if let Ok(paths) = glob::glob(&pattern_str) {
                for entry in paths.filter_map(|e| e.ok()) {
                    if entry.is_dir() && entry.join("Cargo.toml").exists() {
                        check_workspace_member(&entry, &workspace_edition, &mut errors)?;
                    }
                }
            }
        } else {
            let member_path = root.join(member_pattern);
            if member_path.exists() {
                check_workspace_member(&member_path, &workspace_edition, &mut errors)?;
            }
        }
    }

    Ok(Report {
        workspace_edition,
        errors,
    })
}

fn main() -> Result<()> {
    // Find workspace root
    let root = find_workspace_root()
        .context("Could not find workspace root (Cargo.toml with [workspace])")?;

    let Report {
        workspace_edition,
        errors,
    } = check_workspace(&root)?;

    println!("Workspace edition: {}", workspace_edition);

    // Report results
    if !errors.is_empty() {
        println!("\nFound {} edition alignment issue(s):\n", errors.len());
        for error in &errors {
            println!("  - {}", error);
        }
        println!("\nTo fix:");
        println!("  1. Update Cargo.toml files to use 'edition.workspace = true'");
        println!(
            "  2. Update rules.star files to use 'edition = \"{}\"'",
            workspace_edition
        );
        std::process::exit(1);
    }

    println!("All editions are aligned");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    const WORKSPACE: &str = r#"
[workspace]
members = ["crates/*", "tools/single"]

[workspace.package]
edition = "2024"
"#;

    const GOOD_MEMBER: &str = r#"
[package]
name = "good"
edition.workspace = true
"#;

    fn workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "Cargo.toml", WORKSPACE);
        dir
    }

    #[test]
    fn workspace_edition_is_read_from_workspace_package() {
        let cargo: toml::Table = WORKSPACE.parse().unwrap();
        assert_eq!(get_workspace_edition(&cargo).as_deref(), Some("2024"));
    }

    #[test]
    fn workspace_edition_is_none_without_workspace_package() {
        let cargo: toml::Table = "[workspace]\nmembers = []\n".parse().unwrap();
        assert_eq!(get_workspace_edition(&cargo), None);
    }

    #[test]
    fn workspace_members_are_listed_in_order() {
        let cargo: toml::Table = WORKSPACE.parse().unwrap();
        assert_eq!(
            get_workspace_members(&cargo),
            vec!["crates/*", "tools/single"]
        );
    }

    #[test]
    fn aligned_workspace_has_no_errors() {
        let dir = workspace();
        write(dir.path(), "crates/a/Cargo.toml", GOOD_MEMBER);
        write(
            dir.path(),
            "crates/a/rules.star",
            r#"rust_library(name = "a", srcs = [], edition = "2024")"#,
        );
        write(dir.path(), "tools/single/Cargo.toml", GOOD_MEMBER);

        assert_eq!(
            check_workspace(dir.path()).unwrap().errors,
            Vec::<String>::new()
        );
    }

    #[test]
    fn literal_cargo_edition_is_reported() {
        let dir = workspace();
        write(
            dir.path(),
            "tools/single/Cargo.toml",
            "[package]\nname = \"x\"\nedition = \"2021\"\n",
        );

        let errors = check_workspace(dir.path()).unwrap().errors;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("instead of 'edition = \"2021\"'"),
            "{errors:?}"
        );
    }

    #[test]
    fn missing_cargo_edition_is_reported() {
        let dir = workspace();
        write(
            dir.path(),
            "crates/b/Cargo.toml",
            "[package]\nname = \"b\"\n",
        );

        let errors = check_workspace(dir.path()).unwrap().errors;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("no edition specified"), "{errors:?}");
    }

    #[test]
    fn mismatched_rust_target_edition_is_reported_per_target() {
        let dir = workspace();
        write(dir.path(), "crates/c/Cargo.toml", GOOD_MEMBER);
        write(
            dir.path(),
            "crates/c/rules.star",
            r#"
rust_binary(name = "c", srcs = [], edition = "2021")
rust_test(name = "c-test", srcs = [], edition = "2021")
rust_library(name = "c-lib", srcs = [], edition = "2024")
"#,
        );

        let errors = check_workspace(dir.path()).unwrap().errors;
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].contains("target 'c' has edition = \"2021\""),
            "{errors:?}"
        );
        assert!(
            errors[1].contains("target 'c-test' has edition = \"2021\""),
            "{errors:?}"
        );
    }

    #[test]
    fn non_rust_targets_are_ignored() {
        let dir = workspace();
        write(dir.path(), "crates/d/Cargo.toml", GOOD_MEMBER);
        write(
            dir.path(),
            "crates/d/rules.star",
            r#"genrule(name = "d", out = "x", cmd = "true", edition = "2021")"#,
        );

        assert_eq!(
            check_workspace(dir.path()).unwrap().errors,
            Vec::<String>::new()
        );
    }

    #[test]
    fn glob_member_directories_without_cargo_toml_are_skipped() {
        let dir = workspace();
        write(dir.path(), "crates/not-a-crate/README.md", "");

        assert_eq!(
            check_workspace(dir.path()).unwrap().errors,
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_workspace_edition_is_an_error() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "Cargo.toml", "[workspace]\nmembers = []\n");

        assert!(check_workspace(dir.path()).is_err());
    }
}
