//! Tracing and debugging infrastructure for composition backends
//!
//! This module provides comprehensive logging and debugging support:
//! - FUSE operation tracing with configurable verbosity
//! - State machine transition logging
//! - Performance metrics collection
//!
//! # Example
//!
//! ```ignore
//! use composition::tracing::{TracingConfig, StateLogger, Metrics};
//!
//! // Configure tracing
//! let config = TracingConfig::default()
//!     .with_fuse_ops(true)
//!     .with_state_transitions(true);
//!
//! // Create a state logger
//! let logger = StateLogger::new();
//!
//! // Record metrics
//! let metrics = Metrics::new();
//! metrics.record_fuse_op("getattr", Duration::from_micros(50));
//! println!("{}", metrics.summary());
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use log::{debug, info, trace, warn};

use crate::state::StateObserver;
use crate::BackendStatus;

/// Configuration for tracing and debugging
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Enable FUSE operation tracing
    pub trace_fuse_ops: bool,
    /// Enable state machine transition logging
    pub trace_state_transitions: bool,
    /// Enable performance metrics collection
    pub collect_metrics: bool,
    /// Log slow operations (threshold in microseconds)
    pub slow_op_threshold_us: u64,
    /// Sample rate for FUSE ops (1 = all, 10 = every 10th, etc.)
    pub fuse_sample_rate: u32,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            trace_fuse_ops: false,
            trace_state_transitions: true,
            collect_metrics: true,
            slow_op_threshold_us: 10_000, // 10ms
            fuse_sample_rate: 1,
        }
    }
}

impl TracingConfig {
    /// Create a new tracing config with all tracing enabled
    pub fn verbose() -> Self {
        Self {
            trace_fuse_ops: true,
            trace_state_transitions: true,
            collect_metrics: true,
            slow_op_threshold_us: 1_000, // 1ms
            fuse_sample_rate: 1,
        }
    }

    /// Create a minimal config (only errors and warnings)
    pub fn minimal() -> Self {
        Self {
            trace_fuse_ops: false,
            trace_state_transitions: false,
            collect_metrics: false,
            slow_op_threshold_us: 100_000, // 100ms
            fuse_sample_rate: 100,
        }
    }

    /// Enable FUSE operation tracing
    pub fn with_fuse_ops(mut self, enable: bool) -> Self {
        self.trace_fuse_ops = enable;
        self
    }

    /// Enable state transition logging
    pub fn with_state_transitions(mut self, enable: bool) -> Self {
        self.trace_state_transitions = enable;
        self
    }

    /// Enable metrics collection
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.collect_metrics = enable;
        self
    }

    /// Set slow operation threshold
    pub fn with_slow_threshold(mut self, threshold_us: u64) -> Self {
        self.slow_op_threshold_us = threshold_us;
        self
    }

    /// Set FUSE operation sample rate
    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.fuse_sample_rate = rate.max(1);
        self
    }
}

/// Logger for state machine transitions
///
/// Implements the `StateObserver` trait to receive notifications
/// about state changes and log them appropriately.
pub struct StateLogger {
    /// Whether to log transitions
    enabled: bool,
    /// Count of transitions logged
    transition_count: AtomicU64,
}

impl StateLogger {
    /// Create a new state logger
    pub fn new() -> Self {
        Self {
            enabled: true,
            transition_count: AtomicU64::new(0),
        }
    }

    /// Create a disabled state logger
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            transition_count: AtomicU64::new(0),
        }
    }

    /// Enable or disable logging
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get the number of transitions logged
    pub fn transition_count(&self) -> u64 {
        self.transition_count.load(Ordering::Relaxed)
    }
}

