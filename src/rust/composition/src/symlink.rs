//! Symlink-based composition backend
//!
//! This module provides the `SymlinkBackend` struct that implements the
//! `CompositionBackend` trait using symlinks to Nix store paths.
//!
//! This is the fallback backend for CI environments and systems without FUSE.
//!
//! # Directory Structure
//!
//! ```text
//! .turnkey/                    # Composition root (configurable)
//! ├── godeps -> /nix/store/xxx-godeps-cell
//! ├── rustdeps -> /nix/store/xxx-rustdeps-cell
//! ├── pydeps -> /nix/store/xxx-pydeps-cell
//! └── ...
//! ```

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use log::{debug, info};

use crate::{
    BackendStatus, CellMapping, CompositionBackend, CompositionConfig, Error, Result,
};

/// Symlink-based composition backend
///
/// This backend creates symlinks in a `.turnkey/` directory (or other configured
/// location) pointing to Nix store paths for each dependency cell.
///
/// # Lifecycle
///
/// 1. Create with `SymlinkBackend::new(config)`
/// 2. Call `mount()` to create the symlink directory and links
/// 3. Cell paths are available immediately
/// 4. Call `unmount()` to remove the symlinks (optional)
///
/// Unlike the FUSE backend, the symlink backend doesn't require a running daemon.
pub struct SymlinkBackend {
    /// Configuration for this backend
    config: CompositionConfig,
    /// Current status
    status: BackendStatus,
    /// Whether the symlinks have been created
    mounted: bool,
}

impl SymlinkBackend {
    /// Create a new symlink backend with the given configuration
    pub fn new(config: CompositionConfig) -> Self {
        Self {
            config,
            status: BackendStatus::Stopped,
            mounted: false,
        }
    }

