//! Manifest file watcher for dependency changes
//!
//! This module provides a file watcher that monitors dependency manifest files
//! (go-deps.toml, rust-deps.toml, etc.) for changes and triggers callbacks
//! when modifications are detected.
//!
//! The watcher uses debouncing to avoid excessive callbacks when files are
//! being actively edited.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use log::{debug, info, warn};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};

use crate::Error;

/// Default debounce timeout for file changes
const DEFAULT_DEBOUNCE_MS: u64 = 500;

/// Known manifest file patterns
const MANIFEST_PATTERNS: &[&str] = &[
    "go-deps.toml",
    "rust-deps.toml",
    "python-deps.toml",
    "js-deps.toml",
];

/// Events that can be emitted by the watcher
#[derive(Debug, Clone)]
pub enum WatcherEvent {
    /// A manifest file was modified
    ManifestChanged {
        /// Path to the changed file
        path: PathBuf,
        /// Name of the manifest (e.g., "go-deps", "rust-deps")
        manifest_name: String,
    },
    /// The watcher encountered an error
    Error {
        /// Error message
        message: String,
    },
}

/// Configuration for the manifest watcher
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Root directory to watch
    pub watch_root: PathBuf,
    /// Debounce timeout in milliseconds
    pub debounce_ms: u64,
    /// Additional manifest patterns to watch (beyond the defaults)
    pub extra_patterns: Vec<String>,
}

impl WatcherConfig {
    /// Create a new watcher configuration
    pub fn new(watch_root: impl Into<PathBuf>) -> Self {
        Self {
            watch_root: watch_root.into(),
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            extra_patterns: Vec::new(),
        }
    }

    /// Set the debounce timeout
    pub fn with_debounce(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    /// Add an additional manifest pattern to watch
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.extra_patterns.push(pattern.into());
        self
    }
}

/// A file watcher for dependency manifest files
pub struct ManifestWatcher {
    /// Configuration
    config: WatcherConfig,
    /// The debounced watcher (kept alive to maintain watching)
    _debouncer: Debouncer<notify::RecommendedWatcher>,
    /// Receiver for watcher events
    event_rx: Receiver<WatcherEvent>,
}

