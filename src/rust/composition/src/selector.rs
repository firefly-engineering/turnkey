//! Backend selection based on platform and FUSE availability
//!
//! This module provides automatic backend selection based on:
//! - Platform detection (Linux, macOS)
//! - FUSE availability (native FUSE, FUSE-T, or none)
//! - User preferences (explicit backend override)
//!
//! # Selection Priority
//!
//! When `BackendType::Auto` is specified (the default):
//! 1. Check if FUSE is available on the platform
//! 2. If available, use FUSE backend
//! 3. Otherwise, fall back to symlink backend
//!
//! # Example
//!
//! ```ignore
//! use composition::{CompositionConfig, selector::{BackendType, create_backend}};
//!
//! let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
//!
//! // Auto-select best backend
//! let backend = create_backend(BackendType::Auto, config)?;
//!
//! // Or explicitly request symlinks
//! let backend = create_backend(BackendType::Symlink, config)?;
//! ```

use std::fmt;

use log::info;

use crate::{CompositionBackend, CompositionConfig, Error, Result, SymlinkBackend};

#[cfg(feature = "fuse")]
use crate::fuse::{check_fuse_availability, FuseAvailability, FuseBackend, Platform};

/// Backend type for composition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendType {
    /// Automatically select the best available backend
    ///
    /// Selection order:
    /// 1. FUSE (if available and enabled)
    /// 2. Symlink (fallback)
    #[default]
    Auto,

    /// Force FUSE backend
    ///
    /// Will fail if FUSE is not available on the platform.
    Fuse,

    /// Force symlink backend
    ///
    /// Always available, used for CI environments or when FUSE
    /// is not desired.
    Symlink,
}

impl BackendType {
    /// Parse backend type from string
    ///
    /// Accepts: "auto", "fuse", "symlink" (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(BackendType::Auto),
            "fuse" => Some(BackendType::Fuse),
            "symlink" | "symlinks" => Some(BackendType::Symlink),
            _ => None,
        }
    }

    /// Get all valid backend type names
    pub fn valid_names() -> &'static [&'static str] {
        &["auto", "fuse", "symlink"]
    }
}

impl fmt::Display for BackendType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendType::Auto => write!(f, "auto"),
            BackendType::Fuse => write!(f, "fuse"),
            BackendType::Symlink => write!(f, "symlink"),
        }
    }
}

/// Result of backend selection
#[derive(Debug)]
pub struct BackendSelection {
    /// The selected backend type
    pub backend_type: BackendType,
    /// Human-readable reason for the selection
    pub reason: String,
}

/// Determine which backend to use based on request and availability
#[cfg(feature = "fuse")]
pub fn select_backend(requested: BackendType) -> BackendSelection {
    match requested {
        BackendType::Fuse => BackendSelection {
            backend_type: BackendType::Fuse,
            reason: "FUSE backend explicitly requested".to_string(),
        },
        BackendType::Symlink => BackendSelection {
            backend_type: BackendType::Symlink,
            reason: "Symlink backend explicitly requested".to_string(),
        },
        BackendType::Auto => {
            let platform = Platform::detect();
            let availability = check_fuse_availability();

            match availability {
                FuseAvailability::Available {
                    implementation,
                    version,
                } => {
                    let version_str = version
                        .map(|v| format!(" ({})", v))
                        .unwrap_or_default();
                    BackendSelection {
                        backend_type: BackendType::Fuse,
                        reason: format!(
                            "Auto-selected FUSE on {}: {}{}",
                            platform.name(),
                            implementation,
                            version_str
                        ),
                    }
                }
                FuseAvailability::NotInstalled {
                    install_instructions,
                } => {
                    info!(
                        "FUSE not available, falling back to symlinks. To enable FUSE:\n{}",
                        install_instructions
                    );
                    BackendSelection {
                        backend_type: BackendType::Symlink,
                        reason: format!(
                            "Auto-selected symlinks: FUSE not installed on {}",
                            platform.name()
                        ),
                    }
                }
                FuseAvailability::UnsupportedPlatform => BackendSelection {
                    backend_type: BackendType::Symlink,
                    reason: format!(
                        "Auto-selected symlinks: FUSE not supported on {}",
                        platform.name()
                    ),
                },
            }
        }
    }
}

/// Determine which backend to use (non-FUSE builds always use symlinks)
#[cfg(not(feature = "fuse"))]
pub fn select_backend(requested: BackendType) -> BackendSelection {
    match requested {
        BackendType::Fuse => BackendSelection {
            backend_type: BackendType::Symlink,
            reason: "FUSE requested but not compiled in, using symlinks".to_string(),
        },
        BackendType::Symlink | BackendType::Auto => BackendSelection {
            backend_type: BackendType::Symlink,
            reason: "Symlink backend (FUSE not compiled in)".to_string(),
        },
    }
}

