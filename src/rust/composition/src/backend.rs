//! The CompositionBackend trait definition

use crate::{BackendStatus, CellMapping, CompositionConfig, Result};
use std::path::PathBuf;

/// A backend for composing repository views
///
/// This trait abstracts the mechanism used to present dependency cells
/// to build systems. Implementations include:
///
/// - **SymlinkBackend**: Uses symlinks in `.turnkey/` to point to Nix store paths.
///   This is the fallback for CI environments and systems without FUSE.
///
/// - **FuseBackend**: Uses a FUSE filesystem to present a unified view at a
///   fixed mount point. This enables remote caching and transparent editing.
///
/// # Lifecycle
///
/// ```text
/// new(config) ──► mount() ──► [ready for use] ──► unmount()
///                    │              │
///                    │         refresh()
///                    │              │
///                    └──────────────┘
/// ```
///
/// # Thread Safety
///
/// Implementations should be thread-safe. The FUSE backend in particular
/// will handle concurrent filesystem operations from multiple processes.
pub trait CompositionBackend: Send + Sync {
    /// Get the configuration for this backend
    fn config(&self) -> &CompositionConfig;

    /// Mount the composition view
    ///
    /// For symlink backend: Creates the `.turnkey/` directory structure
    /// with symlinks to Nix store paths.
    ///
    /// For FUSE backend: Mounts the FUSE filesystem at the configured
    /// mount point.
    ///
    /// # Errors
    ///
    /// Returns `Error::AlreadyMounted` if the backend is already mounted.
    /// Returns `Error::MountPointCreationFailed` if the mount point cannot be created.
    /// Returns `Error::FuseMountFailed` for FUSE-specific mount errors.
    fn mount(&mut self) -> Result<()>;

    /// Unmount the composition view
    ///
    /// For symlink backend: Removes the `.turnkey/` directory structure.
    ///
    /// For FUSE backend: Unmounts the FUSE filesystem.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotMounted` if the backend is not currently mounted.
    /// Returns `Error::FuseUnmountFailed` for FUSE-specific unmount errors.
    fn unmount(&mut self) -> Result<()>;

    /// Get the current status of the backend
    ///
    /// This method is cheap to call and can be used for polling.
    fn status(&self) -> BackendStatus;

    /// Refresh the composition view
    ///
    /// This should be called when dependency manifests change. The backend
    /// will rebuild affected cells and update the composition view.
    ///
    /// For symlink backend: Re-evaluates Nix expressions and updates symlinks.
    ///
    /// For FUSE backend: Triggers Nix builds and atomically switches to
    /// new derivation outputs.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotMounted` if the backend is not mounted.
    /// Returns `Error::RefreshInvalidState` if refresh is called during an update.
    /// Returns `Error::NixBuildFailed` if a Nix build fails.
    fn refresh(&mut self) -> Result<()>;

    /// Get the filesystem path for a cell
    ///
    /// Returns the path where the cell is available in the composition view.
    ///
    /// # Arguments
    ///
    /// * `cell_name` - The name of the cell (e.g., "godeps", "rustdeps")
    ///
    /// # Returns
    ///
    /// The filesystem path if the cell exists, or `None` if not found.
    fn cell_path(&self, cell_name: &str) -> Option<PathBuf>;

    /// Get all cell mappings
    ///
    /// Returns a list of all cells and their filesystem paths.
    fn cell_mappings(&self) -> Vec<CellMapping>;

    /// Check if the backend is currently mounted
    fn is_mounted(&self) -> bool {
        !matches!(self.status(), BackendStatus::Stopped)
    }

    /// Check if the backend is ready for file operations
    fn is_ready(&self) -> bool {
        matches!(self.status(), BackendStatus::Ready)
    }

    /// Wait for the backend to become ready
    ///
    /// Blocks until the backend reaches the `Ready` state or an error occurs.
    /// This is useful after calling `mount()` or `refresh()` to ensure the
    /// composition view is consistent.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum time to wait. If `None`, waits indefinitely.
    ///
    /// # Errors
    ///
    /// Returns `Error::Timeout` if the timeout is reached.
    /// Returns the underlying error if the backend enters an error state.
    fn wait_ready(&self, timeout: Option<std::time::Duration>) -> Result<()>;

    /// Get the path to the src/ directory in the composition view
    ///
    /// This returns the path where repository source files are accessible.
    /// For FUSE backend, this is inside the mount point.
    /// For symlink backend, this might be a pass-through to the repo root.
    fn src_path(&self) -> PathBuf {
        self.config().mount_point.join("src")
    }

    /// Get the path to the external/ directory in the composition view
    ///
    /// This returns the path where external dependencies are accessible.
    fn external_path(&self) -> PathBuf {
        self.config().mount_point.join("external")
    }
}

/// Extension trait for backend-specific operations
///
/// Some operations are only available on specific backends. This trait
/// provides a way to access them through dynamic dispatch.
pub trait CompositionBackendExt: CompositionBackend {
    /// Get the backend type name (e.g., "symlink", "fuse")
    fn backend_type(&self) -> &'static str;

