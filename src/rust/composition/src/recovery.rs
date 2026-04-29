//! Error recovery utilities for composition backends
//!
//! This module provides utilities for handling and recovering from errors:
//! - Retry with exponential backoff for transient failures
//! - Error categorization (transient vs permanent)
//! - Recovery suggestions for common errors
//! - Daemon state recovery helpers
//!
//! # Example
//!
//! ```ignore
//! use composition::recovery::{retry_with_backoff, RetryConfig, is_transient_error};
//!
//! // Retry a potentially flaky operation
//! let result = retry_with_backoff(
//!     || some_flaky_operation(),
//!     RetryConfig::default(),
//! )?;
//!
//! // Check if an error is transient
//! if is_transient_error(&err) {
//!     // Suggest retry
//! }
//! ```

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use log::{debug, info, warn};

use crate::{Error, Result};

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles delay each time)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to delays (helps avoid thundering herd)
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Create a config for quick retries (short delays)
    pub fn quick() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    /// Create a config for patient retries (longer delays for slow operations)
    pub fn patient() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    /// Set the maximum number of attempts
    pub fn with_max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts;
        self
    }

    /// Set the initial delay
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }
}

/// Retry an operation with exponential backoff
///
/// # Arguments
///
/// * `operation` - The operation to retry. Should return `Result<T>`.
/// * `config` - Retry configuration
///
/// # Returns
///
/// The successful result, or the last error if all retries failed.
///
/// # Example
///
/// ```ignore
/// let result = retry_with_backoff(
///     || mount_fuse_filesystem(),
///     RetryConfig::default(),
/// )?;
/// ```
pub fn retry_with_backoff<T, F>(mut operation: F, config: RetryConfig) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut last_error = None;
    let mut delay = config.initial_delay;

    for attempt in 1..=config.max_attempts {
        match operation() {
            Ok(result) => {
                if attempt > 1 {
                    info!("Operation succeeded on attempt {}", attempt);
                }
                return Ok(result);
            }
            Err(e) => {
                let is_last = attempt == config.max_attempts;
                let is_transient = is_transient_error(&e);

                if is_last || !is_transient {
                    if !is_transient {
                        debug!(
                            "Non-transient error on attempt {}, not retrying: {}",
                            attempt, e
                        );
                    }
                    last_error = Some(e);
                    break;
                }

                warn!(
                    "Transient error on attempt {}/{}: {}. Retrying in {:?}...",
                    attempt, config.max_attempts, e, delay
                );

                // Sleep with optional jitter
                let actual_delay = if config.jitter {
                    add_jitter(delay)
                } else {
                    delay
                };
                thread::sleep(actual_delay);

                // Calculate next delay with exponential backoff
                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * config.backoff_multiplier).min(config.max_delay.as_secs_f64()),
                );
            }
        }
    }

    Err(last_error.unwrap_or_else(|| Error::ConfigError("retry failed with no error".into())))
}

/// Add random jitter to a duration (±25%)
fn add_jitter(duration: Duration) -> Duration {
    use std::time::SystemTime;

    // Simple pseudo-random based on time
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let jitter_factor = 0.75 + (nanos % 500) as f64 / 1000.0; // 0.75 to 1.25

    Duration::from_secs_f64(duration.as_secs_f64() * jitter_factor)
}

/// Check if an error is transient (worth retrying)
///
/// Transient errors are typically:
/// - I/O errors that might succeed on retry (EAGAIN, EINTR, etc.)
/// - Timeouts
/// - Temporary resource unavailability
///
/// Non-transient errors are:
/// - Configuration errors
/// - Permission errors
/// - Missing files/paths
/// - Invalid state transitions
pub fn is_transient_error(error: &Error) -> bool {
    match error {
        // Timeouts are transient
        Error::Timeout(_) => true,

        // I/O errors may be transient
        Error::Io(io_err) => matches!(
            io_err.kind(),
            std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
        ),

        // Mount point inaccessible might be transient (NFS, etc.)
        Error::MountPointInaccessible { source, .. } => matches!(
            source.kind(),
            std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut
        ),

        // Path updating is transient (wait and retry)
        Error::PathUpdating(_) => true,

        // These are NOT transient
        Error::AlreadyMounted(_) => false,
        Error::NotMounted => false,
        Error::CellNotFound(_) => false,
        Error::CellSourceNotFound { .. } => false,
        Error::ConfigError(_) => false,
        Error::StateTransitionError(_) => false,
        Error::FuseUnavailable(_) => false,

        // Build failures may or may not be transient
        Error::NixBuildFailed { source, .. } => matches!(
            source.kind(),
            std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut
        ),

        // Other errors - assume not transient
        _ => false,
    }
}