/// Create a backend based on the requested type
///
/// # Arguments
///
/// * `backend_type` - The requested backend type (Auto, Fuse, or Symlink)
/// * `config` - Configuration for the backend
///
/// # Returns
///
/// A boxed backend implementing `CompositionBackend`, or an error if the
/// requested backend is not available.
///
/// # Example
///
/// ```ignore
/// let config = CompositionConfig::new("/mount", "/repo");
/// let backend = create_backend(BackendType::Auto, config)?;
/// ```
pub fn create_backend(
    backend_type: BackendType,
    config: CompositionConfig,
) -> Result<Box<dyn CompositionBackend>> {
    let selection = select_backend(backend_type);
    info!("Backend selection: {}", selection.reason);

    match selection.backend_type {
        BackendType::Fuse => create_fuse_backend(config),
        BackendType::Symlink | BackendType::Auto => {
            // Auto should never reach here after select_backend, but handle it
            Ok(Box::new(SymlinkBackend::new(config)))
        }
    }
}

/// Create a FUSE backend (or error if not available)
#[cfg(feature = "fuse")]
fn create_fuse_backend(config: CompositionConfig) -> Result<Box<dyn CompositionBackend>> {
    let availability = check_fuse_availability();

    match availability {
        FuseAvailability::Available { .. } => Ok(Box::new(FuseBackend::new(config))),
        FuseAvailability::NotInstalled {
            install_instructions,
        } => Err(Error::FuseUnavailable(format!(
            "FUSE is not installed.\n\n{}",
            install_instructions
        ))),
        FuseAvailability::UnsupportedPlatform => {
            Err(Error::FuseUnavailable("FUSE is not supported on this platform".to_string()))
        }
    }
}

#[cfg(not(feature = "fuse"))]
fn create_fuse_backend(_config: CompositionConfig) -> Result<Box<dyn CompositionBackend>> {
    Err(Error::FuseUnavailable(
        "FUSE support not compiled in. Rebuild with --features fuse".to_string(),
    ))
}

/// Check if FUSE is available on this system
///
/// This is a convenience function for checking FUSE availability without
/// creating a backend.
#[cfg(feature = "fuse")]
pub fn is_fuse_available() -> bool {
    check_fuse_availability().is_available()
}

#[cfg(not(feature = "fuse"))]
pub fn is_fuse_available() -> bool {
    false
}

/// Get installation instructions for FUSE on the current platform
#[cfg(feature = "fuse")]
pub fn fuse_install_instructions() -> Option<String> {
    match check_fuse_availability() {
        FuseAvailability::NotInstalled {
            install_instructions,
        } => Some(install_instructions),
        _ => None,
    }
}

#[cfg(not(feature = "fuse"))]
pub fn fuse_install_instructions() -> Option<String> {
    Some("FUSE support not compiled in. Rebuild with --features fuse".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_from_str() {
        assert_eq!(BackendType::from_str("auto"), Some(BackendType::Auto));
        assert_eq!(BackendType::from_str("AUTO"), Some(BackendType::Auto));
        assert_eq!(BackendType::from_str("fuse"), Some(BackendType::Fuse));
        assert_eq!(BackendType::from_str("FUSE"), Some(BackendType::Fuse));
        assert_eq!(BackendType::from_str("symlink"), Some(BackendType::Symlink));
        assert_eq!(BackendType::from_str("symlinks"), Some(BackendType::Symlink));
        assert_eq!(BackendType::from_str("invalid"), None);
    }

    #[test]
    fn test_backend_type_display() {
        assert_eq!(BackendType::Auto.to_string(), "auto");
        assert_eq!(BackendType::Fuse.to_string(), "fuse");
        assert_eq!(BackendType::Symlink.to_string(), "symlink");
    }

    #[test]
    fn test_backend_type_default() {
        assert_eq!(BackendType::default(), BackendType::Auto);
    }

    #[test]
    fn test_select_backend_explicit_symlink() {
        let selection = select_backend(BackendType::Symlink);
        assert_eq!(selection.backend_type, BackendType::Symlink);
        assert!(selection.reason.contains("explicitly requested"));
    }

    #[test]
    fn test_select_backend_auto() {
        let selection = select_backend(BackendType::Auto);
        // Should select something (either Fuse or Symlink based on availability)
        assert!(
            selection.backend_type == BackendType::Fuse
                || selection.backend_type == BackendType::Symlink
        );
        assert!(selection.reason.contains("Auto-selected"));
    }

    #[test]
    fn test_valid_names() {
        let names = BackendType::valid_names();
        assert!(names.contains(&"auto"));
        assert!(names.contains(&"fuse"));
        assert!(names.contains(&"symlink"));
    }
}
