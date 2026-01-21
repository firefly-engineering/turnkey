//! Solidity import extraction using tree-sitter.

use crate::extraction::{Import, ImportKind, Package, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use walkdir::WalkDir;

/// Common Solidity library prefixes.
const COMMON_LIBRARIES: &[&str] = &[
    "@openzeppelin/", "@chainlink/", "@uniswap/", "@aave/", "@compound/",
    "@gnosis/", "@safe-global/", "forge-std/", "ds-test/", "solmate/", "solady/",
];

/// Default directories to exclude.
const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules", "lib", "out", "cache", "artifacts", "forge-cache",
    ".git", ".hg", ".svn",
];

/// Extract Solidity imports from a directory.
pub fn extract(dir: &Path, exclude_patterns: &[&str]) -> anyhow::Result<Result> {
    let mut result = Result::new("solidity");

    let mut parser = Parser::new();
    let language = tree_sitter_solidity::LANGUAGE;
    parser.set_language(&language.into())?;

    // Query for import statements
    let query = Query::new(
        &language.into(),
        r#"
        (import_directive
          source: (string) @import_source)
        "#,
    )?;

    let abs_dir = dir.canonicalize()?;
    let mut packages: HashMap<String, Package> = HashMap::new();

    for entry in WalkDir::new(&abs_dir)
        .into_iter()
        .filter_entry(|e| !should_exclude(e.file_name().to_str().unwrap_or(""), exclude_patterns))
    {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let extension = path.extension().and_then(|e| e.to_str());
        if extension != Some("sol") {
            continue;
        }

        let rel_path = path.strip_prefix(&abs_dir)?;
        let pkg_dir = rel_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let filename = rel_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Determine if this is a test file
        let is_test_file = filename.contains(".t.")
            || filename.ends_with("Test.sol")
            || filename.ends_with("_test.sol")
            || pkg_dir.contains("test");

        // Parse the file
        let source = std::fs::read_to_string(path)?;
        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => {
                result.add_error(format!("Failed to parse {}", rel_path.display()));
                continue;
            }
        };

        // Extract imports
        let mut cursor = QueryCursor::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut imports = Vec::new();

        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let node = capture.node;
                let text = node.utf8_text(source.as_bytes())?;

                // Remove quotes from string literal
                let import_path = text.trim_matches(|c| c == '"' || c == '\'');

                let (pkg_name, kind) = classify_sol_import(import_path);

                if !seen.contains(&pkg_name) {
                    seen.insert(pkg_name.clone());
                    imports.push(Import { path: pkg_name, kind });
                }
            }
        }

        // Add to package
        let pkg = packages.entry(pkg_dir.clone()).or_insert_with(|| Package::new(pkg_dir));
        pkg.files.push(filename);

        for imp in imports {
            if is_test_file {
                if !pkg.test_imports.contains(&imp) {
                    pkg.test_imports.push(imp);
                }
            } else if !pkg.imports.contains(&imp) {
                pkg.imports.push(imp);
            }
        }
    }

    // Sort and add packages to result
    let mut pkgs: Vec<_> = packages.into_values().collect();
    pkgs.sort_by(|a, b| a.path.cmp(&b.path));

    for mut pkg in pkgs {
        pkg.imports.sort_by(|a, b| a.path.cmp(&b.path));
        pkg.test_imports.sort_by(|a, b| a.path.cmp(&b.path));
        result.add_package(pkg);
    }

    Ok(result)
}

/// Classify a Solidity import.
fn classify_sol_import(import_path: &str) -> (String, ImportKind) {
    // Relative imports are internal
    if import_path.starts_with("./") || import_path.starts_with("../") {
        return (import_path.to_string(), ImportKind::Internal);
    }

    // Get package name
    let pkg_name = get_package_name(import_path);

    // Check for common libraries (they're external)
    for prefix in COMMON_LIBRARIES {
        if import_path.starts_with(prefix) {
            return (pkg_name, ImportKind::External);
        }
    }

    // Scoped packages and non-relative paths are external
    if import_path.starts_with('@') || !import_path.starts_with('/') {
        return (pkg_name, ImportKind::External);
    }

    // Default to internal
    (import_path.to_string(), ImportKind::Internal)
}

/// Extract package name from import path.
/// "@openzeppelin/contracts/token/ERC20.sol" -> "@openzeppelin/contracts"
/// "forge-std/Test.sol" -> "forge-std"
fn get_package_name(import_path: &str) -> String {
    if import_path.starts_with('@') {
        // Scoped package
        let parts: Vec<&str> = import_path.splitn(3, '/').collect();
        if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            import_path.to_string()
        }
    } else {
        // Regular package
        import_path.split('/').next().unwrap_or(import_path).to_string()
    }
}

/// Check if a directory/file should be excluded.
fn should_exclude(name: &str, patterns: &[&str]) -> bool {
    if name.starts_with('.') {
        return true;
    }

    for pattern in DEFAULT_EXCLUDES.iter().chain(patterns.iter()) {
        if name == *pattern || name.contains(pattern) {
            return true;
        }
    }

    false
}
