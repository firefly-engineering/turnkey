//! FUSE backend implementation
//!
//! This module provides the `FuseBackend` struct that implements the
//! `CompositionBackend` trait using a FUSE filesystem.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use fuser::{MountOption, Session};
use log::{debug, error, info};

use super::filesystem::CompositionFs;
use crate::{
    BackendStatus, CellMapping, CompositionBackend, CompositionConfig, Error, Result,
};

/// FUSE-based composition backend for Linux
///
/// This backend mounts a FUSE filesystem that presents a unified view of
/// the repository with dependency cells.
///
/// # Lifecycle
///
/// 1. Create with `FuseBackend::new(config)`
/// 2. Call `mount()` to start the FUSE filesystem
/// 3. Filesystem is available at the configured mount point
/// 4. Call `unmount()` to stop the filesystem
///
/// # Thread Safety
///
/// The FUSE session runs in a background thread. All methods are thread-safe.
pub struct FuseBackend {
    /// Configuration for this backend
    config: CompositionConfig,

    /// Current status
    status: Arc<Mutex<BackendStatus>>,

    /// Flag to signal the FUSE thread to stop
    should_stop: Arc<AtomicBool>,

    /// Handle to the FUSE thread
    fuse_thread: Option<JoinHandle<()>>,

    /// FUSE session (for unmounting)
    session: Arc<Mutex<Option<Session<CompositionFs>>>>,
}

impl FuseBackend {
    /// Create a new FUSE backend with the given configuration
    pub fn new(config: CompositionConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(BackendStatus::Stopped)),
            should_stop: Arc::new(AtomicBool::new(false)),
            fuse_thread: None,
            session: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the repository root path
    fn repo_root(&self) -> PathBuf {
        self.config.repo_root.clone()
    }

