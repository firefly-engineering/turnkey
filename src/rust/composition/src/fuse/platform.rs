//! Platform detection and FUSE availability checking
//!
//! This module provides platform-specific functionality for FUSE operations:
//! - Linux: Uses native FUSE via /dev/fuse
//! - macOS: Uses FUSE-T (NFS-based, no kernel extension required)
//!
//! # FUSE-T on macOS
//!
//! FUSE-T is the recommended FUSE implementation for macOS as it doesn't require
//! a kernel extension (important for Apple Silicon and security). It provides
//! binary compatibility with macFUSE, allowing the `fuser` crate to work.
//!
//! Installation: `brew install macos-fuse-t/homebrew-cask/fuse-t`

use std::path::Path;
use std::process::Command;

/// Supported platforms for FUSE operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux with native FUSE support
    Linux,
    /// macOS with FUSE-T
    MacOS,
    /// Unsupported platform
    Unsupported,
}

impl Platform {
    /// Detect the current platform
    pub fn detect() -> Self {
        #[cfg(target_os = "linux")]
        {
            Platform::Linux
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOS
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Platform::Unsupported
        }
    }

    /// Get a human-readable name for this platform
    pub fn name(&self) -> &'static str {
        match self {
            Platform::Linux => "Linux",
            Platform::MacOS => "macOS",
            Platform::Unsupported => "unsupported",
        }
    }
}

/// FUSE availability status
#[derive(Debug, Clone)]
pub enum FuseAvailability {
    /// FUSE is available and ready to use
    Available {
        /// Name of the FUSE implementation
        implementation: String,
        /// Version if known
        version: Option<String>,
    },
    /// FUSE is not installed
    NotInstalled {
        /// Instructions for installing FUSE
        install_instructions: String,
    },
    /// Platform doesn't support FUSE
    UnsupportedPlatform,
}

impl FuseAvailability {
    /// Check if FUSE is available
    pub fn is_available(&self) -> bool {
        matches!(self, FuseAvailability::Available { .. })
    }
}

/// Check FUSE availability on the current platform
pub fn check_fuse_availability() -> FuseAvailability {
    match Platform::detect() {
        Platform::Linux => check_fuse_linux(),
        Platform::MacOS => check_fuse_macos(),
        Platform::Unsupported => FuseAvailability::UnsupportedPlatform,
    }
}

/// Check FUSE availability on Linux
fn check_fuse_linux() -> FuseAvailability {
    // Check if /dev/fuse exists
    if Path::new("/dev/fuse").exists() {
        // Try to get version from fusermount
        let version = get_fusermount_version();
        FuseAvailability::Available {
            implementation: "FUSE".to_string(),
            version,
        }
    } else {
        FuseAvailability::NotInstalled {
            install_instructions: concat!(
                "Install FUSE on your Linux distribution:\n",
                "  Ubuntu/Debian: sudo apt install fuse3\n",
                "  Fedora: sudo dnf install fuse3\n",
                "  Arch: sudo pacman -S fuse3"
            )
            .to_string(),
        }
    }
}

/// Check FUSE availability on macOS (FUSE-T)
fn check_fuse_macos() -> FuseAvailability {
    // Check for FUSE-T installation
    // FUSE-T installs to /Library/Filesystems/fuse-t.fs
    let fuse_t_path = Path::new("/Library/Filesystems/fuse-t.fs");

    if fuse_t_path.exists() {
        let version = get_fuse_t_version();
        FuseAvailability::Available {
            implementation: "FUSE-T".to_string(),
            version,
        }
    } else {
        // Also check for macFUSE as fallback
        let macfuse_path = Path::new("/Library/Filesystems/macfuse.fs");
        if macfuse_path.exists() {
            FuseAvailability::Available {
                implementation: "macFUSE".to_string(),
                version: None,
            }
        } else {
            FuseAvailability::NotInstalled {
                install_instructions: concat!(
                    "Install FUSE-T on macOS (recommended, no kernel extension required):\n",
                    "  brew install macos-fuse-t/homebrew-cask/fuse-t\n",
                    "\n",
                    "Alternatively, install macFUSE (requires kernel extension):\n",
                    "  brew install --cask macfuse"
                )
                .to_string(),
            }
        }
    }
}

/// Get fusermount version on Linux
fn get_fusermount_version() -> Option<String> {
    // Try fusermount3 first
    if let Ok(output) = Command::new("fusermount3").arg("-V").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Some(stdout.trim().to_string());
        }
    }

    // Fall back to fusermount
    if let Ok(output) = Command::new("fusermount").arg("-V").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Some(stdout.trim().to_string());
        }
    }

    None
}

