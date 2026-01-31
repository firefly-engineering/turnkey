//! Consistency state machine for composition backends
//!
//! This module provides a thread-safe state machine that manages the lifecycle
//! of dependency updates and tracks which paths are affected during transitions.
//!
//! # State Transitions
//!
//! ```text
//! STOPPED ──mount()──► READY ◄───────────────────────┐
//!    ▲                   │                           │
//!    │              trigger_update()                 │
//!    │                   │                           │
//!    │                   ▼                           │
//! unmount()          UPDATING                        │
//!    │                   │                           │
//!    │              start_build()                    │
//!    │                   │                           │
//!    │                   ▼                           │
//!    │               BUILDING ──build_complete()──► TRANSITIONING
//!    │                   │                           │
//!    │               build_failed()              transition_complete()
//!    │                   │                           │
//!    └───────────────► ERROR ◄───────────────────────┘
//! ```
//!
//! # Thread Safety
//!
//! The state machine uses `RwLock` for thread-safe access. Multiple readers
//! can check state concurrently, while state transitions require exclusive access.
//!
//! # Example
//!
//! ```ignore
//! use composition::state::ConsistencyStateMachine;
//! use std::time::Duration;
//!
//! let state_machine = ConsistencyStateMachine::new();
//!
//! // Mount and become ready
//! state_machine.set_ready()?;
//!
//! // Trigger an update for specific cells
//! state_machine.trigger_update(vec!["godeps".into()])?;
//!
//! // Start building with affected paths
//! state_machine.start_build(vec!["/external/godeps".into()])?;
//!
//! // Check if a path is affected
//! if state_machine.is_path_affected("/external/godeps/vendor/foo") {
//!     // Block or return stale data based on consistency mode
//! }
//!
//! // Complete the update
//! state_machine.build_complete()?;
//! state_machine.transition_complete()?;
//!
//! // Wait for ready state
//! state_machine.wait_ready(Some(Duration::from_secs(30)))?;
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, RwLock};
use std::time::{Duration, Instant};

use crate::{BackendStatus, Error, Result};

/// A thread-safe state machine for managing consistency during updates
///
/// This state machine tracks the current state of a composition backend
/// and which paths are being affected during updates. It provides methods
/// for state transitions and for blocking until the system is ready.
pub struct ConsistencyStateMachine {
    /// The current state (protected by RwLock for thread safety)
    inner: RwLock<StateMachineInner>,
    /// Condition variable for waiting on state changes
    /// Currently unused (polling-based waiting), kept for future condvar-based impl
    #[allow(dead_code)]
    state_changed: Condvar,
}

/// Inner state of the state machine
struct StateMachineInner {
    /// Current status
    status: BackendStatus,
    /// Cells currently being updated (when in Updating state)
    updating_cells: Vec<String>,
    /// Paths affected by the current update (when in Building state)
    affected_paths: HashSet<PathBuf>,
    /// When the current update started
    update_started: Option<Instant>,
    /// Progress message for builds
    build_message: Option<String>,
    /// Last error message (if any)
    last_error: Option<String>,
}

impl Default for StateMachineInner {
    fn default() -> Self {
        Self {
            status: BackendStatus::Stopped,
            updating_cells: Vec::new(),
            affected_paths: HashSet::new(),
            update_started: None,
            build_message: None,
            last_error: None,
        }
    }
}