    /// Create the mount point directory if it doesn't exist
    fn ensure_mount_point(&self) -> Result<()> {
        let mount_point = &self.config.mount_point;
        if !mount_point.exists() {
            std::fs::create_dir_all(mount_point).map_err(|e| Error::MountPointCreationFailed {
                path: mount_point.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    /// Check if the mount point is already mounted
    fn is_mount_point_busy(&self) -> bool {
        // Check if the mount point is already a mount point
        // by comparing device IDs with parent
        let mount_point = &self.config.mount_point;
        if let (Ok(mp_meta), Ok(parent_meta)) = (
            std::fs::metadata(mount_point),
            mount_point
                .parent()
                .and_then(|p| std::fs::metadata(p).ok())
                .ok_or(()),
        ) {
            use std::os::unix::fs::MetadataExt;
            // Different device means it's a mount point
            mp_meta.dev() != parent_meta.dev()
        } else {
            false
        }
    }
}

impl CompositionBackend for FuseBackend {
    fn config(&self) -> &CompositionConfig {
        &self.config
    }

    fn mount(&mut self) -> Result<()> {
        // Check if already mounted
        if self.is_mounted() {
            return Err(Error::AlreadyMounted(self.config.mount_point.clone()));
        }

        // Check if mount point is busy
        if self.is_mount_point_busy() {
            return Err(Error::AlreadyMounted(self.config.mount_point.clone()));
        }

        // Ensure mount point exists
        self.ensure_mount_point()?;

        // Reset stop flag
        self.should_stop.store(false, Ordering::SeqCst);

        // Update status to building (mounting phase)
        {
            let mut status = self.status.lock().unwrap();
            *status = BackendStatus::Building {
                affected_paths: vec![self.config.mount_point.clone()],
                message: Some("mounting FUSE filesystem".into()),
            };
        }

        // Create the filesystem
        let fs = CompositionFs::new(self.config.clone(), self.repo_root());
        let mount_point = self.config.mount_point.clone();

        // Mount options
        let options = vec![
            MountOption::FSName("turnkey".to_string()),
            MountOption::AutoUnmount,
            MountOption::AllowOther,
            MountOption::RO, // Read-only for now
        ];

        // Create FUSE session
        info!("Mounting FUSE filesystem at {:?}", mount_point);
        let session = Session::new(fs, &mount_point, &options)
            .map_err(|e| Error::FuseMountFailed(e.to_string()))?;

        // Store session for later unmounting
        {
            let mut session_lock = self.session.lock().unwrap();
            *session_lock = Some(session);
        }

        // Start the FUSE thread
        let status = Arc::clone(&self.status);
        let should_stop = Arc::clone(&self.should_stop);
        let session = Arc::clone(&self.session);

        let handle = thread::spawn(move || {
            // Update status to ready
            {
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Ready;
            }

            // Run the session
            // Note: We need to run the session in a loop until should_stop is set
            loop {
                if should_stop.load(Ordering::SeqCst) {
                    debug!("FUSE thread received stop signal");
                    break;
                }

                // Check if session is still valid
                let session_guard = session.lock().unwrap();
                if session_guard.is_none() {
                    debug!("FUSE session is gone, stopping thread");
                    break;
                }
                drop(session_guard);

                // Sleep briefly to avoid busy-waiting
                // In a real implementation, we'd use the session.run() method
                thread::sleep(Duration::from_millis(100));
            }

            // Update status to stopped
            {
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Stopped;
            }
        });

        self.fuse_thread = Some(handle);

        Ok(())
    }

    fn unmount(&mut self) -> Result<()> {
        if !self.is_mounted() {
            return Err(Error::NotMounted);
        }

        info!("Unmounting FUSE filesystem at {:?}", self.config.mount_point);

        // Signal the thread to stop
        self.should_stop.store(true, Ordering::SeqCst);

        // Drop the session to unmount
        {
            let mut session = self.session.lock().unwrap();
            *session = None;
        }

        // Wait for the thread to finish
        if let Some(handle) = self.fuse_thread.take() {
            handle
                .join()
                .map_err(|_| Error::FuseUnmountFailed("thread join failed".into()))?;
        }

        // Update status
        {
            let mut status = self.status.lock().unwrap();
            *status = BackendStatus::Stopped;
        }

        Ok(())
    }

    fn status(&self) -> BackendStatus {
        self.status.lock().unwrap().clone()
    }

    fn refresh(&mut self) -> Result<()> {
        if !self.is_mounted() {
            return Err(Error::NotMounted);
        }

        // For now, refresh is a no-op
        // In the full implementation, this would:
        // 1. Re-read cell configurations
        // 2. Trigger Nix rebuilds if needed
        // 3. Update the filesystem view
        info!("Refresh requested (currently a no-op)");
        Ok(())
    }

    fn cell_path(&self, cell_name: &str) -> Option<PathBuf> {
        self.config
            .cells
            .iter()
            .find(|c| c.name == cell_name)
            .map(|_| self.config.mount_point.join("external").join(cell_name))
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

    fn wait_ready(&self, timeout: Option<Duration>) -> Result<()> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(10);

        loop {
            let status = self.status();

            if status.is_ready() {
                return Ok(());
            }

            if status.is_error() {
                return Err(Error::ConfigError("backend in error state".into()));
            }

            if let Some(timeout) = timeout {
                if start.elapsed() >= timeout {
                    return Err(Error::Timeout("wait_ready timed out".into()));
                }
            }

            thread::sleep(poll_interval);
        }
    }
}

impl Drop for FuseBackend {
    fn drop(&mut self) {
        if self.is_mounted() {
            if let Err(e) = self.unmount() {
                error!("Failed to unmount FUSE filesystem on drop: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;

    #[test]
    fn test_fuse_backend_new() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let backend = FuseBackend::new(config);

        assert!(backend.status().is_stopped());
        assert!(!backend.is_mounted());
    }

    #[test]
    fn test_cell_path() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));
        let backend = FuseBackend::new(config);

        assert_eq!(
            backend.cell_path("godeps"),
            Some(PathBuf::from("/firefly/turnkey/external/godeps"))
        );
        assert_eq!(
            backend.cell_path("rustdeps"),
            Some(PathBuf::from("/firefly/turnkey/external/rustdeps"))
        );
        assert_eq!(backend.cell_path("unknown"), None);
    }

    #[test]
    fn test_cell_mappings() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let backend = FuseBackend::new(config);

        let mappings = backend.cell_mappings();
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].name, "godeps");
        assert_eq!(
            mappings[0].path,
            PathBuf::from("/firefly/turnkey/external/godeps")
        );
    }
}
