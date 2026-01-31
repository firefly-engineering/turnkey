//! Check that all source files are covered by Buck2 targets.
//!
//! This tool ensures no source files are accidentally forgotten when adding
//! new code to the repository. It parses rules.star files using tree-sitter
//! to extract source patterns and compares them against actual source files.

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Source file extensions we care about
const SOURCE_EXTENSIONS: &[&str] = &[".go", ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".sol"];

/// Directories to always exclude
const EXCLUDED_DIRS: &[&str] = &[
    "__pycache__",
    ".git",
    "node_modules",
    "target",
    "buck-out",
    ".turnkey",
    ".devenv",
    "vendor",
    ".beads",
];

/// Files to always exclude
const EXCLUDED_FILES: &[&str] = &[
    "__init__.py", // Often not explicitly listed but implicitly included
];

#[derive(Parser)]
#[command(name = "check-source-coverage-rs")]
#[command(about = "Check that all source files are covered by Buck2 targets")]
struct Args {
    /// Directory scope to check
    #[arg(long, default_value = "src/")]
    scope: String,

    /// Show verbose output including covered files
    #[arg(short, long)]
    verbose: bool,
}

fn find_project_root() -> PathBuf {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut root = cwd.clone();

    while root.parent().is_some() {
        if root.join(".git").exists() || root.join("flake.nix").exists() {
            return root;
        }
        root = root.parent().unwrap().to_path_buf();
    }

    cwd
}

fn is_excluded_dir(name: &str) -> bool {
    EXCLUDED_DIRS.contains(&name)
}

fn is_source_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext_str = format!(".{}", ext.to_string_lossy());
        SOURCE_EXTENSIONS.contains(&ext_str.as_str())
    } else {
        false
    }
}

fn is_excluded_file(path: &Path) -> bool {
    if let Some(name) = path.file_name() {
        EXCLUDED_FILES.contains(&name.to_string_lossy().as_ref())
    } else {
        false
    }
}

fn find_all_source_files(scope_dir: &Path) -> HashSet<PathBuf> {
    let mut files = HashSet::new();

    for entry in WalkDir::new(scope_dir)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    return !is_excluded_dir(name);
                }
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && is_source_file(path) && !is_excluded_file(path) {
            files.insert(path.to_path_buf());
        }
    }

    files
}

fn find_rules_star_files(scope_dir: &Path) -> Vec<PathBuf> {
    let mut rules_files = Vec::new();

    for entry in WalkDir::new(scope_dir)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    return !is_excluded_dir(name);
                }
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.file_name() == Some("rules.star".as_ref()) {
            rules_files.push(path.to_path_buf());
        }
    }

    rules_files
}

