//! Error types for Nix operations

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum NixError {
    /// Failed to execute the nix binary
    #[error("failed to run nix: {message}")]
    Exec { message: String },

    /// Nix command returned non-zero exit code
    #[error("nix command failed: {message}")]
    Command { message: String },

    /// Failed to parse Nix output
    #[error("failed to parse nix output: {message}")]
    Parse { message: String },

    /// Nix binary not found in PATH
    #[error("nix binary not found in PATH")]
    NotFound,

    /// Flake not found at the specified path
    #[error("no flake found at {0}")]
    NoFlake(PathBuf),
}