/// Get FUSE-T version on macOS
fn get_fuse_t_version() -> Option<String> {
    // FUSE-T stores version in Info.plist
    let plist_path = "/Library/Filesystems/fuse-t.fs/Contents/Info.plist";
    if Path::new(plist_path).exists() {
        // Use defaults to read the version
        if let Ok(output) = Command::new("defaults")
            .args(["read", plist_path, "CFBundleShortVersionString"])
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                return Some(format!("FUSE-T {}", version.trim()));
            }
        }
    }
    None
}

/// Unmount a FUSE filesystem
///
/// Uses the appropriate command for the current platform:
/// - Linux: fusermount3 -u or fusermount -u
/// - macOS: umount
///
/// Returns an error message if unmount fails.
pub fn unmount(mount_point: &Path) -> Result<(), String> {
    match Platform::detect() {
        Platform::Linux => unmount_linux(mount_point),
        Platform::MacOS => unmount_macos(mount_point),
        Platform::Unsupported => Err("FUSE not supported on this platform".to_string()),
    }
}

/// Unmount on Linux using fusermount
fn unmount_linux(mount_point: &Path) -> Result<(), String> {
    // Try fusermount3 first
    let result = Command::new("fusermount3")
        .arg("-u")
        .arg(mount_point)
        .output();

    match result {
        Ok(output) if output.status.success() => return Ok(()),
        Ok(output) => {
            // fusermount3 failed, try fusermount as fallback
            let result2 = Command::new("fusermount")
                .arg("-u")
                .arg(mount_point)
                .output();

            match result2 {
                Ok(output2) if output2.status.success() => Ok(()),
                Ok(output2) => {
                    let stderr = String::from_utf8_lossy(&output2.stderr);
                    Err(format!("fusermount failed: {}", stderr.trim()))
                }
                Err(e) => {
                    // Both failed, report the original error
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(format!(
                        "fusermount3 failed: {}, fusermount error: {}",
                        stderr.trim(),
                        e
                    ))
                }
            }
        }
        Err(_) => {
            // fusermount3 not found, try fusermount
            let result2 = Command::new("fusermount")
                .arg("-u")
                .arg(mount_point)
                .output();

            match result2 {
                Ok(output2) if output2.status.success() => Ok(()),
                Ok(output2) => {
                    let stderr = String::from_utf8_lossy(&output2.stderr);
                    Err(format!("fusermount failed: {}", stderr.trim()))
                }
                Err(e) => Err(format!("fusermount not found: {}", e)),
            }
        }
    }
}

/// Unmount on macOS using umount
fn unmount_macos(mount_point: &Path) -> Result<(), String> {
    // On macOS, use the standard umount command
    let result = Command::new("umount").arg(mount_point).output();

    match result {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If regular umount fails, try with diskutil (handles force unmount better)
            let result2 = Command::new("diskutil")
                .args(["unmount", "force"])
                .arg(mount_point)
                .output();

            match result2 {
                Ok(output2) if output2.status.success() => Ok(()),
                _ => Err(format!("umount failed: {}", stderr.trim())),
            }
        }
        Err(e) => Err(format!("umount command failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detect() {
        let platform = Platform::detect();

        #[cfg(target_os = "linux")]
        assert_eq!(platform, Platform::Linux);

        #[cfg(target_os = "macos")]
        assert_eq!(platform, Platform::MacOS);

        // Platform should have a name
        assert!(!platform.name().is_empty());
    }

    #[test]
    fn test_platform_name() {
        assert_eq!(Platform::Linux.name(), "Linux");
        assert_eq!(Platform::MacOS.name(), "macOS");
        assert_eq!(Platform::Unsupported.name(), "unsupported");
    }

    #[test]
    fn test_fuse_availability_is_available() {
        let available = FuseAvailability::Available {
            implementation: "FUSE".to_string(),
            version: Some("3.10.0".to_string()),
        };
        assert!(available.is_available());

        let not_installed = FuseAvailability::NotInstalled {
            install_instructions: "test".to_string(),
        };
        assert!(!not_installed.is_available());

        let unsupported = FuseAvailability::UnsupportedPlatform;
        assert!(!unsupported.is_available());
    }

    #[test]
    fn test_check_fuse_availability() {
        // Just test that it doesn't panic
        let availability = check_fuse_availability();

        // On CI/test systems, FUSE might not be available
        match availability {
            FuseAvailability::Available { implementation, .. } => {
                assert!(!implementation.is_empty());
            }
            FuseAvailability::NotInstalled {
                install_instructions,
            } => {
                assert!(!install_instructions.is_empty());
            }
            FuseAvailability::UnsupportedPlatform => {
                // Expected on unsupported platforms
            }
        }
    }
}
