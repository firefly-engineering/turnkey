//! goparse: what a Go source file says about the package it belongs to,
//! read without the go command
//!
//! A Go file's package clause, its imports, its build constraint and its
//! `//go:embed` patterns ([`parse_file`]), the non-test files of a
//! package's directory ([`scan_package`]), and which of them a build
//! includes ([`BuildContext`]): enough to tell a package's imports in each
//! configuration turnkey builds for, as buckgen does for every module of
//! the Go deps cell, and which build tags a tree's files depend on
//! ([`tree_constraint_tags`]), as rules sync does.
//!
//! It reads files as `go/parser` reads them in `ImportsOnly` mode: the
//! package clause and the import declarations are parsed (with
//! `tree-sitter-go`), and a file whose header doesn't parse is an error,
//! whatever the rest of it holds. Its comments are found anywhere in the
//! file as Go's scanner finds them, with a small tokenizer that skips the
//! literals that can hold `//`. Build constraints are parsed with
//! [`gostd::constraint`], Go's `go/build/constraint`.
//!
//! The Rust port of src/go/pkg/goparse (#213), with the same results.

mod build;
mod comments;
mod parse;
mod scan;
mod tags;

pub use build::{BuildContext, parse_filename_constraint};
pub use parse::{Error, parse_file, parse_source};
pub use scan::{DirEntry, Visit, scan_dir, scan_package, tree_constraint_tags, walk_dir};
pub use tags::{config_context, config_tags, tag_dimension};

use gostd::constraint::Expr;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// What a single `.go` file says
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoFile {
    /// The file's path
    pub path: PathBuf,
    /// Its package name
    pub package: String,
    /// Its import paths, in order
    pub imports: Vec<String>,
    /// The patterns of its `//go:embed` directives, in order
    pub embed_patterns: Vec<String>,
    /// Its build constraint: its `//go:build` line, or else all its
    /// `// +build` lines ANDed, before the package clause
    pub constraint: Option<Expr>,
    /// Whether it imports "C"
    pub has_cgo: bool,
    /// Whether it is a test file (`*_test.go`)
    pub is_test: bool,
}

impl GoFile {
    /// The tags its build constraint names, sorted
    pub fn constraint_tags(&self) -> Vec<String> {
        fn walk(e: &Expr, seen: &mut BTreeSet<String>) {
            match e {
                Expr::Tag(t) => {
                    seen.insert(t.clone());
                }
                Expr::Not(x) => walk(x, seen),
                Expr::And(x, y) | Expr::Or(x, y) => {
                    walk(x, seen);
                    walk(y, seen);
                }
            }
        }
        let mut seen = BTreeSet::new();
        if let Some(c) = &self.constraint {
            walk(c, &mut seen);
        }
        seen.into_iter().collect()
    }
}

/// The Go files of a package's directory
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoPackage {
    /// The directory
    pub dir: String,
    /// The package's import path
    pub import_path: String,
    /// The package name, its first file's
    pub name: String,
    /// Its non-test Go files, whatever their constraints, in name order
    pub files: Vec<GoFile>,
    /// Its files' `//go:embed` patterns, sorted, without duplicates
    pub embed_patterns: Vec<String>,
    /// Whether any of its files imports "C"
    pub has_cgo: bool,
}

impl GoPackage {
    /// The imports of the package's files that are part of a build in
    /// `ctx`, sorted and without duplicates, or `None` if no file is: the
    /// package isn't built there
    pub fn imports(&self, ctx: &BuildContext) -> Option<Vec<String>> {
        let mut seen = BTreeSet::new();
        let mut built = false;
        for f in &self.files {
            if !ctx.matches(f) {
                continue;
            }
            built = true;
            seen.extend(f.imports.iter().cloned());
        }
        built.then(|| seen.into_iter().collect())
    }
}