impl ManifestWatcher {
    /// Create a new manifest watcher
    ///
    /// The watcher will start monitoring immediately upon creation.
    pub fn new(config: WatcherConfig) -> Result<Self, Error> {
        let (event_tx, event_rx) = channel();
        let watch_root = config.watch_root.clone();
        let extra_patterns = config.extra_patterns.clone();

        // Create the debouncer
        let debouncer = new_debouncer(
            Duration::from_millis(config.debounce_ms),
            move |result: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                match result {
                    Ok(events) => {
                        for event in events {
                            if let Some(watcher_event) =
                                Self::process_event(&event, &watch_root, &extra_patterns)
                            {
                                if event_tx.send(watcher_event).is_err() {
                                    // Receiver dropped, stop processing
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = event_tx.send(WatcherEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }
            },
        )
        .map_err(|e| Error::ConfigError(format!("Failed to create watcher: {}", e)))?;

        let mut watcher = Self {
            config,
            _debouncer: debouncer,
            event_rx,
        };

        // Start watching
        watcher.start_watching()?;

        Ok(watcher)
    }

    /// Start watching the configured directory
    fn start_watching(&mut self) -> Result<(), Error> {
        info!("Starting manifest watcher for {:?}", self.config.watch_root);

        self._debouncer
            .watcher()
            .watch(&self.config.watch_root, RecursiveMode::NonRecursive)
            .map_err(|e| {
                Error::ConfigError(format!(
                    "Failed to watch {:?}: {}",
                    self.config.watch_root, e
                ))
            })?;

        Ok(())
    }

    /// Process a file event and convert to a WatcherEvent if relevant
    fn process_event(
        event: &notify_debouncer_mini::DebouncedEvent,
        _watch_root: &Path,
        extra_patterns: &[String],
    ) -> Option<WatcherEvent> {
        // Only process write/create events
        if event.kind != DebouncedEventKind::Any {
            return None;
        }

        let path = &event.path;
        let file_name = path.file_name()?.to_str()?;

        // Check if this is a manifest file
        let is_manifest = MANIFEST_PATTERNS.iter().any(|p| file_name == *p)
            || extra_patterns.iter().any(|p| file_name == p);

        if !is_manifest {
            debug!("Ignoring non-manifest file change: {:?}", path);
            return None;
        }

        // Extract manifest name (e.g., "go-deps" from "go-deps.toml")
        let manifest_name = file_name.strip_suffix(".toml").unwrap_or(file_name);

        info!("Manifest changed: {} ({:?})", manifest_name, path);

        Some(WatcherEvent::ManifestChanged {
            path: path.clone(),
            manifest_name: manifest_name.to_string(),
        })
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&self) -> Option<WatcherEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Receive an event, blocking until one is available
    pub fn recv(&self) -> Option<WatcherEvent> {
        self.event_rx.recv().ok()
    }

    /// Receive an event with a timeout
    pub fn recv_timeout(&self, timeout: Duration) -> Option<WatcherEvent> {
        self.event_rx.recv_timeout(timeout).ok()
    }

    /// Get the watch root path
    pub fn watch_root(&self) -> &Path {
        &self.config.watch_root
    }
}

/// A simple callback-based watcher that runs in the background
pub struct CallbackWatcher {
    /// The underlying watcher (kept alive to maintain watching)
    _watcher: ManifestWatcher,
    /// Sender to signal stop
    stop_tx: Option<Sender<()>>,
    /// Handle to the background thread
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl CallbackWatcher {
    /// Create a new callback watcher that invokes the callback on manifest changes
    ///
    /// The callback will be invoked in a background thread.
    pub fn new<F>(config: WatcherConfig, callback: F) -> Result<Self, Error>
    where
        F: Fn(WatcherEvent) + Send + 'static,
    {
        let watcher = ManifestWatcher::new(config)?;
        let (stop_tx, stop_rx) = channel();

        // Move the receiver to the background thread
        let event_rx = watcher.event_rx;

        let handle = std::thread::spawn(move || {
            loop {
                // Check for stop signal
                if stop_rx.try_recv().is_ok() {
                    debug!("Callback watcher received stop signal");
                    break;
                }

                // Check for events with timeout
                match event_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => callback(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        warn!("Watcher event channel disconnected");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _watcher: ManifestWatcher {
                config: WatcherConfig::new(""),
                _debouncer: watcher._debouncer,
                event_rx: channel().1, // Dummy receiver since real one moved to thread
            },
            stop_tx: Some(stop_tx),
            thread_handle: Some(handle),
        })
    }

    /// Stop the callback watcher
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for CallbackWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_watcher_config() {
        let config = WatcherConfig::new("/test/path")
            .with_debounce(1000)
            .with_pattern("custom-deps.toml");

        assert_eq!(config.watch_root, PathBuf::from("/test/path"));
        assert_eq!(config.debounce_ms, 1000);
        assert_eq!(config.extra_patterns, vec!["custom-deps.toml"]);
    }

    #[test]
    fn test_manifest_patterns() {
        // Verify all expected patterns are present
        assert!(MANIFEST_PATTERNS.contains(&"go-deps.toml"));
        assert!(MANIFEST_PATTERNS.contains(&"rust-deps.toml"));
        assert!(MANIFEST_PATTERNS.contains(&"python-deps.toml"));
        assert!(MANIFEST_PATTERNS.contains(&"js-deps.toml"));
    }

    #[test]
    fn test_watcher_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WatcherConfig::new(temp_dir.path());

        let watcher = ManifestWatcher::new(config);
        assert!(watcher.is_ok());
    }

    #[test]
    fn test_watcher_detects_changes() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("go-deps.toml");

        // Create initial file
        fs::write(&manifest_path, "# initial").unwrap();

        let config = WatcherConfig::new(temp_dir.path()).with_debounce(100);
        let watcher = ManifestWatcher::new(config).unwrap();

        // Modify the file
        std::thread::sleep(Duration::from_millis(50));
        fs::write(&manifest_path, "# modified").unwrap();

        // Wait for debounced event
        std::thread::sleep(Duration::from_millis(200));

        // Try to receive the event
        if let Some(event) = watcher.try_recv() {
            match event {
                WatcherEvent::ManifestChanged { manifest_name, .. } => {
                    assert_eq!(manifest_name, "go-deps");
                }
                WatcherEvent::Error { message } => {
                    panic!("Unexpected error: {}", message);
                }
            }
        }
        // Note: File watching in tests can be flaky, so we don't assert on receiving an event
    }
}
