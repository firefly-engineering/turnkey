//! Integration tests for the composition crate
//!
//! These tests verify the behavior of both symlink and FUSE backends
//! across different platforms.
//!
//! # Running Tests
//!
//! Basic tests (symlink backend):
//! ```bash
//! cargo test -p composition
//! ```
//!
//! FUSE tests (requires FUSE to be available):
//! ```bash
//! cargo test -p composition --features fuse -- --ignored
//! ```
//!
//! # Platform Support
//!
//! - Linux: Native FUSE via /dev/fuse
//! - macOS: macFUSE 5.2+ (FSKit on macOS 26+) — FUSE-T also works at the
//!   libfuse3 ABI level but is no longer the project's default.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use composition::{
    BackendStatus, CellConfig, CompositionBackend, CompositionConfig, SymlinkBackend,
};
use tempfile::TempDir;

/// Helper to set up a test environment with mock cells
struct TestEnv {
    /// Temporary directory for the mount point
    #[allow(dead_code)]
    mount_dir: TempDir,
    /// Temporary directory for cell sources
    #[allow(dead_code)]
    source_dir: TempDir,
    /// Path to the mount point
    mount_point: PathBuf,
    /// Paths to cell sources
    cell_sources: Vec<(String, PathBuf)>,
}

impl TestEnv {
    /// Create a new test environment
    fn new() -> Self {
        let mount_dir = TempDir::new().expect("Failed to create temp mount dir");
        let source_dir = TempDir::new().expect("Failed to create temp source dir");

        let mount_point = mount_dir.path().join("composition");

        Self {
            mount_dir,
            source_dir,
            mount_point,
            cell_sources: Vec::new(),
        }
    }

    /// Add a cell with sample content
    fn with_cell(mut self, name: &str, files: &[(&str, &str)]) -> Self {
        let cell_path = self.source_dir.path().join(name);
        fs::create_dir_all(&cell_path).expect("Failed to create cell directory");

        for (file_name, content) in files {
            let file_path = cell_path.join(file_name);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            let mut file = File::create(&file_path).expect("Failed to create file");
            file.write_all(content.as_bytes())
                .expect("Failed to write content");
        }

        self.cell_sources.push((name.to_string(), cell_path));
        self
    }

    /// Build a CompositionConfig from this environment
    fn config(&self) -> CompositionConfig {
        let mut config = CompositionConfig::new(&self.mount_point, self.source_dir.path());

        for (name, path) in &self.cell_sources {
            config = config.with_cell(CellConfig::new(name, path));
        }

        config
    }
}

// ============================================================================
// Symlink Backend Tests
// ============================================================================

mod symlink_tests {
    use super::*;

    #[test]
    fn test_symlink_backend_lifecycle() {
        let env = TestEnv::new().with_cell("godeps", &[("vendor/test.txt", "hello world")]);

        let config = env.config();
        let mut backend = SymlinkBackend::new(config);

        // Initial state
        assert!(matches!(backend.status(), BackendStatus::Stopped));
        assert!(!backend.is_mounted());
        assert!(!backend.is_ready());

        // Mount
        backend.mount().expect("Mount should succeed");
        assert!(matches!(backend.status(), BackendStatus::Ready));
        assert!(backend.is_mounted());
        assert!(backend.is_ready());

        // Wait ready should return immediately
        backend
            .wait_ready(Some(Duration::from_secs(1)))
            .expect("Wait ready should succeed");

        // Unmount
        backend.unmount().expect("Unmount should succeed");
        assert!(matches!(backend.status(), BackendStatus::Stopped));
        assert!(!backend.is_mounted());
    }

    #[test]
    fn test_symlink_double_mount_fails() {
        let env = TestEnv::new().with_cell("godeps", &[("test.txt", "content")]);

        let config = env.config();
        let mut backend = SymlinkBackend::new(config);

        backend.mount().expect("First mount should succeed");
        let result = backend.mount();
        assert!(result.is_err(), "Second mount should fail");
    }

    #[test]
    fn test_symlink_double_unmount_fails() {
        let env = TestEnv::new().with_cell("godeps", &[("test.txt", "content")]);

        let config = env.config();
        let mut backend = SymlinkBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend.unmount().expect("First unmount should succeed");
        let result = backend.unmount();
        assert!(result.is_err(), "Second unmount should fail");
    }

