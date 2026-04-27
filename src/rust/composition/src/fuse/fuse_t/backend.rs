//! FUSE-T backend implementation for macOS
//!
//! Uses direct FFI to libfuse3 (FUSE-T) instead of the `fuser` crate.
//! The key difference: we call `fuse_loop()` which uses FUSE-T's internal
//! NFS-based session loop, rather than reading the fd directly.

#![cfg(target_os = "macos")]

use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::{debug, error, info};

use super::bindings;
use super::operations;
use crate::fuse::fs_core::FsCore;
use crate::fuse::platform::{self, Platform};
use crate::state::ConsistencyStateMachine;
use crate::{BackendStatus, CellMapping, CompositionBackend, CompositionConfig, Error, Result};

/// FUSE-T-based composition backend for macOS
pub struct FuseTBackend {
    config: CompositionConfig,
    status: Arc<Mutex<BackendStatus>>,
    state_machine: Arc<ConsistencyStateMachine>,
    should_stop: Arc<AtomicBool>,
    fuse_thread: Option<JoinHandle<()>>,
}

impl FuseTBackend {
    pub fn new(config: CompositionConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(BackendStatus::Stopped)),
            state_machine: Arc::new(ConsistencyStateMachine::new()),
            should_stop: Arc::new(AtomicBool::new(false)),
            fuse_thread: None,
        }
    }

    pub fn state_machine(&self) -> &Arc<ConsistencyStateMachine> {
        &self.state_machine
    }

    fn ensure_mount_point(&self) -> Result<()> {
        let mp = &self.config.mount_point;
        if !mp.exists() {
            std::fs::create_dir_all(mp).map_err(|e| Error::MountPointCreationFailed {
                path: mp.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    fn is_mount_point_busy(&self) -> bool {
        let mp = &self.config.mount_point;
        if let (Ok(mp_meta), Ok(parent_meta)) = (
            std::fs::metadata(mp),
            mp.parent()
                .and_then(|p| std::fs::metadata(p).ok())
                .ok_or(()),
        ) {
            use std::os::unix::fs::MetadataExt;
            mp_meta.dev() != parent_meta.dev()
        } else {
            false
        }
    }
}

impl CompositionBackend for FuseTBackend {
    fn config(&self) -> &CompositionConfig {
        &self.config
    }

    fn mount(&mut self) -> Result<()> {
        if self.is_mounted() {
            return Err(Error::AlreadyMounted(self.config.mount_point.clone()));
        }
        if self.is_mount_point_busy() {
            return Err(Error::AlreadyMounted(self.config.mount_point.clone()));
        }
        self.ensure_mount_point()?;
        self.should_stop.store(false, Ordering::SeqCst);

        {
            let mut status = self.status.lock().unwrap();
            *status = BackendStatus::Building {
                affected_paths: vec![self.config.mount_point.clone()],
                message: Some("mounting FUSE-T filesystem".into()),
            };
        }

        let mount_point = self.config.mount_point.clone();
        let config = self.config.clone();
        let repo_root = self.config.repo_root.clone();
        let status = Arc::clone(&self.status);
        let should_stop = Arc::clone(&self.should_stop);
        let state_machine = Arc::clone(&self.state_machine);

        info!("Mounting FUSE-T filesystem at {:?}", mount_point);

        let handle = thread::spawn(move || {
            // Create the filesystem core
            let core = FsCore::new(config, repo_root, state_machine);
            let core_ptr = &core as *const FsCore as *mut c_void;

            // Build fuse_args: argv[0] = program name, then mount options
            let arg0 = CString::new("turnkey-composed").unwrap();
            let arg_ro = CString::new("-o").unwrap();
            let arg_ro_val = CString::new("ro,fsname=turnkey").unwrap();
            let mut argv: Vec<*mut i8> = vec![
                arg0.as_ptr() as *mut i8,
                arg_ro.as_ptr() as *mut i8,
                arg_ro_val.as_ptr() as *mut i8,
            ];
            let mut args = bindings::fuse_args {
                argc: argv.len() as i32,
                argv: argv.as_mut_ptr(),
                allocated: 0,
            };

            // Build the operations table
            let ops = operations::build_operations();

            // Create FUSE instance
            let mut version = bindings::libfuse_version {
                major: bindings::FUSE_MAJOR_VERSION,
                minor: bindings::FUSE_MINOR_VERSION,
                hotfix: 0,
                padding: 0,
            };

            let fuse = unsafe {
                bindings::fuse_new(
                    &mut args,
                    &ops,
                    std::mem::size_of::<bindings::fuse_operations>(),
                    &mut version,
                    core_ptr,
                )
            };

            if fuse.is_null() {
                error!("fuse_new() returned null");
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Error {
                    message: "fuse_new() failed".into(),
                    recoverable: false,
                };
                return;
            }

            // Mount
            let mount_cstr = CString::new(mount_point.to_str().unwrap()).unwrap();
            let ret = unsafe { bindings::fuse_mount(fuse, mount_cstr.as_ptr()) };
            if ret != 0 {
                error!("fuse_mount() failed with code {}", ret);
                unsafe { bindings::fuse_destroy(fuse) };
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Error {
                    message: format!("fuse_mount() failed: {}", ret),
                    recoverable: false,
                };
                return;
            }

            // Update status to ready
            {
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Ready;
            }

            info!("FUSE-T filesystem mounted and ready");
            debug!("Entering fuse_loop()");

            // Run the event loop — this blocks until unmounted
            let ret = unsafe { bindings::fuse_loop(fuse) };
            if ret != 0 && !should_stop.load(Ordering::SeqCst) {
                error!("fuse_loop() returned {}", ret);
            }

            debug!("fuse_loop() returned, cleaning up");
            unsafe {
                bindings::fuse_unmount(fuse);
                bindings::fuse_destroy(fuse);
            }

            {
                let mut s = status.lock().unwrap();
                *s = BackendStatus::Stopped;
            }
        });

        self.fuse_thread = Some(handle);

        // Wait briefly for mount
        thread::sleep(Duration::from_millis(200));

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
            "Unmounting FUSE-T filesystem at {:?} (platform: {})",
            self.config.mount_point,
            Platform::detect().name()
        );

        self.should_stop.store(true, Ordering::SeqCst);

        if let Err(e) = platform::unmount(&self.config.mount_point) {
            return Err(Error::FuseUnmountFailed(e));
        }

        if let Some(handle) = self.fuse_thread.take() {
            handle
                .join()
                .map_err(|_| Error::FuseUnmountFailed("thread join failed".into()))?;
        }

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
        info!("Refresh requested (currently a no-op)");
        Ok(())
    }

    fn cell_path(&self, cell_name: &str) -> Option<PathBuf> {
        self.config
            .cells
            .iter()
            .find(|c| c.name == cell_name)
            .map(|_| {
                self.config
                    .mount_point
                    .join(&self.config.cell_prefix)
                    .join(cell_name)
            })
    }

    fn cell_mappings(&self) -> Vec<CellMapping> {
        self.config
            .cells
            .iter()
            .map(|c| {
                CellMapping::new(
                    c.name.clone(),
                    self.config
                        .mount_point
                        .join(&self.config.cell_prefix)
                        .join(&c.name),
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

impl Drop for FuseTBackend {
    fn drop(&mut self) {
        if self.is_mounted() {
            if let Err(e) = self.unmount() {
                error!("Failed to unmount FUSE-T filesystem on drop: {}", e);
            }
        }
    }
}