    /// Ensure the mount point directory exists
    fn ensure_mount_point(&self) -> Result<()> {
        let mount_point = &self.config.mount_point;
        if !mount_point.exists() {
            fs::create_dir_all(mount_point).map_err(|e| Error::MountPointCreationFailed {
                path: mount_point.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    /// Create a symlink, removing any existing file/link at the target
    fn create_symlink(&self, target: &PathBuf, link: &PathBuf) -> Result<()> {
        // Remove existing symlink or file if present
        if link.exists() || link.is_symlink() {
            fs::remove_file(link).map_err(|e| Error::SymlinkRemoveFailed {
                path: link.clone(),
                source: e,
            })?;
        }

        // Create the symlink
        symlink(target, link).map_err(|e| Error::SymlinkFailed {
            target: target.clone(),
            link: link.clone(),
            source: e,
        })?;

        debug!("Created symlink: {:?} -> {:?}", link, target);
        Ok(())
    }

    /// Remove a symlink if it exists
    fn remove_symlink(&self, link: &PathBuf) -> Result<()> {
        if link.is_symlink() {
            fs::remove_file(link).map_err(|e| Error::SymlinkRemoveFailed {
                path: link.clone(),
                source: e,
            })?;
            debug!("Removed symlink: {:?}", link);
        }
        Ok(())
    }
}

impl CompositionBackend for SymlinkBackend {
    fn config(&self) -> &CompositionConfig {
        &self.config
    }

    fn mount(&mut self) -> Result<()> {
        if self.mounted {
            return Err(Error::AlreadyMounted(self.config.mount_point.clone()));
        }

        info!("Setting up symlink composition at {:?}", self.config.mount_point);

        // Create mount point directory
        self.ensure_mount_point()?;

        // Create symlinks for each cell
        for cell in &self.config.cells {
            let link_path = self.config.mount_point.join(&cell.name);

            // Verify source path exists
            if !cell.source_path.exists() {
                return Err(Error::CellSourceNotFound {
                    cell: cell.name.clone(),
                    path: cell.source_path.clone(),
                });
            }

            self.create_symlink(&cell.source_path, &link_path)?;
        }

        self.mounted = true;
        self.status = BackendStatus::Ready;

        info!(
            "Symlink composition ready with {} cells",
            self.config.cells.len()
        );

        Ok(())
    }

    fn unmount(&mut self) -> Result<()> {
        if !self.mounted {
            return Err(Error::NotMounted);
        }

        info!("Removing symlink composition at {:?}", self.config.mount_point);

        // Remove symlinks for each cell
        for cell in &self.config.cells {
            let link_path = self.config.mount_point.join(&cell.name);
            self.remove_symlink(&link_path)?;
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
            return Err(Error::NotMounted);
        }

        info!("Refreshing symlink composition");

        // Re-create all symlinks to pick up any changes
        for cell in &self.config.cells {
            let link_path = self.config.mount_point.join(&cell.name);

            // Verify source path exists
            if !cell.source_path.exists() {
                return Err(Error::CellSourceNotFound {
                    cell: cell.name.clone(),
                    path: cell.source_path.clone(),
                });
            }

            self.create_symlink(&cell.source_path, &link_path)?;
        }

        Ok(())
    }

    fn cell_path(&self, cell_name: &str) -> Option<PathBuf> {
        self.config
            .cells
            .iter()
            .find(|c| c.name == cell_name)
            .map(|_| self.config.mount_point.join(cell_name))
    }

    fn cell_mappings(&self) -> Vec<CellMapping> {
        self.config
            .cells
            .iter()
            .map(|c| CellMapping::new(c.name.clone(), self.config.mount_point.join(&c.name)))
            .collect()
    }

    fn wait_ready(&self, _timeout: Option<std::time::Duration>) -> Result<()> {
        // Symlink backend is always immediately ready after mount
        if self.mounted {
            Ok(())
        } else {
            Err(Error::NotMounted)
        }
    }
}

impl Drop for SymlinkBackend {
    fn drop(&mut self) {
        // Don't automatically unmount on drop - symlinks should persist
        // for the build system to use even after the backend is dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;
    use std::fs::File;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, TempDir, PathBuf, PathBuf) {
        let mount_dir = TempDir::new().unwrap();
        let source_dir = TempDir::new().unwrap();

        // Create a fake cell source directory
        let cell_source = source_dir.path().join("godeps");
        fs::create_dir_all(&cell_source).unwrap();
        File::create(cell_source.join("test.txt")).unwrap();

        let mount_point = mount_dir.path().join(".turnkey");

        (mount_dir, source_dir, mount_point, cell_source)
    }

    #[test]
    fn test_symlink_backend_new() {
        let config = CompositionConfig::new("/tmp/test", "/tmp/repo");
        let backend = SymlinkBackend::new(config);

        assert!(backend.status().is_stopped());
        assert!(!backend.is_mounted());
    }

    #[test]
    fn test_symlink_mount_unmount() {
        let (_mount_dir, _source_dir, mount_point, cell_source) = setup_test_env();

        let config = CompositionConfig::new(&mount_point, "/tmp/repo")
            .with_cell(CellConfig::new("godeps", &cell_source));

        let mut backend = SymlinkBackend::new(config);

        // Mount
        backend.mount().unwrap();
        assert!(backend.is_mounted());
        assert!(backend.status().is_ready());

        // Check symlink was created
        let link_path = mount_point.join("godeps");
        assert!(link_path.is_symlink());
        assert_eq!(fs::read_link(&link_path).unwrap(), cell_source);

        // Unmount
        backend.unmount().unwrap();
        assert!(!backend.is_mounted());
        assert!(backend.status().is_stopped());

        // Check symlink was removed
        assert!(!link_path.exists());
    }

    #[test]
    fn test_symlink_already_mounted() {
        let (_mount_dir, _source_dir, mount_point, cell_source) = setup_test_env();

        let config = CompositionConfig::new(&mount_point, "/tmp/repo")
            .with_cell(CellConfig::new("godeps", &cell_source));

        let mut backend = SymlinkBackend::new(config);

        backend.mount().unwrap();
        let result = backend.mount();
        assert!(result.is_err());
    }

    #[test]
    fn test_symlink_cell_path() {
        let (_mount_dir, _source_dir, mount_point, cell_source) = setup_test_env();

        let config = CompositionConfig::new(&mount_point, "/tmp/repo")
            .with_cell(CellConfig::new("godeps", &cell_source))
            .with_cell(CellConfig::new("rustdeps", &cell_source));

        let backend = SymlinkBackend::new(config);

        assert_eq!(
            backend.cell_path("godeps"),
            Some(mount_point.join("godeps"))
        );
        assert_eq!(
            backend.cell_path("rustdeps"),
            Some(mount_point.join("rustdeps"))
        );
        assert_eq!(backend.cell_path("unknown"), None);
    }

    #[test]
    fn test_symlink_cell_mappings() {
        let (_mount_dir, _source_dir, mount_point, cell_source) = setup_test_env();

        let config = CompositionConfig::new(&mount_point, "/tmp/repo")
            .with_cell(CellConfig::new("godeps", &cell_source));

        let backend = SymlinkBackend::new(config);
        let mappings = backend.cell_mappings();

        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].name, "godeps");
        assert_eq!(mappings[0].path, mount_point.join("godeps"));
    }

    #[test]
    fn test_symlink_refresh() {
        let (_mount_dir, _source_dir, mount_point, cell_source) = setup_test_env();

        let config = CompositionConfig::new(&mount_point, "/tmp/repo")
            .with_cell(CellConfig::new("godeps", &cell_source));

        let mut backend = SymlinkBackend::new(config);
        backend.mount().unwrap();

        // Refresh should succeed and recreate symlinks
        backend.refresh().unwrap();

        let link_path = mount_point.join("godeps");
        assert!(link_path.is_symlink());
    }

    #[test]
    fn test_symlink_source_not_found() {
        let mount_dir = TempDir::new().unwrap();
        let mount_point = mount_dir.path().join(".turnkey");

        let config = CompositionConfig::new(&mount_point, "/tmp/repo")
            .with_cell(CellConfig::new("godeps", "/nonexistent/path"));

        let mut backend = SymlinkBackend::new(config);
        let result = backend.mount();

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::CellSourceNotFound { cell, .. } => {
                assert_eq!(cell, "godeps");
            }
            e => panic!("Expected CellSourceNotFound, got {:?}", e),
        }
    }
}
