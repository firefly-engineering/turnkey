//! Pluggable file access policy system
//!
//! This module provides a structured way to control file access based on:
//! - **File Class**: What category of file is being accessed
//! - **System State**: What the composition system is currently doing
//! - **Operation Type**: What kind of file operation is requested
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     FUSE Operation                          │
//! │  (lookup, getattr, read, readdir, write, create, ...)      │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Classify Request                          │
//! │  path/inode → FileClass                                     │
//! │  state machine → SystemState                                │
//! │  operation → OperationType                                  │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Policy.check()                            │
//! │  (FileClass, SystemState, OperationType) → PolicyDecision   │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Execute Decision                          │
//! │  Allow → proceed                                            │
//! │  Block → wait then retry                                    │
//! │  Deny → return errno                                        │
//! │  AllowStale → proceed with warning                          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use composition::policy::{AccessPolicy, FileClass, SystemState, OperationType, StrictPolicy};
//!
//! let policy = StrictPolicy::new();
//!
//! // Check if we can read cell content during a build
//! let decision = policy.check(
//!     FileClass::CellContent { cell: "godeps".into() },
//!     SystemState::Building,
//!     OperationType::Read,
//! );
//!
//! match decision {
//!     PolicyDecision::Allow => { /* proceed */ }
//!     PolicyDecision::Block { timeout } => { /* wait */ }
//!     PolicyDecision::Deny { errno } => { /* return error */ }
//!     PolicyDecision::AllowStale => { /* proceed with warning */ }
//! }
//! ```

use std::time::Duration;

// Standard POSIX errno values - defined here to avoid libc dependency
// when the fuse feature is disabled
/// Resource temporarily unavailable (POSIX EAGAIN)
pub const EAGAIN: i32 = 11;
/// Device or resource busy (POSIX EBUSY)
pub const EBUSY: i32 = 16;

/// Classification of files/paths in the composition view
///
/// Each class has different behavioral characteristics during
/// state transitions and updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileClass {
    /// Repository source files (passthrough to repo root)
    ///
    /// These are always accessible regardless of system state.
    /// Examples: src/main.rs, docs/README.md
    SourcePassthrough,

    /// Dependency cell content
    ///
    /// These may be affected during updates when the cell is being rebuilt.
    /// Access behavior depends on the configured policy.
    CellContent {
        /// Name of the cell (e.g., "godeps", "rustdeps")
        cell: String,
    },

    /// Generated virtual files
    ///
    /// Files like .buckconfig and .buckroot that are generated on-the-fly.
    /// Always accessible, content may change after transitions.
    VirtualGenerated,

    /// Virtual directory structure
    ///
    /// The mount root, cell prefix directory, etc.
    /// Always accessible for navigation.
    VirtualDirectory,

    /// Edit layer content (future)
    ///
    /// User modifications to dependency files, stored in an overlay.
    /// Write operations are always allowed; reads merge with base.
    #[allow(dead_code)]
    EditLayer {
        /// Name of the cell being edited
        cell: String,
    },
}

impl FileClass {
    /// Check if this class is always accessible regardless of state
    pub fn is_always_accessible(&self) -> bool {
        matches!(
            self,
            FileClass::SourcePassthrough
                | FileClass::VirtualGenerated
                | FileClass::VirtualDirectory
        )
    }

    /// Get the cell name if this is cell-related content
    pub fn cell_name(&self) -> Option<&str> {
        match self {
            FileClass::CellContent { cell } | FileClass::EditLayer { cell } => Some(cell),
            _ => None,
        }
    }
}

/// Current state of the composition system
///
/// The state machine transitions through these states during updates:
///
/// ```text
/// Settled ──manifest change──► Syncing ──nix build──► Building
///    ▲                                                    │
///    │                                               build done
///    │                                                    │
///    └───────────────────── Transitioning ◄───────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    /// System is stable, no pending changes
    ///
    /// All files are consistent and up-to-date.
    Settled,

    /// Manifest file changed, preparing for update
    ///
    /// The system detected changes to dependency manifests
    /// (e.g., go-deps.toml) and is preparing to rebuild.
    Syncing,

    /// Nix derivation is building
    ///
    /// One or more dependency cells are being rebuilt.
    /// Cell content may be stale or in flux.
    Building,

    /// Atomically transitioning to new view
    ///
    /// The build completed and the system is switching to
    /// the new cell contents. This is a brief transient state.
    Transitioning,

    /// System is in an error state
    ///
    /// Something failed during an update. The system may be
    /// in an inconsistent state until recovery.
    Error,
}