/// Expand a glob pattern relative to a base directory
fn expand_glob_pattern(base_dir: &Path, pattern: &str) -> HashSet<PathBuf> {
    let mut files = HashSet::new();

    if pattern.contains("**") {
        // Handle recursive glob patterns
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0].trim_end_matches('/');
            let suffix = parts[1].trim_start_matches('/');

            let search_dir = if prefix.is_empty() {
                base_dir.to_path_buf()
            } else {
                base_dir.join(prefix)
            };

            if search_dir.exists() {
                for entry in WalkDir::new(&search_dir)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let path = entry.path();
                    if path.is_file() {
                        // Check if the file matches the suffix pattern
                        if let Some(file_name) = path.file_name() {
                            let file_name_str = file_name.to_string_lossy();
                            if matches_suffix(&file_name_str, suffix) {
                                files.insert(path.to_path_buf());
                            }
                        }
                    }
                }
            }
        }
    } else if pattern.contains('*') {
        // Simple glob with wildcard
        if let Ok(entries) = glob::glob(&base_dir.join(pattern).to_string_lossy()) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.is_file() {
                    files.insert(entry);
                }
            }
        } else {
            // Fall back to manual matching if glob crate not available
            // Try simple extension matching
            if let Some(ext) = pattern.strip_prefix("*.") {
                for entry in WalkDir::new(base_dir)
                    .max_depth(1)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(path_ext) = path.extension() {
                            if path_ext.to_string_lossy() == ext {
                                files.insert(path.to_path_buf());
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Not a glob, treat as literal file
        let file_path = base_dir.join(pattern);
        if file_path.exists() {
            files.insert(file_path);
        }
    }

    files
}

fn matches_suffix(file_name: &str, suffix: &str) -> bool {
    // Simple suffix matching (e.g., "*.rs" -> ".rs")
    if let Some(ext_pattern) = suffix.strip_prefix("*.") {
        if let Some(dot_pos) = file_name.rfind('.') {
            return &file_name[dot_pos + 1..] == ext_pattern;
        }
        return false;
    }
    // Exact match
    file_name == suffix
}

fn expand_patterns(rules_dir: &Path, patterns: &[String]) -> HashSet<PathBuf> {
    let mut files = HashSet::new();

    for pattern in patterns {
        if pattern.contains('*') {
            files.extend(expand_glob_pattern(rules_dir, pattern));
        } else {
            let file_path = rules_dir.join(pattern);
            if file_path.exists() {
                files.insert(file_path);
            }
        }
    }

    files
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Allow override via environment variable
    let scope = env::var("TURNKEY_SOURCE_SCOPE").unwrap_or(args.scope);

    let project_root = find_project_root();
    let scope_dir = project_root.join(&scope);

    if !scope_dir.exists() {
        anyhow::bail!("Scope directory does not exist: {}", scope_dir.display());
    }

    println!("Checking source coverage in: {}", scope_dir.display());

    // Find all source files
    let all_source_files = find_all_source_files(&scope_dir);
    println!("Found {} source files", all_source_files.len());

    // Find all rules.star files
    let rules_files = find_rules_star_files(&scope_dir);
    println!("Found {} rules.star files", rules_files.len());

    // Track covered files
    let mut covered_files: HashSet<PathBuf> = HashSet::new();
    let mut coverage_map: HashMap<PathBuf, Vec<String>> = HashMap::new();

    for rules_file in &rules_files {
        let rules_dir = rules_file.parent().unwrap();

        let content = std::fs::read_to_string(rules_file)
            .with_context(|| format!("Failed to read {}", rules_file.display()))?;

        match starlark_parse::parse_targets(&content) {
            Ok(targets) => {
                for target in targets {
                    // Get patterns from srcs, main, crate_root, or src
                    let mut patterns = Vec::new();

                    if let Some(srcs) = target.args.get("srcs") {
                        patterns.extend(srcs.collect_patterns());
                    }
                    if let Some(main) = target.args.get("main") {
                        patterns.extend(main.collect_patterns());
                    }
                    if let Some(crate_root) = target.args.get("crate_root") {
                        patterns.extend(crate_root.collect_patterns());
                    }
                    if let Some(src) = target.args.get("src") {
                        patterns.extend(src.collect_patterns());
                    }

                    let target_files = expand_patterns(rules_dir, &patterns);
                    for f in target_files {
                        covered_files.insert(f.clone());
                        let rel_rules = rules_file.strip_prefix(&project_root).unwrap_or(rules_file);
                        let target_ref = format!("{}:{}", rel_rules.display(), target.name);
                        coverage_map.entry(f).or_default().push(target_ref);
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse {}: {}",
                    rules_file.display(),
                    e
                );
            }
        }
    }

    // Find uncovered files
    let uncovered_files: HashSet<_> = all_source_files.difference(&covered_files).collect();

    if args.verbose {
        println!("\nCovered files: {}", covered_files.len());
        let mut sorted_covered: Vec<_> = covered_files.iter().collect();
        sorted_covered.sort();
        for f in sorted_covered {
            let rel = f.strip_prefix(&project_root).unwrap_or(f);
            let targets = coverage_map.get(f).map(|v| v.join(", ")).unwrap_or_default();
            println!("  {} -> {}", rel.display(), targets);
        }
    }

    if !uncovered_files.is_empty() {
        println!(
            "\nFound {} uncovered source file(s):\n",
            uncovered_files.len()
        );
        let mut sorted_uncovered: Vec<_> = uncovered_files.iter().collect();
        sorted_uncovered.sort();
        for f in sorted_uncovered {
            let rel = f.strip_prefix(&project_root).unwrap_or(f);
            println!("  - {}", rel.display());
        }

        println!("\nThese files are not included in any rules.star target.");
        println!("Either add them to an existing target or create a new one.");
        println!("\nExample fixes:");
        println!("  1. Add to existing target: srcs = glob([\"src/**/*.rs\"])");
        println!("  2. Add explicitly: srcs = [\"newfile.py\", ...]");
        std::process::exit(1);
    }

    println!(
        "\nAll {} source files are covered by Buck2 targets",
        all_source_files.len()
    );
    Ok(())
}