    #[test]
    fn test_symlink_cell_path() {
        let env = TestEnv::new()
            .with_cell("godeps", &[("test.txt", "content")])
            .with_cell("rustdeps", &[("Cargo.toml", "[package]")]);

        let config = env.config();
        let backend = SymlinkBackend::new(config);

        assert!(backend.cell_path("godeps").is_some());
        assert!(backend.cell_path("rustdeps").is_some());
        assert!(backend.cell_path("nonexistent").is_none());
    }

    #[test]
    fn test_symlink_cell_mappings() {
        let env = TestEnv::new()
            .with_cell("godeps", &[("test.txt", "content")])
            .with_cell("rustdeps", &[("Cargo.toml", "[package]")]);

        let config = env.config();
        let backend = SymlinkBackend::new(config);

        let mappings = backend.cell_mappings();
        assert_eq!(mappings.len(), 2);

        let names: Vec<&str> = mappings.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"godeps"));
        assert!(names.contains(&"rustdeps"));
    }

    #[test]
    fn test_symlink_file_access() {
        let env = TestEnv::new().with_cell(
            "godeps",
            &[
                ("vendor/test.txt", "hello world"),
                ("vendor/nested/file.txt", "nested content"),
            ],
        );

        let config = env.config();
        let mut backend = SymlinkBackend::new(config);

        backend.mount().expect("Mount should succeed");

        // Read file through symlink
        let cell_path = backend.cell_path("godeps").unwrap();
        let test_file = cell_path.join("vendor/test.txt");

        assert!(
            test_file.exists(),
            "File should be accessible through symlink"
        );

        let mut content = String::new();
        File::open(&test_file)
            .expect("Should open file")
            .read_to_string(&mut content)
            .expect("Should read content");

        assert_eq!(content, "hello world");

        // Check nested file
        let nested_file = cell_path.join("vendor/nested/file.txt");
        assert!(nested_file.exists(), "Nested file should be accessible");

        backend.unmount().ok();
    }

    #[test]
    fn test_symlink_refresh() {
        let env = TestEnv::new().with_cell("godeps", &[("test.txt", "original")]);

        let config = env.config();
        let mut backend = SymlinkBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend.refresh().expect("Refresh should succeed");

        // Symlinks should still work after refresh
        let cell_path = backend.cell_path("godeps").unwrap();
        assert!(cell_path.join("test.txt").exists());

        backend.unmount().ok();
    }

    #[test]
    fn test_symlink_missing_source() {
        let mount_dir = TempDir::new().unwrap();
        let mount_point = mount_dir.path().join("composition");

        let config = CompositionConfig::new(&mount_point, "/tmp")
            .with_cell(CellConfig::new("nonexistent", "/path/that/does/not/exist"));

        let mut backend = SymlinkBackend::new(config);
        let result = backend.mount();

        assert!(result.is_err(), "Mount should fail for missing source");
    }
}

// ============================================================================
// FUSE Backend Tests (Feature-Gated)
// ============================================================================

#[cfg(feature = "fuse")]
mod fuse_tests {
    use super::*;
    use composition::fuse::FuseBackend;
    use composition::selector::{is_fuse_available, select_backend, BackendType};

    /// Check if FUSE is available for testing
    fn fuse_available() -> bool {
        is_fuse_available()
    }

    #[test]
    fn test_fuse_availability_check() {
        // This should not panic regardless of FUSE availability
        let available = is_fuse_available();
        println!("FUSE available: {}", available);
    }

    #[test]
    fn test_backend_selection_auto() {
        let selection = select_backend(BackendType::Auto);

        // Auto should succeed on any platform
        println!("Selected backend: {:?}", selection.backend_type);
        println!("Reason: {}", selection.reason);
    }

    #[test]
    fn test_backend_selection_explicit_symlink() {
        let selection = select_backend(BackendType::Symlink);

        assert_eq!(selection.backend_type, BackendType::Symlink);
    }

    #[test]
    fn test_backend_selection_explicit_fuse() {
        let selection = select_backend(BackendType::Fuse);

        // Should always return Fuse when explicitly requested
        // (create_backend would fail if not available)
        assert_eq!(selection.backend_type, BackendType::Fuse);
    }