impl Default for StateLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl StateObserver for StateLogger {
    fn on_state_change(&self, old_status: &BackendStatus, new_status: &BackendStatus) {
        if !self.enabled {
            return;
        }

        self.transition_count.fetch_add(1, Ordering::Relaxed);

        match (old_status, new_status) {
            (BackendStatus::Stopped, BackendStatus::Ready) => {
                info!("State: Stopped → Ready (backend mounted and ready)");
            }
            (BackendStatus::Ready, BackendStatus::Updating { cells }) => {
                info!(
                    "State: Ready → Updating (cells: {})",
                    cells.join(", ")
                );
            }
            (BackendStatus::Updating { .. }, BackendStatus::Building { message, .. }) => {
                let msg = message.as_deref().unwrap_or("starting build");
                info!("State: Updating → Building ({})", msg);
            }
            (BackendStatus::Building { .. }, BackendStatus::Transitioning) => {
                info!("State: Building → Transitioning (applying updates)");
            }
            (BackendStatus::Transitioning, BackendStatus::Ready) => {
                info!("State: Transitioning → Ready (update complete)");
            }
            (_, BackendStatus::Error { message, recoverable }) => {
                if *recoverable {
                    warn!("State: → Error (recoverable): {}", message);
                } else {
                    warn!("State: → Error (non-recoverable): {}", message);
                }
            }
            (BackendStatus::Error { .. }, BackendStatus::Ready) => {
                info!("State: Error → Ready (recovered)");
            }
            (_, BackendStatus::Stopped) => {
                info!("State: → Stopped (backend unmounted)");
            }
            (old, new) => {
                debug!("State: {:?} → {:?}", old, new);
            }
        }
    }