/// Get a human-readable recovery suggestion for an error
///
/// # Example
///
/// ```ignore
/// match result {
///     Err(e) => {
///         eprintln!("Error: {}", e);
///         if let Some(suggestion) = recovery_suggestion(&e) {
///             eprintln!("Suggestion: {}", suggestion);
///         }
///     }
///     Ok(_) => {}
/// }
/// ```
pub fn recovery_suggestion(error: &Error) -> Option<String> {
    match error {
        Error::AlreadyMounted(path) => Some(format!(
            "The mount point {} is already in use.\n\
             Try one of:\n\
             • Run 'turnkey-composed stop' to stop the existing daemon\n\
             • Use 'umount {}' to manually unmount\n\
             • Check for stale mounts with 'mount | grep turnkey'",
            path.display(),
            path.display()
        )),

        Error::NotMounted => Some(
            "The composition backend is not mounted.\n\
             Try: 'turnkey-composed start --mount-point <path> --repo-root <path>'"
                .to_string(),
        ),

        Error::MountPointInaccessible { path, source } => {
            let base = format!(
                "Cannot access mount point: {}\n\
                 Error: {}",
                path.display(),
                source
            );
            Some(format!(
                "{}\n\
                 Try:\n\
                 • Check that the path exists and is accessible\n\
                 • Verify you have read/write permissions\n\
                 • If it's a network path, check connectivity",
                base
            ))
        }

        Error::MountPointCreationFailed { path, source } => Some(format!(
            "Failed to create mount point directory: {}\n\
             Error: {}\n\
             Try:\n\
             • Check parent directory permissions\n\
             • Create the directory manually: mkdir -p {}",
            path.display(),
            source,
            path.display()
        )),

        Error::CellSourceNotFound { cell, path } => Some(format!(
            "Cell '{}' source path does not exist: {}\n\
             This usually means the Nix derivation hasn't been built.\n\
             Try:\n\
             • Run 'tk sync' to build dependencies\n\
             • Check that the Nix store path is correct\n\
             • Verify the cell configuration in your settings",
            cell,
            path.display()
        )),

        Error::FuseUnavailable(msg) => Some(format!(
            "FUSE is not available: {}\n\
             Options:\n\
             • Install FUSE for your platform:\n\
               - Linux: sudo apt install fuse3 (or fuse)\n\
               - macOS: brew install --cask macfuse, then activate the\n\
                 FSKit extension via the macFUSE app and System Settings.\n\
             • Use symlink backend instead: --backend=symlink\n\
             • Check /dev/fuse permissions",
            msg
        )),

        Error::FuseMountFailed(msg) => Some(format!(
            "FUSE mount failed: {}\n\
             Common causes:\n\
             • Mount point already in use (check with 'mount')\n\
             • Insufficient permissions (may need root or fuse group)\n\
             • FUSE module not loaded (try 'modprobe fuse')\n\
             • On macOS: check System Preferences → Security & Privacy",
            msg
        )),

        Error::FuseUnmountFailed(msg) => Some(format!(
            "FUSE unmount failed: {}\n\
             Try:\n\
             • Close any programs using files in the mount\n\
             • Use 'lsof +D <mount-point>' to find open files\n\
             • Force unmount: 'umount -f <mount-point>' or 'fusermount -uz <mount-point>'",
            msg
        )),

        Error::NixBuildFailed { cell, .. } => Some(format!(
            "Nix build failed for cell '{}'\n\
             Try:\n\
             • Check Nix daemon is running: 'systemctl status nix-daemon'\n\
             • Verify network connectivity for fetching\n\
             • Check available disk space\n\
             • Run 'nix build' manually to see detailed errors",
            cell
        )),

        Error::Timeout(duration) => Some(format!(
            "Operation timed out after {:?}\n\
             This might be a transient issue. Try:\n\
             • Retry the operation\n\
             • Check system load and available resources\n\
             • Increase timeout if this consistently occurs",
            duration
        )),

        Error::StateTransitionError(msg) => Some(format!(
            "Invalid state transition: {}\n\
             The backend is in an unexpected state.\n\
             Try:\n\
             • Check current status with 'turnkey-composed status'\n\
             • Restart the daemon: 'turnkey-composed stop && turnkey-composed start ...'",
            msg
        )),

        // No specific suggestion for these
        Error::SymlinkFailed { .. }
        | Error::SymlinkRemoveFailed { .. }
        | Error::CellNotFound(_)
        | Error::RefreshInvalidState(_)
        | Error::ConfigError(_)
        | Error::PathUpdating(_)
        | Error::Io(_) => None,
    }
}