impl SystemState {
    /// Check if the system is in a stable state
    pub fn is_stable(&self) -> bool {
        matches!(self, SystemState::Settled)
    }

    /// Check if the system is actively updating
    pub fn is_updating(&self) -> bool {
        matches!(
            self,
            SystemState::Syncing | SystemState::Building | SystemState::Transitioning
        )
    }
}

/// Type of file operation being requested
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    /// Path lookup (FUSE lookup)
    Lookup,

    /// Get file/directory attributes (FUSE getattr)
    Getattr,

    /// Read file content (FUSE read)
    Read,

    /// Read directory entries (FUSE readdir)
    Readdir,

    /// Read symbolic link target (FUSE readlink)
    Readlink,

    /// Open file for reading (FUSE open)
    Open,

    /// Open directory (FUSE opendir)
    Opendir,

    /// Write file content (FUSE write) - future
    #[allow(dead_code)]
    Write,

    /// Create new file (FUSE create) - future
    #[allow(dead_code)]
    Create,

    /// Remove file (FUSE unlink) - future
    #[allow(dead_code)]
    Unlink,
}

impl OperationType {
    /// Check if this is a read-only operation
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            OperationType::Lookup
                | OperationType::Getattr
                | OperationType::Read
                | OperationType::Readdir
                | OperationType::Readlink
                | OperationType::Open
                | OperationType::Opendir
        )
    }
}

/// Decision returned by the access policy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Allow the operation to proceed immediately
    Allow,

    /// Block until the system reaches a stable state
    ///
    /// The operation will wait up to the specified timeout,
    /// then either proceed or fail depending on the policy.
    Block {
        /// Maximum time to wait for stable state
        timeout: Duration,
    },

    /// Deny the operation with an error
    Deny {
        /// Error number to return (e.g., EAGAIN, EBUSY)
        errno: i32,
    },

    /// Allow with potentially stale data
    ///
    /// The operation proceeds but may return outdated content.
    /// A warning is logged for observability.
    AllowStale,
}

impl PolicyDecision {
    /// Create a Block decision with default timeout (5 minutes)
    pub fn block() -> Self {
        PolicyDecision::Block {
            timeout: Duration::from_secs(300),
        }
    }

    /// Create a Block decision with custom timeout
    pub fn block_with_timeout(timeout: Duration) -> Self {
        PolicyDecision::Block { timeout }
    }

    /// Create a Deny decision with EAGAIN
    pub fn eagain() -> Self {
        PolicyDecision::Deny { errno: EAGAIN }
    }

    /// Create a Deny decision with EBUSY
    pub fn ebusy() -> Self {
        PolicyDecision::Deny { errno: EBUSY }
    }
}

/// Trait for implementing file access policies
///
/// Policies determine how file operations behave based on the
/// current system state and what type of file is being accessed.
///
/// # Thread Safety
///
/// Policies must be thread-safe as they're called from multiple
/// FUSE worker threads concurrently.
pub trait AccessPolicy: Send + Sync {
    /// Check if an operation should be allowed
    ///
    /// # Arguments
    ///
    /// * `class` - Classification of the file being accessed
    /// * `state` - Current system state
    /// * `op` - Type of operation being requested
    ///
    /// # Returns
    ///
    /// A decision indicating how to handle the operation.
    fn check(&self, class: &FileClass, state: SystemState, op: OperationType) -> PolicyDecision;

    /// Get a human-readable name for this policy
    fn name(&self) -> &'static str;

    /// Get a description of this policy's behavior
    fn description(&self) -> &'static str;
}

/// Strict policy: block all cell access during updates
///
/// This policy provides the strongest consistency guarantee:
/// reads will never return stale data, but may block for the
/// duration of the Nix build.
///
/// Use this policy when correctness is more important than latency.
#[derive(Debug, Clone, Default)]
pub struct StrictPolicy {
    /// Timeout for blocking operations
    pub block_timeout: Duration,
}

impl StrictPolicy {
    /// Create a new strict policy with default timeout
    pub fn new() -> Self {
        Self {
            block_timeout: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Create with custom timeout
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            block_timeout: timeout,
        }
    }
}

impl AccessPolicy for StrictPolicy {
    fn check(&self, class: &FileClass, state: SystemState, _op: OperationType) -> PolicyDecision {
        // Always-accessible classes proceed immediately
        if class.is_always_accessible() {
            return PolicyDecision::Allow;
        }

        // During stable state, allow everything
        if state.is_stable() {
            return PolicyDecision::Allow;
        }

        // During any update phase, block cell access
        PolicyDecision::Block {
            timeout: self.block_timeout,
        }
    }

