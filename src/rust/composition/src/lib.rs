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
pub mod layout;
pub mod performance;
pub mod policy;
pub mod recovery;
pub mod selector;
pub mod state;
mod status;
pub mod symlink;
pub mod tracing;

#[cfg(feature = "fuse")]
pub mod fuse;

#[cfg(feature = "watcher")]
pub mod watcher;

pub use backend::CompositionBackend;
pub use symlink::SymlinkBackend;
pub use config::{CellConfig, CompositionConfig, ConsistencyMode};
pub use error::Error;
pub use policy::{
    AccessPolicy, CIPolicy, DevelopmentPolicy, FileClass, LenientPolicy, OperationType,
    PolicyDecision, StrictPolicy, SystemState,
};
pub use state::{CellUpdate, ConsistencyStateMachine, StateObserver};
pub use status::BackendStatus;
pub use layout::{
    available_layouts, default_layout, global_registry, layout_by_name, BazelLayout, BoxedLayout,
    Buck2Layout, CellInfo, ConfigFile, Layout, LayoutContext, LayoutFactory, LayoutRegistry,
    SimpleLayout,
};
pub use selector::{
    create_backend, fuse_install_instructions, is_fuse_available, select_backend,
    BackendSelection, BackendType,
};
pub use recovery::{
    is_transient_error, recovery_suggestion, retry_with_backoff, DaemonRecovery, RecoveryAction,
    RetryConfig,
};
pub use tracing::{DebugInfo, FuseTracer, Metrics, StateLogger, TracingConfig};
pub use performance::{CacheConfig, CacheStats, DirEntry, DirEntryType, InodeCache, OptimizedReaddir};

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