/// Recovery helper for daemon startup
///
/// Checks for and cleans up stale state from previous daemon runs:
/// - Removes stale socket files
/// - Detects busy mount points
/// - Checks for orphaned FUSE mounts
#[derive(Debug)]
pub struct DaemonRecovery {
    socket_path: PathBuf,
    mount_point: PathBuf,
}

/// Result of daemon recovery check
#[derive(Debug)]
pub enum RecoveryAction {
    /// No recovery needed, can proceed normally
    Ready,
    /// Cleaned up stale socket, can proceed
    CleanedSocket,
    /// Mount point is busy, need to unmount first
    MountPointBusy { suggestion: String },
    /// Previous daemon is still running
    DaemonRunning { pid: Option<u32> },
}

impl DaemonRecovery {
    /// Create a new daemon recovery helper
    pub fn new(socket_path: impl Into<PathBuf>, mount_point: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            mount_point: mount_point.into(),
        }
    }

    /// Check for and recover from stale daemon state
    ///
    /// Returns the action taken or required.
    pub fn check_and_recover(&self) -> Result<RecoveryAction> {
        // Check if socket exists and is stale
        if self.socket_path.exists() {
            if self.is_socket_active()? {
                return Ok(RecoveryAction::DaemonRunning { pid: None });
            }

            // Socket exists but daemon isn't responding - clean it up
            info!(
                "Removing stale socket at {:?}",
                self.socket_path
            );
            std::fs::remove_file(&self.socket_path).map_err(|e| Error::Io(e))?;
            return Ok(RecoveryAction::CleanedSocket);
        }

        // Check if mount point is busy
        if self.mount_point.exists() && self.is_mount_point_busy()? {
            let suggestion = format!(
                "Mount point {:?} appears to be in use.\n\
                 Try:\n\
                 • umount {:?}\n\
                 • fusermount -uz {:?}",
                self.mount_point, self.mount_point, self.mount_point
            );
            return Ok(RecoveryAction::MountPointBusy { suggestion });
        }

        Ok(RecoveryAction::Ready)
    }

    /// Check if the socket is active (daemon is running)
    fn is_socket_active(&self) -> Result<bool> {
        use std::os::unix::net::UnixStream;

        match UnixStream::connect(&self.socket_path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => {
                // Other errors - assume not active but log it
                debug!("Error checking socket: {}", e);
                Ok(false)
            }
        }
    }

    /// Check if the mount point is busy (already mounted)
    fn is_mount_point_busy(&self) -> Result<bool> {
        // Check by comparing device IDs with parent
        let mp_meta = match std::fs::metadata(&self.mount_point) {
            Ok(m) => m,
            Err(_) => return Ok(false),
        };

        let parent = match self.mount_point.parent() {
            Some(p) => p,
            None => return Ok(false),
        };

        let parent_meta = match std::fs::metadata(parent) {
            Ok(m) => m,
            Err(_) => return Ok(false),
        };

        use std::os::unix::fs::MetadataExt;
        Ok(mp_meta.dev() != parent_meta.dev())
    }
}