    // Integration tests that require actual FUSE mount
    // These are marked #[ignore] and run with --ignored flag

    #[test]
    #[ignore]
    fn test_fuse_backend_lifecycle() {
        if !fuse_available() {
            println!("Skipping: FUSE not available");
            return;
        }

        let env = TestEnv::new().with_cell("godeps", &[("vendor/test.txt", "hello world")]);

        let config = env.config();
        let mut backend = FuseBackend::new(config);

        // Initial state
        assert!(matches!(backend.status(), BackendStatus::Stopped));
        assert!(!backend.is_mounted());

        // Mount
        backend.mount().expect("FUSE mount should succeed");

        // Wait for ready
        backend
            .wait_ready(Some(Duration::from_secs(5)))
            .expect("Should become ready");

        assert!(backend.is_mounted());
        assert!(backend.is_ready());

        // Unmount
        backend.unmount().expect("FUSE unmount should succeed");
        assert!(!backend.is_mounted());
    }

    #[test]
    #[ignore]
    fn test_fuse_file_operations() {
        if !fuse_available() {
            println!("Skipping: FUSE not available");
            return;
        }

        let env = TestEnv::new().with_cell(
            "godeps",
            &[
                ("vendor/test.txt", "hello fuse"),
                ("vendor/nested/deep/file.txt", "deeply nested"),
            ],
        );

        let config = env.config();
        let mut backend = FuseBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend
            .wait_ready(Some(Duration::from_secs(5)))
            .expect("Should become ready");

        // Read file through FUSE
        let cell_path = backend.cell_path("godeps").unwrap();
        let test_file = cell_path.join("vendor/test.txt");

        // Give FUSE a moment to initialize
        std::thread::sleep(Duration::from_millis(100));

        assert!(test_file.exists(), "File should be accessible through FUSE");

        let mut content = String::new();
        File::open(&test_file)
            .expect("Should open file through FUSE")
            .read_to_string(&mut content)
            .expect("Should read content through FUSE");

        assert_eq!(content, "hello fuse");

        // Test directory listing
        let vendor_dir = cell_path.join("vendor");
        let entries: Vec<_> = fs::read_dir(&vendor_dir)
            .expect("Should list directory")
            .filter_map(|e| e.ok())
            .collect();

        assert!(!entries.is_empty(), "Should have directory entries");

        backend.unmount().ok();
    }

    #[test]
    #[ignore]
    fn test_fuse_readdir() {
        if !fuse_available() {
            println!("Skipping: FUSE not available");
            return;
        }

        let env = TestEnv::new().with_cell(
            "godeps",
            &[
                ("file1.txt", "content1"),
                ("file2.txt", "content2"),
                ("subdir/file3.txt", "content3"),
            ],
        );

        let config = env.config();
        let mut backend = FuseBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend.wait_ready(Some(Duration::from_secs(5))).ok();

        let cell_path = backend.cell_path("godeps").unwrap();

        // List root of cell
        let entries: Vec<String> = fs::read_dir(&cell_path)
            .expect("Should list cell root")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(entries.contains(&"file1.txt".to_string()));
        assert!(entries.contains(&"file2.txt".to_string()));
        assert!(entries.contains(&"subdir".to_string()));

        backend.unmount().ok();
    }

    #[test]
    #[ignore]
    fn test_fuse_getattr() {
        if !fuse_available() {
            println!("Skipping: FUSE not available");
            return;
        }

        let env = TestEnv::new().with_cell("godeps", &[("test.txt", "12345")]);

        let config = env.config();
        let mut backend = FuseBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend.wait_ready(Some(Duration::from_secs(5))).ok();

        let cell_path = backend.cell_path("godeps").unwrap();
        let test_file = cell_path.join("test.txt");

        let metadata = fs::metadata(&test_file).expect("Should get file metadata");

        assert!(metadata.is_file());
        assert_eq!(metadata.len(), 5); // "12345" is 5 bytes

        backend.unmount().ok();
    }