    fn on_paths_affected(&self, paths: &[PathBuf]) {
        if !self.enabled {
            return;
        }

        if paths.is_empty() {
            return;
        }

        if paths.len() <= 3 {
            info!(
                "Paths affected by update: {}",
                paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        } else {
            info!(
                "Paths affected by update: {} (and {} more)",
                paths
                    .iter()
                    .take(3)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                paths.len() - 3
            );
        }
    }

    fn on_update_complete(&self, success: bool, duration: Duration) {
        if !self.enabled {
            return;
        }

        if success {
            info!("Update completed successfully in {:?}", duration);
        } else {
            warn!("Update failed after {:?}", duration);
        }
    }
}

/// Performance metrics for FUSE operations
///
/// Collects timing information for FUSE operations to help identify
/// performance bottlenecks.
pub struct Metrics {
    /// Operation counts
    op_counts: RwLock<HashMap<String, u64>>,
    /// Total duration per operation (microseconds)
    op_durations_us: RwLock<HashMap<String, u64>>,
    /// Maximum duration per operation (microseconds)
    op_max_us: RwLock<HashMap<String, u64>>,
    /// Count of slow operations
    slow_op_count: AtomicU64,
    /// Total operations recorded
    total_ops: AtomicU64,
    /// Creation time for calculating uptime
    created_at: Instant,
}

impl Metrics {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            op_counts: RwLock::new(HashMap::new()),
            op_durations_us: RwLock::new(HashMap::new()),
            op_max_us: RwLock::new(HashMap::new()),
            slow_op_count: AtomicU64::new(0),
            total_ops: AtomicU64::new(0),
            created_at: Instant::now(),
        }
    }

    /// Record a FUSE operation with its duration
    pub fn record_op(&self, op_name: &str, duration: Duration) {
        self.total_ops.fetch_add(1, Ordering::Relaxed);

        let duration_us = duration.as_micros() as u64;

        // Update counts
        {
            let mut counts = self.op_counts.write().unwrap();
            *counts.entry(op_name.to_string()).or_insert(0) += 1;
        }

        // Update total duration
        {
            let mut durations = self.op_durations_us.write().unwrap();
            *durations.entry(op_name.to_string()).or_insert(0) += duration_us;
        }

        // Update max duration
        {
            let mut maxes = self.op_max_us.write().unwrap();
            let entry = maxes.entry(op_name.to_string()).or_insert(0);
            if duration_us > *entry {
                *entry = duration_us;
            }
        }
    }

    /// Record a slow operation
    pub fn record_slow_op(&self) {
        self.slow_op_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get total operation count
    pub fn total_ops(&self) -> u64 {
        self.total_ops.load(Ordering::Relaxed)
    }

    /// Get slow operation count
    pub fn slow_ops(&self) -> u64 {
        self.slow_op_count.load(Ordering::Relaxed)
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get operation count for a specific operation
    pub fn op_count(&self, op_name: &str) -> u64 {
        self.op_counts
            .read()
            .unwrap()
            .get(op_name)
            .copied()
            .unwrap_or(0)
    }

    /// Get average duration for a specific operation (in microseconds)
    pub fn avg_duration_us(&self, op_name: &str) -> Option<u64> {
        let counts = self.op_counts.read().unwrap();
        let durations = self.op_durations_us.read().unwrap();

        let count = counts.get(op_name)?;
        let total = durations.get(op_name)?;

        if *count > 0 {
            Some(total / count)
        } else {
            None
        }
    }

    /// Get max duration for a specific operation (in microseconds)
    pub fn max_duration_us(&self, op_name: &str) -> Option<u64> {
        self.op_max_us.read().unwrap().get(op_name).copied()
    }

    /// Generate a summary of all metrics
    pub fn summary(&self) -> String {
        let mut summary = String::new();
        summary.push_str("=== FUSE Metrics Summary ===\n");
        summary.push_str(&format!("Uptime: {:?}\n", self.uptime()));
        summary.push_str(&format!("Total operations: {}\n", self.total_ops()));
        summary.push_str(&format!("Slow operations: {}\n", self.slow_ops()));
        summary.push('\n');

        let counts = self.op_counts.read().unwrap();
        let durations = self.op_durations_us.read().unwrap();
        let maxes = self.op_max_us.read().unwrap();

        // Sort by count descending
        let mut ops: Vec<_> = counts.keys().collect();
        ops.sort_by(|a, b| counts.get(*b).cmp(&counts.get(*a)));

        summary.push_str("Operation breakdown:\n");
        for op in ops {
            let count = counts.get(op).unwrap_or(&0);
            let total_us = durations.get(op).unwrap_or(&0);
            let max_us = maxes.get(op).unwrap_or(&0);
            let avg_us = if *count > 0 { total_us / count } else { 0 };

            summary.push_str(&format!(
                "  {}: {} calls, avg {}μs, max {}μs\n",
                op, count, avg_us, max_us
            ));
        }

        summary
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.op_counts.write().unwrap().clear();
        self.op_durations_us.write().unwrap().clear();
        self.op_max_us.write().unwrap().clear();
        self.slow_op_count.store(0, Ordering::Relaxed);
        self.total_ops.store(0, Ordering::Relaxed);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// FUSE operation tracer
///
/// Wraps FUSE operations with timing and logging.
pub struct FuseTracer {
    config: TracingConfig,
    metrics: Metrics,
    op_counter: AtomicU64,
}

impl FuseTracer {
    /// Create a new FUSE tracer with the given configuration
    pub fn new(config: TracingConfig) -> Self {
        Self {
            config,
            metrics: Metrics::new(),
            op_counter: AtomicU64::new(0),
        }
    }

    /// Create a tracer with default configuration
    pub fn with_defaults() -> Self {
        Self::new(TracingConfig::default())
    }

    /// Get the configuration
    pub fn config(&self) -> &TracingConfig {
        &self.config
    }

    /// Get the metrics collector
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Check if this operation should be traced (based on sample rate)
    fn should_trace(&self) -> bool {
        if !self.config.trace_fuse_ops {
            return false;
        }

        let count = self.op_counter.fetch_add(1, Ordering::Relaxed);
        count % (self.config.fuse_sample_rate as u64) == 0
    }

    /// Trace the start of a FUSE operation
    pub fn trace_start(&self, op_name: &str, context: &str) -> Option<Instant> {
        if self.should_trace() {
            trace!("FUSE {} start: {}", op_name, context);
            Some(Instant::now())
        } else if self.config.collect_metrics {
            Some(Instant::now())
        } else {
            None
        }
    }

    /// Trace the end of a FUSE operation
    pub fn trace_end(&self, op_name: &str, start: Option<Instant>, result: &str) {
        if let Some(start_time) = start {
            let duration = start_time.elapsed();
            let duration_us = duration.as_micros() as u64;

            // Collect metrics
            if self.config.collect_metrics {
                self.metrics.record_op(op_name, duration);

                if duration_us > self.config.slow_op_threshold_us {
                    self.metrics.record_slow_op();
                    warn!(
                        "FUSE {} slow: {} ({}μs > {}μs threshold)",
                        op_name, result, duration_us, self.config.slow_op_threshold_us
                    );
                }
            }

            // Log if tracing
            if self.config.trace_fuse_ops && self.op_counter.load(Ordering::Relaxed) % (self.config.fuse_sample_rate as u64) == 0 {
                trace!("FUSE {} end: {} ({}μs)", op_name, result, duration_us);
            }
        }
    }

    /// Get a summary of metrics
    pub fn summary(&self) -> String {
        self.metrics.summary()
    }
}

impl Default for FuseTracer {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Debug information collector
///
/// Collects various debug information about the composition backend
/// that can be displayed with `tk compose debug`.
#[derive(Debug, Default)]
pub struct DebugInfo {
    /// Backend type (fuse or symlink)
    pub backend_type: String,
    /// Current status
    pub status: String,
    /// Mount point
    pub mount_point: Option<String>,
    /// Repository root
    pub repo_root: Option<String>,
    /// Active cells
    pub cells: Vec<String>,
    /// Metrics summary (if available)
    pub metrics: Option<String>,
    /// State transition count
    pub state_transitions: u64,
    /// Uptime
    pub uptime: Option<Duration>,
    /// Additional info
    pub extra: HashMap<String, String>,
}

impl DebugInfo {
    /// Create a new debug info collector
    pub fn new() -> Self {
        Self::default()
    }

    /// Set backend type
    pub fn with_backend_type(mut self, backend_type: impl Into<String>) -> Self {
        self.backend_type = backend_type.into();
        self
    }

    /// Set status
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }

    /// Set mount point
    pub fn with_mount_point(mut self, mount_point: impl Into<String>) -> Self {
        self.mount_point = Some(mount_point.into());
        self
    }

    /// Set repository root
    pub fn with_repo_root(mut self, repo_root: impl Into<String>) -> Self {
        self.repo_root = Some(repo_root.into());
        self
    }

    /// Add a cell
    pub fn with_cell(mut self, cell: impl Into<String>) -> Self {
        self.cells.push(cell.into());
        self
    }

    /// Set metrics summary
    pub fn with_metrics(mut self, metrics: impl Into<String>) -> Self {
        self.metrics = Some(metrics.into());
        self
    }

    /// Set state transition count
    pub fn with_state_transitions(mut self, count: u64) -> Self {
        self.state_transitions = count;
        self
    }

    /// Set uptime
    pub fn with_uptime(mut self, uptime: Duration) -> Self {
        self.uptime = Some(uptime);
        self
    }

    /// Add extra info
    pub fn with_extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }

    /// Format as a human-readable string
    pub fn format(&self) -> String {
        let mut output = String::new();

        output.push_str("=== Composition Debug Info ===\n");
        output.push_str(&format!("Backend: {}\n", self.backend_type));
        output.push_str(&format!("Status: {}\n", self.status));

        if let Some(ref mp) = self.mount_point {
            output.push_str(&format!("Mount point: {}\n", mp));
        }
        if let Some(ref rr) = self.repo_root {
            output.push_str(&format!("Repo root: {}\n", rr));
        }
        if let Some(uptime) = self.uptime {
            output.push_str(&format!("Uptime: {:?}\n", uptime));
        }

        output.push_str(&format!("State transitions: {}\n", self.state_transitions));

        if !self.cells.is_empty() {
            output.push_str(&format!("Cells: {}\n", self.cells.join(", ")));
        }

        if !self.extra.is_empty() {
            output.push_str("\nAdditional info:\n");
            for (key, value) in &self.extra {
                output.push_str(&format!("  {}: {}\n", key, value));
            }
        }

        if let Some(ref metrics) = self.metrics {
            output.push('\n');
            output.push_str(metrics);
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_config_default() {
        let config = TracingConfig::default();
        assert!(!config.trace_fuse_ops);
        assert!(config.trace_state_transitions);
        assert!(config.collect_metrics);
    }

    #[test]
    fn test_tracing_config_verbose() {
        let config = TracingConfig::verbose();
        assert!(config.trace_fuse_ops);
        assert!(config.trace_state_transitions);
        assert!(config.collect_metrics);
    }

    #[test]
    fn test_tracing_config_minimal() {
        let config = TracingConfig::minimal();
        assert!(!config.trace_fuse_ops);
        assert!(!config.trace_state_transitions);
        assert!(!config.collect_metrics);
    }

    #[test]
    fn test_tracing_config_builder() {
        let config = TracingConfig::default()
            .with_fuse_ops(true)
            .with_state_transitions(false)
            .with_slow_threshold(5000)
            .with_sample_rate(10);

        assert!(config.trace_fuse_ops);
        assert!(!config.trace_state_transitions);
        assert_eq!(config.slow_op_threshold_us, 5000);
        assert_eq!(config.fuse_sample_rate, 10);
    }

    #[test]
    fn test_state_logger_new() {
        let logger = StateLogger::new();
        assert_eq!(logger.transition_count(), 0);
    }

    #[test]
    fn test_state_logger_disabled() {
        let logger = StateLogger::disabled();
        logger.on_state_change(&BackendStatus::Stopped, &BackendStatus::Ready);
        // Should not panic, but also not log
    }

    #[test]
    fn test_state_logger_counts_transitions() {
        let logger = StateLogger::new();
        logger.on_state_change(&BackendStatus::Stopped, &BackendStatus::Ready);
        logger.on_state_change(
            &BackendStatus::Ready,
            &BackendStatus::Updating {
                cells: vec!["godeps".into()],
            },
        );
        assert_eq!(logger.transition_count(), 2);
    }

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        assert_eq!(metrics.total_ops(), 0);
        assert_eq!(metrics.slow_ops(), 0);
    }

    #[test]
    fn test_metrics_record_op() {
        let metrics = Metrics::new();
        metrics.record_op("getattr", Duration::from_micros(100));
        metrics.record_op("getattr", Duration::from_micros(200));
        metrics.record_op("readdir", Duration::from_micros(500));

        assert_eq!(metrics.total_ops(), 3);
        assert_eq!(metrics.op_count("getattr"), 2);
        assert_eq!(metrics.op_count("readdir"), 1);
        assert_eq!(metrics.avg_duration_us("getattr"), Some(150));
        assert_eq!(metrics.max_duration_us("getattr"), Some(200));
    }

    #[test]
    fn test_metrics_summary() {
        let metrics = Metrics::new();
        metrics.record_op("getattr", Duration::from_micros(100));
        metrics.record_op("lookup", Duration::from_micros(200));

        let summary = metrics.summary();
        assert!(summary.contains("FUSE Metrics Summary"));
        assert!(summary.contains("getattr"));
        assert!(summary.contains("lookup"));
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = Metrics::new();
        metrics.record_op("getattr", Duration::from_micros(100));
        assert_eq!(metrics.total_ops(), 1);

        metrics.reset();
        assert_eq!(metrics.total_ops(), 0);
        assert_eq!(metrics.op_count("getattr"), 0);
    }

    #[test]
    fn test_fuse_tracer_new() {
        let tracer = FuseTracer::with_defaults();
        assert_eq!(tracer.metrics().total_ops(), 0);
    }

    #[test]
    fn test_fuse_tracer_trace() {
        let config = TracingConfig::default().with_metrics(true);
        let tracer = FuseTracer::new(config);

        let start = tracer.trace_start("getattr", "inode=1");
        std::thread::sleep(Duration::from_micros(10));
        tracer.trace_end("getattr", start, "success");

        assert_eq!(tracer.metrics().op_count("getattr"), 1);
    }

    #[test]
    fn test_debug_info_format() {
        let info = DebugInfo::new()
            .with_backend_type("fuse")
            .with_status("ready")
            .with_mount_point("/firefly/turnkey")
            .with_cell("godeps")
            .with_cell("rustdeps")
            .with_state_transitions(42)
            .with_extra("version", "0.1.0");

        let output = info.format();
        assert!(output.contains("Backend: fuse"));
        assert!(output.contains("Status: ready"));
        assert!(output.contains("Mount point: /firefly/turnkey"));
        assert!(output.contains("godeps"));
        assert!(output.contains("rustdeps"));
        assert!(output.contains("State transitions: 42"));
        assert!(output.contains("version: 0.1.0"));
    }
}
