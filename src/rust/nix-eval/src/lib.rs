//! Nix evaluation and build client
//!
//! Provides a trait-based interface for interacting with Nix:
//! - Evaluate flake attributes
//! - Build derivations and get store paths
//! - Query available packages
//!
//! # Implementation
//!
//! The current implementation shells out to the `nix` CLI binary.
//! This is an intentional abstraction boundary — when a stable Nix client
//! library becomes available (e.g., via tvix, libnixc, or direct daemon
//! protocol), only this crate needs to change.
//!
//! # Example
//!
//! ```ignore
//! use nix_eval::NixClient;
//!
//! let client = nix_eval::cli::CliNixClient::new("/path/to/repo");
//! let packages = client.list_packages("aarch64-darwin")?;
//! let paths = client.build(&["godeps-cell", "rustdeps-cell"])?;
//! ```

mod error;
mod cli;

pub use error::NixError;
pub use cli::CliNixClient;

use std::collections::HashMap;
use std::path::PathBuf;

/// Trait for Nix evaluation and build operations.
///
/// This is the abstraction boundary that isolates the rest of the codebase
/// from the Nix implementation. Currently backed by CLI invocations;
/// could be replaced with direct daemon protocol, tvix, or libnixc.
pub trait NixClient: Send + Sync {
    /// List package attribute names available in the flake for the given system.
    fn list_packages(&self, system: &str) -> Result<Vec<String>, NixError>;

    /// Build one or more flake packages and return their Nix store paths.
    ///
    /// The `packages` argument contains package attribute names (e.g., `"godeps-cell"`).
    /// Returns a map of package name → store path.
    fn build(&self, packages: &[&str]) -> Result<HashMap<String, PathBuf>, NixError>;

    /// Evaluate a Nix expression and return the JSON result.
    fn eval_json(&self, expr: &str) -> Result<serde_json::Value, NixError>;
}

/// Get the Nix system string for the current platform.
pub fn current_system() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "aarch64-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "x86_64-darwin" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "x86_64-linux" }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "aarch64-linux" }
}