    fn name(&self) -> &'static str {
        "strict"
    }

    fn description(&self) -> &'static str {
        "Block all cell access during updates for maximum consistency"
    }
}

/// Lenient policy: allow stale reads, only block during transition
///
/// This policy balances availability with consistency: reads during
/// syncing and building return potentially stale data (with a warning),
/// but reads during the brief transition phase are blocked.
///
/// Use this for interactive development where latency matters.
#[derive(Debug, Clone, Default)]
pub struct LenientPolicy {
    /// Timeout for blocking during transition
    pub transition_timeout: Duration,
}

impl LenientPolicy {
    /// Create a new lenient policy with default timeout
    pub fn new() -> Self {
        Self {
            transition_timeout: Duration::from_secs(30),
        }
    }
}

impl AccessPolicy for LenientPolicy {
    fn check(&self, class: &FileClass, state: SystemState, _op: OperationType) -> PolicyDecision {
        // Always-accessible classes proceed immediately
        if class.is_always_accessible() {
            return PolicyDecision::Allow;
        }

        match state {
            SystemState::Settled => PolicyDecision::Allow,
            SystemState::Syncing | SystemState::Building => PolicyDecision::AllowStale,
            SystemState::Transitioning => PolicyDecision::Block {
                timeout: self.transition_timeout,
            },
            SystemState::Error => PolicyDecision::AllowStale, // Degrade gracefully
        }
    }

    fn name(&self) -> &'static str {
        "lenient"
    }

    fn description(&self) -> &'static str {
        "Allow stale reads during updates, only block during transition"
    }
}

/// CI policy: fail fast with EAGAIN on any conflict
///
/// This policy never blocks - it immediately returns an error
/// if the requested operation would need to wait. The caller
/// can retry or handle the error appropriately.
///
/// Use this in CI/CD environments where blocking is undesirable.
#[derive(Debug, Clone, Default)]
pub struct CIPolicy;

impl CIPolicy {
    /// Create a new CI policy
    pub fn new() -> Self {
        Self
    }
}

impl AccessPolicy for CIPolicy {
    fn check(&self, class: &FileClass, state: SystemState, _op: OperationType) -> PolicyDecision {
        // Always-accessible classes proceed immediately
        if class.is_always_accessible() {
            return PolicyDecision::Allow;
        }

        // During stable state, allow everything
        if state.is_stable() {
            return PolicyDecision::Allow;
        }

        // During any update phase, fail immediately
        PolicyDecision::eagain()
    }

    fn name(&self) -> &'static str {
        "ci"
    }

    fn description(&self) -> &'static str {
        "Fail immediately with EAGAIN during updates (no blocking)"
    }
}

/// Development policy: balanced approach for interactive use
///
/// This is the default policy, providing a balance between
/// consistency and availability:
/// - Syncing: allow stale reads
/// - Building: block (the build should complete reasonably quickly)
/// - Transitioning: block (brief phase)
/// - Error: allow stale (degrade gracefully)
///
/// This gives good interactive behavior while ensuring that
/// reads during the build phase wait for fresh data.
#[derive(Debug, Clone, Default)]
pub struct DevelopmentPolicy {
    /// Timeout for blocking operations
    pub block_timeout: Duration,
}