    #[test]
    #[ignore]
    fn test_fuse_multiple_cells() {
        if !fuse_available() {
            println!("Skipping: FUSE not available");
            return;
        }

        let env = TestEnv::new()
            .with_cell("godeps", &[("go.txt", "go content")])
            .with_cell("rustdeps", &[("rust.txt", "rust content")])
            .with_cell("pydeps", &[("py.txt", "python content")]);

        let config = env.config();
        let mut backend = FuseBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend.wait_ready(Some(Duration::from_secs(5))).ok();

        // Verify all cells are accessible
        for (name, expected_file, expected_content) in [
            ("godeps", "go.txt", "go content"),
            ("rustdeps", "rust.txt", "rust content"),
            ("pydeps", "py.txt", "python content"),
        ] {
            let cell_path = backend
                .cell_path(name)
                .unwrap_or_else(|| panic!("Cell {} should exist", name));
            let file_path = cell_path.join(expected_file);

            let mut content = String::new();
            File::open(&file_path)
                .unwrap_or_else(|_| panic!("Should open {} in {}", expected_file, name))
                .read_to_string(&mut content)
                .expect("Should read content");

            assert_eq!(content, expected_content);
        }

        backend.unmount().ok();
    }

    #[test]
    #[ignore]
    fn test_fuse_refresh() {
        if !fuse_available() {
            println!("Skipping: FUSE not available");
            return;
        }

        let env = TestEnv::new().with_cell("godeps", &[("test.txt", "original")]);

        let config = env.config();
        let mut backend = FuseBackend::new(config);

        backend.mount().expect("Mount should succeed");
        backend.wait_ready(Some(Duration::from_secs(5))).ok();

        // Refresh should not error
        backend.refresh().expect("Refresh should succeed");

        // Files should still be accessible
        let cell_path = backend.cell_path("godeps").unwrap();
        assert!(cell_path.join("test.txt").exists());

        backend.unmount().ok();
    }
}

// ============================================================================
// Platform-Specific Tests
// ============================================================================

mod platform_tests {
    use composition::selector::{fuse_install_instructions, is_fuse_available};

    #[test]
    fn test_fuse_install_instructions() {
        let instructions = fuse_install_instructions();

        // Instructions are Some when FUSE is not available or not compiled
        if let Some(text) = instructions {
            #[cfg(target_os = "linux")]
            assert!(text.contains("apt") || text.contains("dnf") || text.contains("fuse"));

            #[cfg(target_os = "macos")]
            assert!(text.contains("brew") || text.contains("fuse"));

            // Non-FUSE builds should mention rebuilding
            #[cfg(not(feature = "fuse"))]
            assert!(text.contains("compiled") || text.contains("feature"));
        }
    }

    #[test]
    fn test_fuse_availability_consistent() {
        // Multiple calls should return the same result
        let first = is_fuse_available();
        let second = is_fuse_available();
        assert_eq!(first, second, "FUSE availability should be consistent");
    }
}

// ============================================================================
// State Machine Tests
// ============================================================================

mod state_tests {
    use composition::state::ConsistencyStateMachine;
    use composition::BackendStatus;
    use std::path::PathBuf;

    #[test]
    fn test_state_machine_initial_state() {
        let sm = ConsistencyStateMachine::new();
        assert!(matches!(sm.status(), BackendStatus::Stopped));
    }

    #[test]
    fn test_state_machine_transitions() {
        let sm = ConsistencyStateMachine::new();

        // Stopped -> Ready
        sm.set_ready().expect("set_ready should succeed");
        assert!(matches!(sm.status(), BackendStatus::Ready));

        // Ready -> Updating
        sm.trigger_update(vec!["godeps".into()])
            .expect("trigger_update should succeed");
        assert!(matches!(sm.status(), BackendStatus::Updating { .. }));

        // Updating -> Building
        sm.start_build(vec![PathBuf::from("/external/godeps")])
            .expect("start_build should succeed");
        assert!(matches!(sm.status(), BackendStatus::Building { .. }));

        // Building -> Transitioning
        sm.build_complete().expect("build_complete should succeed");
        assert!(matches!(sm.status(), BackendStatus::Transitioning));

        // Transitioning -> Ready
        sm.transition_complete()
            .expect("transition_complete should succeed");
        assert!(matches!(sm.status(), BackendStatus::Ready));
    }

