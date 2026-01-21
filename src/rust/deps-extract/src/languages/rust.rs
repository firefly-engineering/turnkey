//! Rust import extraction using tree-sitter.

use crate::extraction::{Import, ImportKind, Package, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use walkdir::WalkDir;

/// Rust standard library crates.
const RUST_STDLIB: &[&str] = &[
    "std", "core", "alloc", "proc_macro", "test",
];

/// Default directories to exclude.
const DEFAULT_EXCLUDES: &[&str] = &[
    "target", ".git", ".hg", ".svn", "node_modules",
];

/// Extract Rust imports from a directory.
pub fn extract(dir: &Path, exclude_patterns: &[&str]) -> anyhow::Result<Result> {
    let mut result = Result::new("rust");

    let mut parser = Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    parser.set_language(&language.into())?;

    // Query for use declarations, extern crate, and qualified paths
    // use foo::bar;
    // use foo::{bar, baz};
    // use crate::foo;
    // use self::foo;
    // use super::foo;
    // extern crate foo;
    // foo::bar() (qualified call)
    let query = Query::new(
        &language.into(),
        r#"
        (use_declaration
          argument: (scoped_identifier
            path: (identifier) @use_root))
        (use_declaration
          argument: (scoped_identifier
            path: (scoped_identifier) @use_path))
        (use_declaration
          argument: (identifier) @use_simple)
        (use_declaration
          argument: (use_wildcard
            (identifier) @use_wildcard_root))
        (use_declaration
          argument: (scoped_use_list
            path: (identifier) @use_list_root))
        (use_declaration
          argument: (scoped_use_list
            path: (scoped_identifier) @use_list_path))
        (extern_crate_declaration
          name: (identifier) @extern_crate)

        ; Match qualified paths like foo::bar::baz
        (scoped_identifier
          path: (identifier) @qualified_root)
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
        if extension != Some("rs") {
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
        let is_test_file = filename.ends_with("_test.rs")
            || filename == "tests.rs"
            || pkg_dir.contains("tests");

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

                // Get the root crate name
                let crate_name = get_crate_name(text);

                if let Some((name, kind)) = classify_rust_import(&crate_name) {
                    if !seen.contains(&name) {
                        seen.insert(name.clone());
                        imports.push(Import { path: name, kind });
                    }
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

/// Get the root crate name from a use path.
fn get_crate_name(path: &str) -> String {
    // Handle scoped identifiers like "foo::bar" -> "foo"
    path.split("::").next().unwrap_or(path).to_string()
}

/// Classify a Rust import.
fn classify_rust_import(crate_name: &str) -> Option<(String, ImportKind)> {
    // Skip internal references
    if crate_name == "crate" || crate_name == "self" || crate_name == "super" {
        return None;
    }

    // Check if it's stdlib
    if RUST_STDLIB.contains(&crate_name) {
        return Some((crate_name.to_string(), ImportKind::Stdlib));
    }

    // Everything else is external
    Some((crate_name.to_string(), ImportKind::External))
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