impl DevelopmentPolicy {
    /// Create a new development policy with default timeout
    pub fn new() -> Self {
        Self {
            block_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl AccessPolicy for DevelopmentPolicy {
    fn check(&self, class: &FileClass, state: SystemState, _op: OperationType) -> PolicyDecision {
        // Always-accessible classes proceed immediately
        if class.is_always_accessible() {
            return PolicyDecision::Allow;
        }

        match state {
            SystemState::Settled => PolicyDecision::Allow,
            SystemState::Syncing => PolicyDecision::AllowStale, // Quick phase, stale is OK
            SystemState::Building | SystemState::Transitioning => PolicyDecision::Block {
                timeout: self.block_timeout,
            },
            SystemState::Error => PolicyDecision::AllowStale, // Degrade gracefully
        }
    }

    fn name(&self) -> &'static str {
        "development"
    }

    fn description(&self) -> &'static str {
        "Balanced policy: allow stale during sync, block during build"
    }
}

/// A boxed policy for dynamic dispatch
pub type BoxedPolicy = Box<dyn AccessPolicy>;

/// Create the default policy
pub fn default_policy() -> BoxedPolicy {
    Box::new(DevelopmentPolicy::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_class_always_accessible() {
        assert!(FileClass::SourcePassthrough.is_always_accessible());
        assert!(FileClass::VirtualGenerated.is_always_accessible());
        assert!(FileClass::VirtualDirectory.is_always_accessible());

        assert!(!FileClass::CellContent {
            cell: "godeps".into()
        }
        .is_always_accessible());
    }

    #[test]
    fn test_file_class_cell_name() {
        assert_eq!(
            FileClass::CellContent {
                cell: "godeps".into()
            }
            .cell_name(),
            Some("godeps")
        );
        assert_eq!(FileClass::SourcePassthrough.cell_name(), None);
    }

    #[test]
    fn test_system_state() {
        assert!(SystemState::Settled.is_stable());
        assert!(!SystemState::Building.is_stable());

        assert!(!SystemState::Settled.is_updating());
        assert!(SystemState::Building.is_updating());
        assert!(SystemState::Syncing.is_updating());
        assert!(SystemState::Transitioning.is_updating());
    }

    #[test]
    fn test_operation_type_read_only() {
        assert!(OperationType::Read.is_read_only());
        assert!(OperationType::Readdir.is_read_only());
        assert!(OperationType::Lookup.is_read_only());

        assert!(!OperationType::Write.is_read_only());
        assert!(!OperationType::Create.is_read_only());
    }

    #[test]
    fn test_strict_policy_settled() {
        let policy = StrictPolicy::new();
        let class = FileClass::CellContent {
            cell: "godeps".into(),
        };

        let decision = policy.check(&class, SystemState::Settled, OperationType::Read);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn test_strict_policy_building() {
        let policy = StrictPolicy::new();
        let class = FileClass::CellContent {
            cell: "godeps".into(),
        };

        let decision = policy.check(&class, SystemState::Building, OperationType::Read);
        assert!(matches!(decision, PolicyDecision::Block { .. }));
    }

    #[test]
    fn test_strict_policy_passthrough() {
        let policy = StrictPolicy::new();
        let class = FileClass::SourcePassthrough;

        // Source passthrough always allowed, even during builds
        let decision = policy.check(&class, SystemState::Building, OperationType::Read);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn test_lenient_policy_building() {
        let policy = LenientPolicy::new();
        let class = FileClass::CellContent {
            cell: "godeps".into(),
        };

        let decision = policy.check(&class, SystemState::Building, OperationType::Read);
        assert_eq!(decision, PolicyDecision::AllowStale);
    }

    #[test]
    fn test_lenient_policy_transitioning() {
        let policy = LenientPolicy::new();
        let class = FileClass::CellContent {
            cell: "godeps".into(),
        };

        let decision = policy.check(&class, SystemState::Transitioning, OperationType::Read);
        assert!(matches!(decision, PolicyDecision::Block { .. }));
    }

    #[test]
    fn test_ci_policy_building() {
        let policy = CIPolicy::new();
        let class = FileClass::CellContent {
            cell: "godeps".into(),
        };

        let decision = policy.check(&class, SystemState::Building, OperationType::Read);
        assert_eq!(decision, PolicyDecision::Deny { errno: EAGAIN });
    }

    #[test]
    fn test_development_policy() {
        let policy = DevelopmentPolicy::new();
        let class = FileClass::CellContent {
            cell: "godeps".into(),
        };

        // Syncing: allow stale
        let decision = policy.check(&class, SystemState::Syncing, OperationType::Read);
        assert_eq!(decision, PolicyDecision::AllowStale);

        // Building: block
        let decision = policy.check(&class, SystemState::Building, OperationType::Read);
        assert!(matches!(decision, PolicyDecision::Block { .. }));
    }

    #[test]
    fn test_policy_decision_helpers() {
        let block = PolicyDecision::block();
        assert!(matches!(block, PolicyDecision::Block { timeout } if timeout == Duration::from_secs(300)));

        let custom_block = PolicyDecision::block_with_timeout(Duration::from_secs(60));
        assert!(matches!(custom_block, PolicyDecision::Block { timeout } if timeout == Duration::from_secs(60)));

        let eagain = PolicyDecision::eagain();
        assert_eq!(eagain, PolicyDecision::Deny { errno: EAGAIN });

        let ebusy = PolicyDecision::ebusy();
        assert_eq!(ebusy, PolicyDecision::Deny { errno: EBUSY });
    }
}
