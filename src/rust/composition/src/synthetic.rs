//! macOS synthetic firmlink management
//!
//! On macOS, the root filesystem `/` is read-only (SIP). To create
//! top-level directories like `/firefly`, a synthetic firmlink must be
//! declared in `/etc/synthetic.conf` and activated with `apfs.util -t`.
//!
//! This module manages these entries so the composition daemon can
//! mount at fixed paths like `/firefly/turnkey`.

use std::path::{Path, PathBuf};
use std::process::Command;

use log::{info, warn};

/// Check if a mount point requires a synthetic firmlink on macOS.
///
/// Returns the top-level directory name if needed (e.g., "firefly" for "/firefly/turnkey"),
/// or None if the path is under a writable location.
pub fn needs_synthetic(mount_point: &Path) -> Option<String> {
    // Only relevant on macOS
    if !cfg!(target_os = "macos") {
        return None;
    }

    // Get the first component after /
    let components: Vec<_> = mount_point.components().collect();
    if components.len() < 2 {
        return None;
    }

    let first = match &components[1] {
        std::path::Component::Normal(s) => s.to_string_lossy().to_string(),
        _ => return None,
    };

    // These are standard writable directories on macOS — no synthetic needed
    let writable = [
        "Users", "Volumes", "tmp", "private", "var", "opt", "usr",
        "Applications", "Library", "System",
    ];
    if writable.contains(&first.as_str()) {
        return None;
    }

    // Check if the top-level directory already exists
    let top_level = PathBuf::from("/").join(&first);
    if top_level.exists() {
        return None;
    }

    Some(first)
}

/// Check if a synthetic firmlink entry exists in /etc/synthetic.conf.
pub fn has_synthetic_entry(name: &str) -> bool {
    if let Ok(content) = std::fs::read_to_string("/etc/synthetic.conf") {
        content.lines().any(|line| {
            let trimmed = line.trim();
            trimmed == name || trimmed.starts_with(&format!("{}\t", name))
        })
    } else {
        false
    }
}

/// Ensure a synthetic firmlink exists for the given directory name.
///
/// Adds the entry to /etc/synthetic.conf if missing, then activates it
/// with `apfs.util -t`. Requires sudo for both operations.
///
/// Returns Ok(true) if a new entry was created, Ok(false) if it already existed.
pub fn ensure_synthetic(name: &str) -> Result<bool, SyntheticError> {
    if has_synthetic_entry(name) {
        info!("Synthetic firmlink '{}' already in /etc/synthetic.conf", name);

        // Entry exists but directory might not be activated yet
        let top_level = PathBuf::from("/").join(name);
        if !top_level.exists() {
            activate_synthetics()?;
        }
        return Ok(false);
    }

    info!("Adding synthetic firmlink '{}' to /etc/synthetic.conf", name);

    // Append to /etc/synthetic.conf (requires sudo)
    let output = Command::new("sudo")
        .args(["tee", "-a", "/etc/synthetic.conf"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                writeln!(stdin, "{}", name)?;
            }
            child.wait_with_output()
        })
        .map_err(|e| SyntheticError::Io {
            message: format!("failed to write /etc/synthetic.conf: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SyntheticError::Permission {
            message: format!(
                "Failed to update /etc/synthetic.conf (sudo required): {}",
                stderr.trim()
            ),
        });
    }

    // Activate the synthetic firmlinks
    activate_synthetics()?;

    // Verify it was created
    let top_level = PathBuf::from("/").join(name);
    if !top_level.exists() {
        return Err(SyntheticError::Activation {
            message: format!(
                "/{} was not created after activation. A reboot may be required.",
                name
            ),
        });
    }

    info!("Synthetic firmlink /{} activated", name);
    Ok(true)
}

/// Activate synthetic firmlinks without reboot.
fn activate_synthetics() -> Result<(), SyntheticError> {
    info!("Activating synthetic firmlinks via apfs.util -t");

    let output = Command::new("sudo")
        .args(["/System/Library/Filesystems/apfs.fs/Contents/Resources/apfs.util", "-t"])
        .output()
        .map_err(|e| SyntheticError::Io {
            message: format!("failed to run apfs.util: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("apfs.util -t returned non-zero (may be normal): {}", stderr.trim());
        // Non-zero exit from apfs.util is sometimes normal (e.g., already activated)
    }

    Ok(())
}

/// Ensure the mount point directory exists, creating synthetic firmlinks if needed.
pub fn ensure_mount_point(mount_point: &Path) -> Result<(), SyntheticError> {
    if mount_point.exists() {
        return Ok(());
    }

    // Check if we need a synthetic firmlink
    if let Some(name) = needs_synthetic(mount_point) {
        ensure_synthetic(&name)?;
    }

    // Create the full directory path
    std::fs::create_dir_all(mount_point).map_err(|e| SyntheticError::Io {
        message: format!("failed to create {:?}: {}", mount_point, e),
    })?;

    Ok(())
}

#[derive(Debug)]
pub enum SyntheticError {
    Io { message: String },
    Permission { message: String },
    Activation { message: String },
}

impl std::fmt::Display for SyntheticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyntheticError::Io { message } => write!(f, "{}", message),
            SyntheticError::Permission { message } => write!(f, "{}", message),
            SyntheticError::Activation { message } => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for SyntheticError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_synthetic_firefly() {
        let result = needs_synthetic(Path::new("/firefly/turnkey"));
        if cfg!(target_os = "macos") {
            // On macOS, /firefly doesn't exist by default
            // But in test env it might if we created it
            assert!(result.is_some() || Path::new("/firefly").exists());
        } else {
            assert!(result.is_none()); // Not macOS
        }
    }

    #[test]
    fn test_needs_synthetic_home() {
        // Under /Users — no synthetic needed
        assert!(needs_synthetic(Path::new("/Users/yann/firefly/turnkey")).is_none());
    }

    #[test]
    fn test_needs_synthetic_tmp() {
        assert!(needs_synthetic(Path::new("/tmp/turnkey-test")).is_none());
    }

    #[test]
    fn test_needs_synthetic_volumes() {
        assert!(needs_synthetic(Path::new("/Volumes/data/turnkey")).is_none());
    }
}