/// Force unmount a FUSE mount point
///
/// Attempts various unmount strategies:
/// 1. Normal unmount
/// 2. Lazy unmount (fusermount -uz)
/// 3. Force unmount (umount -f)
pub fn force_unmount(mount_point: &Path) -> Result<()> {
    use std::process::Command;

    // Try fusermount first (Linux)
    let result = Command::new("fusermount")
        .args(["-uz", mount_point.to_str().unwrap_or("")])
        .output();

    if let Ok(output) = result {
        if output.status.success() {
            info!("Successfully unmounted {:?} with fusermount", mount_point);
            return Ok(());
        }
    }

    // Try umount (works on both Linux and macOS)
    let result = Command::new("umount")
        .args(["-f", mount_point.to_str().unwrap_or("")])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            info!("Successfully unmounted {:?} with umount", mount_point);
            Ok(())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::FuseUnmountFailed(format!(
                "umount failed: {}",
                stderr.trim()
            )))
        }
        Err(e) => Err(Error::FuseUnmountFailed(format!(
            "failed to run umount: {}",
            e
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.initial_delay, Duration::from_millis(100));
    }

    #[test]
    fn test_retry_config_quick() {
        let config = RetryConfig::quick();
        assert_eq!(config.max_attempts, 3);
        assert!(config.initial_delay < Duration::from_millis(100));
    }

    #[test]
    fn test_retry_config_patient() {
        let config = RetryConfig::patient();
        assert_eq!(config.max_attempts, 5);
        assert!(config.initial_delay >= Duration::from_secs(1));
    }

    #[test]
    fn test_is_transient_error_timeout() {
        let error = Error::Timeout(Duration::from_secs(5));
        assert!(is_transient_error(&error));
    }

    #[test]
    fn test_is_transient_error_not_mounted() {
        let error = Error::NotMounted;
        assert!(!is_transient_error(&error));
    }

    #[test]
    fn test_is_transient_error_config() {
        let error = Error::ConfigError("bad config".into());
        assert!(!is_transient_error(&error));
    }

    #[test]
    fn test_is_transient_error_path_updating() {
        let error = Error::PathUpdating(PathBuf::from("/some/path"));
        assert!(is_transient_error(&error));
    }

    #[test]
    fn test_recovery_suggestion_not_mounted() {
        let error = Error::NotMounted;
        let suggestion = recovery_suggestion(&error);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("not mounted"));
    }

    #[test]
    fn test_recovery_suggestion_already_mounted() {
        let error = Error::AlreadyMounted(PathBuf::from("/mnt/test"));
        let suggestion = recovery_suggestion(&error);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("already in use"));
    }

    #[test]
    fn test_recovery_suggestion_fuse_unavailable() {
        let error = Error::FuseUnavailable("not installed".into());
        let suggestion = recovery_suggestion(&error);
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert!(s.contains("Install FUSE"));
        assert!(s.contains("--backend=symlink"));
    }

    #[test]
    fn test_retry_success_first_attempt() {
        let mut attempts = 0;
        let result: Result<i32> = retry_with_backoff(
            || {
                attempts += 1;
                Ok(42)
            },
            RetryConfig::quick(),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn test_retry_success_after_failures() {
        let mut attempts = 0;
        let result: Result<i32> = retry_with_backoff(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(Error::Timeout(Duration::from_secs(1)))
                } else {
                    Ok(42)
                }
            },
            RetryConfig::quick(),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);
    }

    #[test]
    fn test_retry_all_failures() {
        let mut attempts = 0;
        let result: Result<i32> = retry_with_backoff(
            || {
                attempts += 1;
                Err(Error::Timeout(Duration::from_secs(1)))
            },
            RetryConfig::quick().with_max_attempts(2),
        );

        assert!(result.is_err());
        assert_eq!(attempts, 2);
    }

    #[test]
    fn test_retry_non_transient_no_retry() {
        let mut attempts = 0;
        let result: Result<i32> = retry_with_backoff(
            || {
                attempts += 1;
                Err(Error::ConfigError("bad config".into()))
            },
            RetryConfig::quick(),
        );

        assert!(result.is_err());
        assert_eq!(attempts, 1); // Should not retry non-transient errors
    }

    #[test]
    fn test_add_jitter() {
        let duration = Duration::from_secs(1);
        let jittered = add_jitter(duration);

        // Should be within ±25% of original
        let min = Duration::from_millis(750);
        let max = Duration::from_millis(1250);
        assert!(jittered >= min && jittered <= max);
    }

    #[test]
    fn test_daemon_recovery_new() {
        let recovery = DaemonRecovery::new("/tmp/test.sock", "/mnt/test");
        assert_eq!(recovery.socket_path, PathBuf::from("/tmp/test.sock"));
        assert_eq!(recovery.mount_point, PathBuf::from("/mnt/test"));
    }
}
