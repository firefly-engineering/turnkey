//! Backend status types

use std::path::PathBuf;

/// The current status of a composition backend
///
/// The status follows a state machine pattern:
///
/// ```text
/// STOPPED ──mount()──► READY ◄──────────────────┐
///    ▲                   │                      │
///    │              manifest changed            │
///    │                   │                      │
///    │                   ▼                      │
/// unmount()          UPDATING                   │
///    │                   │                      │
///    │              nix build                   │
///    │                   │                      │
///    │                   ▼                      │
///    │               BUILDING ──build done──► TRANSITIONING
///    │                   │                      │
///    │               error                      │
///    │                   │                      │
///    └───────────────► ERROR ◄──────────────────┘
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendStatus {
    /// Backend is stopped/unmounted
    ///
    /// This is the initial state before `mount()` is called,
    /// or after `unmount()` completes.
    Stopped,

    /// Composition view is consistent and ready for use
    ///
    /// All file operations will succeed immediately.
    Ready,

    /// Manifest file changed, preparing for update
    ///
    /// The backend has detected changes to dependency manifests
    /// (e.g., go-deps.toml, rust-deps.toml) and is preparing to
    /// rebuild affected cells.
    Updating {
        /// Which cells are being updated
        cells: Vec<String>,
    },

    /// Nix derivation is building
    ///
    /// One or more Nix builds are in progress. Reads to affected
    /// paths may block, return stale data, or fail depending on
    /// the configured `ConsistencyMode`.
    Building {
        /// Paths that are currently being rebuilt
        affected_paths: Vec<PathBuf>,
        /// Progress message (optional)
        message: Option<String>,
    },

    /// Atomically transitioning to new derivation
    ///
    /// The Nix build completed and the backend is switching to
    /// the new cell contents. This is a brief transient state.
    Transitioning,

    /// Backend encountered an error
    ///
    /// The composition view may be in an inconsistent state.
    /// Call `refresh()` to attempt recovery, or `unmount()` to
    /// stop the backend.
    Error {
        /// Error message describing what went wrong
        message: String,
        /// Whether the backend can attempt automatic recovery
        recoverable: bool,
    },
}

impl BackendStatus {
    /// Check if the backend is in a ready state
    pub fn is_ready(&self) -> bool {
        matches!(self, BackendStatus::Ready)
    }

    /// Check if the backend is stopped
    pub fn is_stopped(&self) -> bool {
        matches!(self, BackendStatus::Stopped)
    }

    /// Check if the backend is in an error state
    pub fn is_error(&self) -> bool {
        matches!(self, BackendStatus::Error { .. })
    }

    /// Check if the backend is currently updating
    pub fn is_updating(&self) -> bool {
        matches!(
            self,
            BackendStatus::Updating { .. }
                | BackendStatus::Building { .. }
                | BackendStatus::Transitioning
        )
    }

    /// Get a human-readable status message
    pub fn message(&self) -> &str {
        match self {
            BackendStatus::Stopped => "stopped",
            BackendStatus::Ready => "ready",
            BackendStatus::Updating { .. } => "updating",
            BackendStatus::Building { message, .. } => {
                message.as_deref().unwrap_or("building")
            }
            BackendStatus::Transitioning => "transitioning",
            BackendStatus::Error { message, .. } => message,
        }
    }
}

impl std::fmt::Display for BackendStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendStatus::Stopped => write!(f, "stopped"),
            BackendStatus::Ready => write!(f, "ready"),
            BackendStatus::Updating { cells } => {
                write!(f, "updating cells: {}", cells.join(", "))
            }
            BackendStatus::Building {
                affected_paths,
                message,
            } => {
                if let Some(msg) = message {
                    write!(f, "building: {}", msg)
                } else {
                    write!(f, "building {} paths", affected_paths.len())
                }
            }
            BackendStatus::Transitioning => write!(f, "transitioning"),
            BackendStatus::Error { message, .. } => write!(f, "error: {}", message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_checks() {
        assert!(BackendStatus::Ready.is_ready());
        assert!(!BackendStatus::Stopped.is_ready());

        assert!(BackendStatus::Stopped.is_stopped());
        assert!(!BackendStatus::Ready.is_stopped());

        assert!(BackendStatus::Error {
            message: "test".into(),
            recoverable: false
        }
        .is_error());

        assert!(BackendStatus::Updating {
            cells: vec!["godeps".into()]
        }
        .is_updating());
        assert!(BackendStatus::Building {
            affected_paths: vec![],
            message: None
        }
        .is_updating());
        assert!(BackendStatus::Transitioning.is_updating());
        assert!(!BackendStatus::Ready.is_updating());
    }

    #[test]
    fn test_status_display() {
        assert_eq!(BackendStatus::Ready.to_string(), "ready");
        assert_eq!(
            BackendStatus::Updating {
                cells: vec!["godeps".into(), "rustdeps".into()]
            }
            .to_string(),
            "updating cells: godeps, rustdeps"
        );
    }
}