    #[test]
    fn test_state_machine_path_affected() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();
        sm.trigger_update(vec!["godeps".into()]).unwrap();
        sm.start_build(vec![PathBuf::from("/external/godeps")])
            .unwrap();

        // Path should be affected during Building
        assert!(sm.is_path_affected("/external/godeps/vendor/foo"));
        assert!(!sm.is_path_affected("/external/rustdeps/vendor/bar"));

        // Complete the transition
        sm.build_complete().unwrap();
        sm.transition_complete().unwrap();

        // Path should no longer be affected
        assert!(!sm.is_path_affected("/external/godeps/vendor/foo"));
    }

    #[test]
    fn test_state_machine_build_failure() {
        let sm = ConsistencyStateMachine::new();
        sm.set_ready().unwrap();
        sm.trigger_update(vec!["godeps".into()]).unwrap();
        sm.start_build(vec![PathBuf::from("/external/godeps")])
            .unwrap();

        // Fail the build
        sm.build_failed("nix build failed".into(), true).unwrap();
        assert!(matches!(sm.status(), BackendStatus::Error { .. }));

        // Recover
        sm.recover().unwrap();
        assert!(matches!(sm.status(), BackendStatus::Ready));
    }
}

// ============================================================================
// Recovery Tests
// ============================================================================

mod recovery_tests {
    use composition::recovery::{is_transient_error, recovery_suggestion, RetryConfig};
    use composition::Error;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn test_transient_error_classification() {
        // Transient errors
        assert!(is_transient_error(&Error::Timeout(Duration::from_secs(1))));
        assert!(is_transient_error(&Error::PathUpdating(PathBuf::from(
            "/test"
        ))));

        // Non-transient errors
        assert!(!is_transient_error(&Error::NotMounted));
        assert!(!is_transient_error(&Error::CellNotFound("test".into())));
        assert!(!is_transient_error(&Error::ConfigError("test".into())));
    }

    #[test]
    fn test_recovery_suggestions() {
        let error = Error::NotMounted;
        let suggestion = recovery_suggestion(&error);
        assert!(suggestion.is_some());

        let error = Error::FuseUnavailable("test".into());
        let suggestion = recovery_suggestion(&error);
        assert!(suggestion.is_some());
    }

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();
        assert!(config.max_attempts > 0);
        assert!(config.initial_delay > Duration::ZERO);
    }
}

// ============================================================================
// Layout Tests
// ============================================================================

mod layout_tests {
    use composition::layout::{
        available_layouts, default_layout, global_registry, layout_by_name, Buck2Layout,
        LayoutContext,
    };
    use std::path::PathBuf;

    #[test]
    fn test_default_layout() {
        let layout = default_layout();
        assert_eq!(layout.name(), "buck2");
    }

    #[test]
    fn test_available_layouts() {
        let layouts = available_layouts();
        assert!(layouts.contains(&"buck2"));
        assert!(layouts.contains(&"bazel"));
    }

    #[test]
    fn test_layout_by_name() {
        let layout = layout_by_name("buck2");
        assert!(layout.is_some());

        let layout = layout_by_name("nonexistent");
        assert!(layout.is_none());
    }

    #[test]
    fn test_global_registry() {
        let registry = global_registry();
        assert!(registry.get("buck2").is_some());
    }

    #[test]
    fn test_buck2_layout_config_generation() {
        use composition::layout::{CellInfo, Layout};

        let layout = Buck2Layout::new();
        let ctx = LayoutContext {
            mount_point: PathBuf::from("/firefly/turnkey"),
            repo_root: PathBuf::from("/home/user/repo"),
            source_dir_name: "root".to_string(),
            cell_prefix: "external".to_string(),
            cells: vec![CellInfo {
                name: "godeps".to_string(),
                source_path: PathBuf::from("/nix/store/xxx-godeps"),
                editable: false,
            }],
        };

        let configs = layout.generate_config(&ctx);
        assert!(!configs.is_empty());

        // Should have .buckconfig
        let has_buckconfig = configs.iter().any(|c| c.name.contains("buckconfig"));
        assert!(has_buckconfig, "Should generate .buckconfig");
    }
}
