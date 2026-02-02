//! FUSE backend implementation
//!
//! This module provides the `FuseBackend` struct that implements the
//! `CompositionBackend` trait using a FUSE filesystem.
//!
//! # Platform Support
//!
//! - **Linux**: Native FUSE support
//! - **macOS**: FUSE-T support (NFS-based, no kernel extension)

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use fuser::{MountOption, Session};
use log::{debug, error, info};

use super::filesystem::CompositionFs;
use super::platform::{self, Platform};
use crate::state::ConsistencyStateMachine;
use crate::{BackendStatus, CellMapping, CompositionBackend, CompositionConfig, Error, Result};

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
/// The state machine is shared between the backend and filesystem for
/// coordinating consistency during updates.
pub struct FuseBackend {
    /// Configuration for this backend
    config: CompositionConfig,

    /// Current status
    status: Arc<Mutex<BackendStatus>>,

    /// State machine for consistency during updates
    state_machine: Arc<ConsistencyStateMachine>,

    /// Flag to signal the FUSE thread to stop
    should_stop: Arc<AtomicBool>,

    /// Handle to the FUSE thread
    fuse_thread: Option<JoinHandle<()>>,
}

impl FuseBackend {
    /// Create a new FUSE backend with the given configuration
    pub fn new(config: CompositionConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(BackendStatus::Stopped)),
            state_machine: Arc::new(ConsistencyStateMachine::new()),
            should_stop: Arc::new(AtomicBool::new(false)),
            fuse_thread: None,
        }
    }

    /// Get a reference to the state machine
    ///
    /// This is useful for external code that needs to trigger updates
    /// or check the current state.
    pub fn state_machine(&self) -> &Arc<ConsistencyStateMachine> {
        &self.state_machine
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

        let mount_point = self.config.mount_point.clone();
        let config = self.config.clone();
        let repo_root = self.repo_root();

        info!("Mounting FUSE filesystem at {:?}", mount_point);

        // Start the FUSE thread - create session and run it in the thread
        let status = Arc::clone(&self.status);
        let should_stop = Arc::clone(&self.should_stop);
        let state_machine = Arc::clone(&self.state_machine);

        let handle = thread::spawn(move || {
            // Create mount options - try minimal first (allow_other requires system config)
            let options = vec![
                MountOption::FSName("turnkey".to_string()),
                MountOption::RO,
            ];

            // Create the filesystem and session in this thread
            let fs = CompositionFs::new(config, repo_root, state_machine);
            let mut session = match Session::new(fs, &mount_point, &options) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create FUSE session: {}", e);
                    let mut s = status.lock().unwrap();
                    *s = BackendStatus::Error {
                        message: format!("FUSE mount failed: {}", e),
                        recoverable: false,
                    };
                    return;
                }
            };

            // Update status to ready
            {
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Ready;
            }

            debug!("Starting FUSE session event loop");

            // Run the FUSE session - this blocks until unmounted
            // The session will exit when fusermount -u is called or the mount is unmounted
            if let Err(e) = session.run() {
                if !should_stop.load(Ordering::SeqCst) {
                    error!("FUSE session error: {}", e);
                }
            }

            debug!("FUSE session ended");

            // Update status to stopped
            {
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Stopped;
            }
        });

        self.fuse_thread = Some(handle);

        // Wait briefly for the mount to be ready
        thread::sleep(Duration::from_millis(100));

        // Check if mount succeeded
        let status = self.status();
        if status.is_error() {
            if let BackendStatus::Error { message, .. } = status {
                return Err(Error::FuseMountFailed(message));
            }
        }

        Ok(())
    }

    fn unmount(&mut self) -> Result<()> {
        if !self.is_mounted() {
            return Err(Error::NotMounted);
        }

        info!(
            "Unmounting FUSE filesystem at {:?} (platform: {})",
            self.config.mount_point,
            Platform::detect().name()
        );

        // Signal the thread to stop
        self.should_stop.store(true, Ordering::SeqCst);

        // Use platform-specific unmount command
        let mount_point = &self.config.mount_point;
        if let Err(e) = platform::unmount(mount_point) {
            return Err(Error::FuseUnmountFailed(e));
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
                    return Err(Error::Timeout(timeout));
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