    /// Check if this backend supports editing external dependencies
    fn supports_editing(&self) -> bool {
        false
    }

    /// Enable edit mode for a specific cell
    ///
    /// Only available on backends that support editing (FUSE with edit layer).
    ///
    /// # Arguments
    ///
    /// * `cell_name` - The cell to enable editing for
    ///
    /// # Errors
    ///
    /// Returns an error if editing is not supported or the cell doesn't exist.
    fn enable_editing(&mut self, _cell_name: &str) -> Result<()> {
        Err(crate::Error::ConfigError(
            "editing not supported by this backend".into(),
        ))
    }

    /// Generate patches from edited files
    ///
    /// Only available on backends that support editing.
    ///
    /// # Returns
    ///
    /// A list of (cell_name, patch_path) pairs for generated patches.
    fn generate_patches(&self) -> Result<Vec<(String, PathBuf)>> {
        Err(crate::Error::ConfigError(
            "editing not supported by this backend".into(),
        ))
    }

    /// Discard edits for a cell
    ///
    /// Only available on backends that support editing.
    fn discard_edits(&mut self, _cell_name: &str) -> Result<()> {
        Err(crate::Error::ConfigError(
            "editing not supported by this backend".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;

    /// A mock backend for testing
    struct MockBackend {
        config: CompositionConfig,
        status: BackendStatus,
        mounted: bool,
    }

    impl MockBackend {
        fn new(config: CompositionConfig) -> Self {
            Self {
                config,
                status: BackendStatus::Stopped,
                mounted: false,
            }
        }
    }

    impl CompositionBackend for MockBackend {
        fn config(&self) -> &CompositionConfig {
            &self.config
        }

        fn mount(&mut self) -> Result<()> {
            if self.mounted {
                return Err(crate::Error::AlreadyMounted(
                    self.config.mount_point.clone(),
                ));
            }
            self.mounted = true;
            self.status = BackendStatus::Ready;
            Ok(())
        }

        fn unmount(&mut self) -> Result<()> {
            if !self.mounted {
                return Err(crate::Error::NotMounted);
            }
            self.mounted = false;
            self.status = BackendStatus::Stopped;
            Ok(())
        }

        fn status(&self) -> BackendStatus {
            self.status.clone()
        }

        fn refresh(&mut self) -> Result<()> {
            if !self.mounted {
                return Err(crate::Error::NotMounted);
            }
            Ok(())
        }

        fn cell_path(&self, cell_name: &str) -> Option<PathBuf> {
            self.config
                .cells
                .iter()
                .find(|c| c.name == cell_name)
                .map(|c| self.config.mount_point.join("external").join(&c.name))
        }

        fn cell_mappings(&self) -> Vec<CellMapping> {
            self.config
                .cells
                .iter()
                .map(|c| {
                    CellMapping::new(
                        c.name.clone(),
                        self.config.mount_point.join("external").join(&c.name),
                    )
                })
                .collect()
        }

        fn wait_ready(&self, _timeout: Option<std::time::Duration>) -> Result<()> {
            if self.status.is_ready() {
                Ok(())
            } else if self.status.is_error() {
                Err(crate::Error::ConfigError("backend in error state".into()))
            } else {
                Ok(()) // Mock: instant ready
            }
        }
    }

    #[test]
    fn test_mock_backend_lifecycle() {
        let config = CompositionConfig::new("/mount", "/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));

        let mut backend = MockBackend::new(config);

        // Initial state
        assert!(backend.status().is_stopped());
        assert!(!backend.is_mounted());

        // Mount
        backend.mount().unwrap();
        assert!(backend.status().is_ready());
        assert!(backend.is_mounted());

        // Double mount should fail
        assert!(backend.mount().is_err());

        // Cell path
        let path = backend.cell_path("godeps").unwrap();
        assert_eq!(path, PathBuf::from("/mount/external/godeps"));

        // Unknown cell
        assert!(backend.cell_path("unknown").is_none());

        // Unmount
        backend.unmount().unwrap();
        assert!(backend.status().is_stopped());
        assert!(!backend.is_mounted());

        // Double unmount should fail
        assert!(backend.unmount().is_err());
    }

    #[test]
    fn test_cell_mappings() {
        let config = CompositionConfig::new("/mount", "/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));

        let mut backend = MockBackend::new(config);
        backend.mount().unwrap();

        let mappings = backend.cell_mappings();
        assert_eq!(mappings.len(), 2);
        assert!(mappings.iter().any(|m| m.name == "godeps"));
        assert!(mappings.iter().any(|m| m.name == "rustdeps"));
    }

    #[test]
    fn test_default_paths() {
        let config = CompositionConfig::new("/firefly/turnkey", "/repo");
        let backend = MockBackend::new(config);

        assert_eq!(backend.src_path(), PathBuf::from("/firefly/turnkey/src"));
        assert_eq!(
            backend.external_path(),
            PathBuf::from("/firefly/turnkey/external")
        );
    }
}
