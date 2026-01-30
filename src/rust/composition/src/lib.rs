//! Composition backend trait for repository views
//!
//! This crate defines the `CompositionBackend` trait that abstracts how
//! dependency cells are composed and presented to build systems. It supports
//! two primary backends:
//!
//! - **Symlink backend**: The current approach using `.turnkey/` with symlinks
//!   to Nix store paths. Used as fallback for CI and environments without FUSE.
//!
//! - **FUSE backend**: A FUSE-based filesystem that provides a unified view
//!   at a fixed mount point, enabling remote caching and transparent dependency
//!   editing.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    CompositionBackend trait                      │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────┐       ┌─────────────────────┐         │
//! │  │   FUSE Backend      │       │   Symlink Backend   │         │
//! │  │   (Development)     │       │   (CI / Fallback)   │         │
//! │  └─────────────────────┘       └─────────────────────┘         │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use composition::{CompositionBackend, CompositionConfig, BackendStatus};
//!
//! fn setup_composition(backend: &mut dyn CompositionBackend) -> Result<(), composition::Error> {
//!     backend.mount()?;
//!
//!     match backend.status() {
//!         BackendStatus::Ready => println!("Composition ready"),
//!         BackendStatus::Building { .. } => println!("Building dependencies..."),
//!         _ => {}
//!     }
//!
//!     // Get cell paths
//!     if let Some(path) = backend.cell_path("godeps") {
//!         println!("Go deps at: {}", path.display());
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::path::PathBuf;

mod backend;
mod config;
mod error;
mod status;

pub use backend::CompositionBackend;
pub use config::{CellConfig, CompositionConfig, ConsistencyMode};
pub use error::Error;
pub use status::BackendStatus;

/// Result type for composition operations
pub type Result<T> = std::result::Result<T, Error>;

/// A cell mapping entry (cell name -> filesystem path)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellMapping {
    /// The cell name (e.g., "godeps", "rustdeps")
    pub name: String,
    /// The filesystem path where the cell is mounted/symlinked
    pub path: PathBuf,
}

impl CellMapping {
    /// Create a new cell mapping
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_mapping() {
        let mapping = CellMapping::new("godeps", "/firefly/turnkey/external/godeps");
        assert_eq!(mapping.name, "godeps");
        assert_eq!(
            mapping.path,
            PathBuf::from("/firefly/turnkey/external/godeps")
        );
    }
}
