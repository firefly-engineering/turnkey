//! TypeScript/JavaScript import extraction using tree-sitter.

use crate::extraction::{Import, ImportKind, Package, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use walkdir::WalkDir;

/// Node.js built-in modules.
const NODE_BUILTINS: &[&str] = &[
    "assert", "async_hooks", "buffer", "child_process", "cluster", "console",
    "constants", "crypto", "dgram", "diagnostics_channel", "dns", "domain",
    "events", "fs", "http", "http2", "https", "inspector", "module", "net",
    "os", "path", "perf_hooks", "process", "punycode", "querystring", "readline",
    "repl", "stream", "string_decoder", "sys", "timers", "tls", "trace_events",
    "tty", "url", "util", "v8", "vm", "wasi", "worker_threads", "zlib",
];

/// Default directories to exclude.
const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules", "dist", "build", ".next", "coverage", ".git", ".hg", ".svn",
];

/// Extract TypeScript/JavaScript imports from a directory.
pub fn extract(dir: &Path, exclude_patterns: &[&str]) -> anyhow::Result<Result> {
    let mut result = Result::new("typescript");

    let mut parser = Parser::new();
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
    parser.set_language(&language.into())?;

    // Query for import statements
    let query = Query::new(
        &language.into(),
        r#"
        (import_statement
          source: (string) @import_source)
        (export_statement
          source: (string) @export_source)
        (call_expression
          function: (identifier) @func_name
          arguments: (arguments (string) @require_arg))
        (call_expression
          function: (import)
          arguments: (arguments (string) @dynamic_import))
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
        if !matches!(extension, Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs")) {
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
        let is_test_file = filename.contains(".test.")
            || filename.contains(".spec.")
            || filename.ends_with("_test.ts")
            || filename.ends_with("_test.js")
            || pkg_dir.contains("__tests__");

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

                // Remove quotes from string literals
                let module_path = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');

                // Skip require if function name isn't "require"
                if capture.index == 2 {
                    // This is @require_arg, check if function was "require"
                    let func_capture = match_.captures.iter().find(|c| c.index == 1);
                    if let Some(fc) = func_capture {
                        let func_name = fc.node.utf8_text(source.as_bytes())?;
                        if func_name != "require" {
                            continue;
                        }
                    }
                }

                let (pkg_name, kind) = classify_ts_import(module_path);

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

/// Classify a TypeScript/JavaScript import.
fn classify_ts_import(module_path: &str) -> (String, ImportKind) {
    // Relative imports are internal
    if module_path.starts_with("./") || module_path.starts_with("../") {
        return (module_path.to_string(), ImportKind::Internal);
    }

    // Handle node: prefix
    if module_path.starts_with("node:") {
        return (module_path.to_string(), ImportKind::Stdlib);
    }

    // Get package name (handle scoped packages)
    let pkg_name = get_package_name(module_path);

    // Check for Node.js builtins
    if NODE_BUILTINS.contains(&pkg_name.as_str()) {
        return (pkg_name, ImportKind::Stdlib);
    }

    // Everything else is external
    (pkg_name, ImportKind::External)
}

/// Extract package name from import path.
/// "@org/pkg/subpath" -> "@org/pkg"
/// "pkg/subpath" -> "pkg"
fn get_package_name(module_path: &str) -> String {
    if module_path.starts_with('@') {
        // Scoped package
        let parts: Vec<&str> = module_path.splitn(3, '/').collect();
        if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            module_path.to_string()
        }
    } else {
        // Regular package
        module_path.split('/').next().unwrap_or(module_path).to_string()
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
