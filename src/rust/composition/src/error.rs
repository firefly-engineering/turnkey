//! Error types for composition operations

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during composition operations
#[derive(Error, Debug)]
pub enum Error {
    /// The backend is already mounted
    #[error("backend is already mounted at {}", .0.display())]
    AlreadyMounted(PathBuf),

    /// The backend is not mounted
    #[error("backend is not mounted")]
    NotMounted,

    /// Mount point does not exist or is inaccessible
    #[error("mount point not accessible: {}", path.display())]
    MountPointInaccessible {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to create mount point directory
    #[error("failed to create mount point: {}", path.display())]
    MountPointCreationFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Cell not found in configuration
    #[error("cell not found: {0}")]
    CellNotFound(String),

    /// Cell source path does not exist
    #[error("cell source path does not exist: {cell} -> {}", path.display())]
    CellSourceNotFound { cell: String, path: PathBuf },

    /// Failed to create symlink
    #[error("failed to create symlink: {} -> {}", target.display(), link.display())]
    SymlinkFailed {
        target: PathBuf,
        link: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to remove symlink
    #[error("failed to remove symlink: {}", path.display())]
    SymlinkRemoveFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// FUSE is not available on this system
    #[error("FUSE is not available: {0}")]
    FuseUnavailable(String),

    /// FUSE mount failed
    #[error("FUSE mount failed: {0}")]
    FuseMountFailed(String),

    /// FUSE unmount failed
    #[error("FUSE unmount failed: {0}")]
    FuseUnmountFailed(String),

    /// Refresh failed while backend was in an invalid state
    #[error("cannot refresh: backend is in state {0}")]
    RefreshInvalidState(String),

    /// Nix build failed during refresh
    #[error("nix build failed for cell {cell}")]
    NixBuildFailed {
        cell: String,
        #[source]
        source: std::io::Error,
    },

    /// Configuration error
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Path is being updated, read blocked
    #[error("path is being updated: {}", .0.display())]
    PathUpdating(PathBuf),

    /// Operation timed out
    #[error("operation timed out after {:?}", .0)]
    Timeout(std::time::Duration),

    /// Invalid state transition
    #[error("invalid state transition: {0}")]
    StateTransitionError(String),
}