impl ConsistencyStateMachine {
    /// Create a new state machine in the Stopped state
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(StateMachineInner::default()),
            state_changed: Condvar::new(),
        }
    }

    /// Get the current status
    pub fn status(&self) -> BackendStatus {
        let inner = self.inner.read().unwrap();
        inner.status.clone()
    }

    /// Get the cells being updated (if in Updating state)
    pub fn updating_cells(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        inner.updating_cells.clone()
    }

    /// Get the paths affected by the current update
    pub fn affected_paths(&self) -> HashSet<PathBuf> {
        let inner = self.inner.read().unwrap();
        inner.affected_paths.clone()
    }

    /// Check if a specific path is affected by the current update
    ///
    /// A path is considered affected if it starts with any of the
    /// affected path prefixes being tracked.
    pub fn is_path_affected(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let inner = self.inner.read().unwrap();

        // Only consider paths affected during Building or Transitioning states
        if !matches!(
            inner.status,
            BackendStatus::Building { .. } | BackendStatus::Transitioning
        ) {
            return false;
        }

        // Check if the path starts with any affected path prefix
        inner
            .affected_paths
            .iter()
            .any(|affected| path.starts_with(affected))
    }

    /// Get the duration since the update started (if updating)
    pub fn update_duration(&self) -> Option<Duration> {
        let inner = self.inner.read().unwrap();
        inner.update_started.map(|start| start.elapsed())
    }

    /// Transition to the Ready state
    ///
    /// This is typically called after mount() completes.
    pub fn set_ready(&self) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        // Valid transitions: Stopped -> Ready, Transitioning -> Ready
        match &inner.status {
            BackendStatus::Stopped | BackendStatus::Transitioning => {
                inner.status = BackendStatus::Ready;
                inner.updating_cells.clear();
                inner.affected_paths.clear();
                inner.update_started = None;
                inner.build_message = None;
                inner.last_error = None;
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            status => Err(Error::StateTransitionError(format!(
                "cannot transition to Ready from {:?}",
                status
            ))),
        }
    }

    /// Transition to the Stopped state
    ///
    /// This is typically called after unmount() completes.
    pub fn set_stopped(&self) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        // Can always transition to Stopped (cleanup)
        inner.status = BackendStatus::Stopped;
        inner.updating_cells.clear();
        inner.affected_paths.clear();
        inner.update_started = None;
        inner.build_message = None;
        drop(inner);
        self.notify_state_change();
        Ok(())
    }

    /// Trigger an update for the specified cells
    ///
    /// This transitions from Ready to Updating.
    pub fn trigger_update(&self, cells: Vec<String>) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        // Valid transition: Ready -> Updating
        match &inner.status {
            BackendStatus::Ready => {
                inner.status = BackendStatus::Updating {
                    cells: cells.clone(),
                };
                inner.updating_cells = cells;
                inner.update_started = Some(Instant::now());
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            BackendStatus::Updating { .. } | BackendStatus::Building { .. } => {
                // Already updating, merge the cells
                for cell in cells {
                    if !inner.updating_cells.contains(&cell) {
                        inner.updating_cells.push(cell);
                    }
                }
                // Update the status to reflect merged cells
                inner.status = BackendStatus::Updating {
                    cells: inner.updating_cells.clone(),
                };
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            status => Err(Error::StateTransitionError(format!(
                "cannot trigger update from {:?}",
                status
            ))),
        }
    }

    /// Start the build phase with the given affected paths
    ///
    /// This transitions from Updating to Building.
    pub fn start_build(&self, affected_paths: Vec<PathBuf>) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        // Valid transition: Updating -> Building
        match &inner.status {
            BackendStatus::Updating { .. } => {
                inner.affected_paths = affected_paths.iter().cloned().collect();
                inner.status = BackendStatus::Building {
                    affected_paths,
                    message: None,
                };
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            status => Err(Error::StateTransitionError(format!(
                "cannot start build from {:?}",
                status
            ))),
        }
    }

    /// Update the build progress message
    pub fn set_build_message(&self, message: Option<String>) {
        let mut inner = self.inner.write().unwrap();
        if let BackendStatus::Building { affected_paths, .. } = inner.status.clone() {
            inner.build_message = message.clone();
            inner.status = BackendStatus::Building {
                affected_paths,
                message,
            };
        }
    }

    /// Mark the build as complete, transitioning to Transitioning state
    ///
    /// This transitions from Building to Transitioning.
    pub fn build_complete(&self) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        // Valid transition: Building -> Transitioning
        match &inner.status {
            BackendStatus::Building { .. } => {
                inner.status = BackendStatus::Transitioning;
                inner.build_message = None;
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            status => Err(Error::StateTransitionError(format!(
                "cannot complete build from {:?}",
                status
            ))),
        }
    }

    /// Complete the transition, returning to Ready state
    ///
    /// This transitions from Transitioning to Ready.
    pub fn transition_complete(&self) -> Result<()> {
        self.set_ready()
    }

    /// Mark the build as failed with an error message
    ///
    /// This can be called from Updating or Building states.
    pub fn build_failed(&self, message: String, recoverable: bool) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        // Valid transitions: Updating -> Error, Building -> Error
        match &inner.status {
            BackendStatus::Updating { .. } | BackendStatus::Building { .. } => {
                inner.last_error = Some(message.clone());
                inner.status = BackendStatus::Error {
                    message,
                    recoverable,
                };
                inner.updating_cells.clear();
                inner.affected_paths.clear();
                inner.update_started = None;
                inner.build_message = None;
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            status => Err(Error::StateTransitionError(format!(
                "cannot fail build from {:?}",
                status
            ))),
        }
    }

    /// Attempt to recover from an error state
    ///
    /// If the error was marked as recoverable, transitions back to Ready.
    pub fn recover(&self) -> Result<()> {
        let mut inner = self.inner.write().unwrap();

        match &inner.status {
            BackendStatus::Error { recoverable, .. } if *recoverable => {
                inner.status = BackendStatus::Ready;
                inner.last_error = None;
                drop(inner);
                self.notify_state_change();
                Ok(())
            }
            BackendStatus::Error { recoverable, .. } if !recoverable => {
                Err(Error::StateTransitionError(
                    "error is not recoverable".into(),
                ))
            }
            status => Err(Error::StateTransitionError(format!(
                "cannot recover from {:?}",
                status
            ))),
        }
    }

    /// Get the last error message (if any)
    pub fn last_error(&self) -> Option<String> {
        let inner = self.inner.read().unwrap();
        inner.last_error.clone()
    }

    /// Wait for the state machine to reach the Ready state
    ///
    /// Blocks until the state becomes Ready, or returns an error if:
    /// - The timeout is reached
    /// - The state machine enters an error state
    pub fn wait_ready(&self, timeout: Option<Duration>) -> Result<()> {
        let start = Instant::now();

        let inner = self.inner.read().unwrap();
        let mut guard = inner;

        loop {
            match &guard.status {
                BackendStatus::Ready => return Ok(()),
                BackendStatus::Stopped => {
                    return Err(Error::NotMounted);
                }
                BackendStatus::Error { message, .. } => {
                    return Err(Error::StateTransitionError(message.clone()));
                }
                _ => {
                    // Still updating, wait for change
                    let remaining = timeout.map(|t| t.saturating_sub(start.elapsed()));

                    if remaining == Some(Duration::ZERO) {
                        return Err(Error::Timeout(timeout.unwrap()));
                    }

                    // Wait for state change (need to release and reacquire)
                    drop(guard);

                    // Use a short poll interval
                    std::thread::sleep(Duration::from_millis(10));

                    guard = self.inner.read().unwrap();
                }
            }
        }
    }

    /// Wait for the state machine to leave the updating states
    ///
    /// Returns when the state is no longer Updating, Building, or Transitioning.
    pub fn wait_not_updating(&self, timeout: Option<Duration>) -> Result<()> {
        let start = Instant::now();

        loop {
            let inner = self.inner.read().unwrap();
            let is_updating = matches!(
                &inner.status,
                BackendStatus::Updating { .. }
                    | BackendStatus::Building { .. }
                    | BackendStatus::Transitioning
            );
            drop(inner);

            if !is_updating {
                return Ok(());
            }

            // Check timeout
            if let Some(t) = timeout {
                if start.elapsed() >= t {
                    return Err(Error::Timeout(t));
                }
            }

            // Short poll interval
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Notify waiters that the state has changed
    fn notify_state_change(&self) {
        // The Condvar requires a MutexGuard, but we use RwLock
        // So we just rely on polling for now
        // Future: Could use a separate Mutex for the condvar
    }
}

impl Default for ConsistencyStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Observer trait for state machine changes
///
/// Implement this trait to receive notifications when the state machine
/// transitions between states.
pub trait StateObserver: Send + Sync {
    /// Called when the state changes
    fn on_state_change(&self, old_status: &BackendStatus, new_status: &BackendStatus);

    /// Called when paths become affected by an update
    fn on_paths_affected(&self, paths: &[PathBuf]);

    /// Called when an update completes (successfully or with error)
    fn on_update_complete(&self, success: bool, duration: Duration);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let sm = ConsistencyStateMachine::new();
        assert!(matches!(sm.status(), BackendStatus::Stopped));
    }

    #[test]
    fn test_stopped_to_ready() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();
        assert!(matches!(sm.status(), BackendStatus::Ready));
    }

    #[test]
    fn test_full_update_cycle() {
        let sm = ConsistencyStateMachine::new();

        // Start in Ready state
        sm.set_ready().unwrap();
        assert!(matches!(sm.status(), BackendStatus::Ready));

        // Trigger update
        sm.trigger_update(vec!["godeps".into()]).unwrap();
        assert!(matches!(sm.status(), BackendStatus::Updating { .. }));
        assert_eq!(sm.updating_cells(), vec!["godeps"]);

        // Start build
        let paths = vec![PathBuf::from("/external/godeps")];
        sm.start_build(paths.clone()).unwrap();
        assert!(matches!(sm.status(), BackendStatus::Building { .. }));
        assert_eq!(sm.affected_paths().len(), 1);

        // Check path affected
        assert!(sm.is_path_affected("/external/godeps/vendor/foo"));
        assert!(!sm.is_path_affected("/external/rustdeps/vendor/bar"));

        // Complete build
        sm.build_complete().unwrap();
        assert!(matches!(sm.status(), BackendStatus::Transitioning));

        // Complete transition
        sm.transition_complete().unwrap();
        assert!(matches!(sm.status(), BackendStatus::Ready));

        // Paths no longer affected
        assert!(!sm.is_path_affected("/external/godeps/vendor/foo"));
    }

    #[test]
    fn test_merge_updates() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();

        // First update
        sm.trigger_update(vec!["godeps".into()]).unwrap();
        assert_eq!(sm.updating_cells(), vec!["godeps"]);

        // Merge second update
        sm.trigger_update(vec!["rustdeps".into()]).unwrap();
        let cells = sm.updating_cells();
        assert!(cells.contains(&"godeps".into()));
        assert!(cells.contains(&"rustdeps".into()));
    }

    #[test]
    fn test_build_failure() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();
        sm.trigger_update(vec!["godeps".into()]).unwrap();
        sm.start_build(vec![PathBuf::from("/external/godeps")])
            .unwrap();

        // Fail the build
        sm.build_failed("nix build failed".into(), true).unwrap();
        assert!(matches!(sm.status(), BackendStatus::Error { .. }));
        assert_eq!(sm.last_error(), Some("nix build failed".into()));

        // Recover
        sm.recover().unwrap();
        assert!(matches!(sm.status(), BackendStatus::Ready));
    }

    #[test]
    fn test_non_recoverable_error() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();
        sm.trigger_update(vec!["godeps".into()]).unwrap();

        // Fail with non-recoverable error
        sm.build_failed("critical error".into(), false).unwrap();

        // Cannot recover
        assert!(sm.recover().is_err());
    }

    #[test]
    fn test_invalid_transitions() {
        let sm = ConsistencyStateMachine::new();

        // Cannot trigger update from Stopped
        assert!(sm.trigger_update(vec!["godeps".into()]).is_err());

        // Cannot start build from Stopped
        assert!(sm.start_build(vec![]).is_err());

        // Cannot complete build from Stopped
        assert!(sm.build_complete().is_err());

        sm.set_ready().unwrap();

        // Cannot start build from Ready (must trigger update first)
        assert!(sm.start_build(vec![]).is_err());

        // Cannot complete build from Ready
        assert!(sm.build_complete().is_err());
    }

    #[test]
    fn test_update_duration() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();

        // No duration before update
        assert!(sm.update_duration().is_none());

        // Start update
        sm.trigger_update(vec!["godeps".into()]).unwrap();

        // Duration should be some small value
        std::thread::sleep(Duration::from_millis(10));
        let duration = sm.update_duration().unwrap();
        assert!(duration >= Duration::from_millis(10));
    }

    #[test]
    fn test_wait_ready_immediate() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();

        // Should return immediately
        let result = sm.wait_ready(Some(Duration::from_millis(100)));
        assert!(result.is_ok());
    }

    #[test]
    fn test_wait_ready_stopped() {
        let sm = ConsistencyStateMachine::new();

        // Should fail because stopped
        let result = sm.wait_ready(Some(Duration::from_millis(100)));
        assert!(matches!(result, Err(Error::NotMounted)));
    }

    #[test]
    fn test_set_build_message() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();
        sm.trigger_update(vec!["godeps".into()]).unwrap();
        sm.start_build(vec![PathBuf::from("/external/godeps")])
            .unwrap();

        // Set message
        sm.set_build_message(Some("Building godeps...".into()));

        // Check message is in status
        if let BackendStatus::Building { message, .. } = sm.status() {
            assert_eq!(message, Some("Building godeps...".into()));
        } else {
            panic!("Expected Building status");
        }
    }
}
