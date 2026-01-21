//! Extraction protocol types matching the Go implementation.
//!
//! This module defines the JSON output format that is consumed by
//! the rules sync tooling.

use serde::Serialize;

/// The extraction result containing all packages found.
#[derive(Debug, Serialize)]
pub struct Result {
    /// Protocol version (always "1")
    pub version: String,

    /// Language that was analyzed
    pub language: String,

    /// Packages found in the directory
    pub packages: Vec<Package>,

    /// Errors encountered during extraction
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl Result {
    /// Create a new result for the given language.
    pub fn new(language: &str) -> Self {
        Self {
            version: "1".to_string(),
            language: language.to_string(),
            packages: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Add a package to the result.
    pub fn add_package(&mut self, pkg: Package) {
        self.packages.push(pkg);
    }

    /// Add an error to the result.
    pub fn add_error(&mut self, err: String) {
        self.errors.push(err);
    }
}

/// A package (directory) containing source files.
#[derive(Debug, Serialize)]
pub struct Package {
    /// Relative path from the analysis root
    pub path: String,

    /// Source files in this package
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,

    /// Import dependencies (from non-test files)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<Import>,

    /// Test-only import dependencies
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test_imports: Vec<Import>,
}

impl Package {
    /// Create a new package with the given path.
    pub fn new(path: String) -> Self {
        Self {
            path,
            files: Vec::new(),
            imports: Vec::new(),
            test_imports: Vec::new(),
        }
    }
}

/// An import dependency.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct Import {
    /// The import path/module name
    pub path: String,

    /// Classification of the import
    pub kind: ImportKind,
}

/// Classification of an import.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ImportKind {
    /// Standard library import
    Stdlib,

    /// External/third-party dependency
    External,

    /// Internal/relative import within the project
    Internal,
}
