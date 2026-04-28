//! Configuration types for composition backends

use std::path::PathBuf;

/// Configuration for a composition backend
#[derive(Debug, Clone)]
pub struct CompositionConfig {
    /// The mount point for the composed view
    ///
    /// For FUSE backend: This is where the FUSE filesystem is mounted
    /// (e.g., `/firefly/turnkey`).
    ///
    /// For symlink backend: This is the `.turnkey/` directory in the repo.
    pub mount_point: PathBuf,

    /// The source repository root
    ///
    /// This is the actual repository directory that contains the source
    /// code. The composed view will expose this under `source_dir_name`.
    pub repo_root: PathBuf,

    /// The name of the directory that exposes the repository root
    ///
    /// Default: `"root"`. The repository root will be accessible at
    /// `<mount_point>/<source_dir_name>/` in the composed view.
    pub source_dir_name: String,

    /// The prefix path for dependency cells
    ///
    /// Default: `"external"`. Cells will be mounted at
    /// `<mount_point>/<cell_prefix>/<cell_name>/`.
    ///
    /// For Buck2, this means `godeps = external/godeps` in .buckconfig.
    pub cell_prefix: String,

    /// Cell configurations
    ///
    /// Each cell represents a dependency namespace (godeps, rustdeps, etc.)
    /// that will be composed into the view.
    pub cells: Vec<CellConfig>,

    /// The layout type
    ///
    /// Determines the directory structure of the composed view:
    /// - `"buck2"`: Buck2-compatible layout (current default)
    /// - `"bazel"`: Bazel-compatible layout (future)
    /// - Custom layout names for user-defined layouts
    pub layout: String,

    /// How to handle reads during updates
    pub consistency_mode: ConsistencyMode,

    /// Enable the edit layer for external dependencies
    ///
    /// When enabled, writes to external dependency files are captured
    /// in an overlay and can be converted to patches.
    pub enable_editing: bool,

    /// Directory for storing edits (relative to repo_root)
    ///
    /// Default: `.turnkey/edits`
    pub edits_dir: PathBuf,

    /// Directory for storing generated patches (relative to repo_root)
    ///
    /// Default: `.turnkey/patches`
    pub patches_dir: PathBuf,

    /// Files and directories to exclude from the source pass-through.
    ///
    /// Entries are matched against the first path component under `root/`.
    /// Trailing `/` is stripped. Examples: `.envrc`, `.devenv`, `buck-out`.
    pub exclude: Vec<String>,
}

impl CompositionConfig {
    /// Create a new configuration with sensible defaults
    pub fn new(mount_point: impl Into<PathBuf>, repo_root: impl Into<PathBuf>) -> Self {
        Self {
            mount_point: mount_point.into(),
            repo_root: repo_root.into(),
            source_dir_name: "root".to_string(),
            cell_prefix: "external".to_string(),
            cells: Vec::new(),
            layout: "buck2".to_string(),
            consistency_mode: ConsistencyMode::BlockUntilReady,
            enable_editing: false,
            edits_dir: PathBuf::from(".turnkey/edits"),
            patches_dir: PathBuf::from(".turnkey/patches"),
            exclude: Vec::new(),
        }
    }

    /// Set the source directory name
    pub fn with_source_dir_name(mut self, name: impl Into<String>) -> Self {
        self.source_dir_name = name.into();
        self
    }

    /// Set the cell prefix path
    pub fn with_cell_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.cell_prefix = prefix.into();
        self
    }

    /// Add a cell configuration
    pub fn with_cell(mut self, cell: CellConfig) -> Self {
        self.cells.push(cell);
        self
    }

    /// Add exclusion patterns for the source pass-through
    pub fn with_excludes(mut self, excludes: Vec<String>) -> Self {
        self.exclude = excludes;
        self
    }

    /// Set the layout type
    pub fn with_layout(mut self, layout: impl Into<String>) -> Self {
        self.layout = layout.into();
        self
    }

    /// Set the consistency mode
    pub fn with_consistency_mode(mut self, mode: ConsistencyMode) -> Self {
        self.consistency_mode = mode;
        self
    }

    /// Enable the edit layer
    pub fn with_editing(mut self, enable: bool) -> Self {
        self.enable_editing = enable;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.mount_point.as_os_str().is_empty() {
            return Err("mount_point cannot be empty".into());
        }

        if self.repo_root.as_os_str().is_empty() {
            return Err("repo_root cannot be empty".into());
        }

        if self.layout.is_empty() {
            return Err("layout cannot be empty".into());
        }

        // Check for duplicate cell names
        let mut seen = std::collections::HashSet::new();
        for cell in &self.cells {
            if !seen.insert(&cell.name) {
                return Err(format!("duplicate cell name: {}", cell.name));
            }
        }

        Ok(())
    }
}

/// Configuration for a single cell (dependency namespace)
#[derive(Debug, Clone)]
pub struct CellConfig {
    /// The cell name (e.g., "godeps", "rustdeps", "pydeps")
    pub name: String,

    /// The source path for this cell
    ///
    /// This is typically a Nix store path (e.g., `/nix/store/xxx-godeps`)
    /// or a path that will be resolved to a Nix store path.
    pub source_path: PathBuf,

    /// The manifest file that triggers rebuilds for this cell
    ///
    /// E.g., `go-deps.toml` for godeps, `rust-deps.toml` for rustdeps
    pub manifest_file: Option<PathBuf>,

    /// Whether this cell supports editing
    ///
    /// Some cells (like vendored dependencies) can be edited,
    /// while others (like prebuilt binaries) cannot.
    pub editable: bool,
}

impl CellConfig {
    /// Create a new cell configuration
    pub fn new(name: impl Into<String>, source_path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            source_path: source_path.into(),
            manifest_file: None,
            editable: false,
        }
    }

    /// Set the manifest file for this cell
    pub fn with_manifest(mut self, manifest: impl Into<PathBuf>) -> Self {
        self.manifest_file = Some(manifest.into());
        self
    }

    /// Set whether this cell is editable
    pub fn with_editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }
}

/// How to handle file reads when dependencies are being updated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsistencyMode {
    /// Block reads until the new version is ready (default)
    ///
    /// This provides the strongest consistency guarantee: reads will
    /// never return stale data, but may block for the duration of
    /// the Nix build.
    #[default]
    BlockUntilReady,

    /// Allow reads to return stale data during updates
    ///
    /// Reads will always succeed immediately, but may return data
    /// from the previous version while an update is in progress.
    /// A warning is logged when stale data is returned.
    AllowStale,

    /// Fail reads with EAGAIN if the path is being updated
    ///
    /// This mode is useful for non-blocking access patterns where
    /// the caller can retry later. The error includes information
    /// about when the update might complete.
    FailIfUpdating,
}

impl std::fmt::Display for ConsistencyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsistencyMode::BlockUntilReady => write!(f, "block"),
            ConsistencyMode::AllowStale => write!(f, "stale"),
            ConsistencyMode::FailIfUpdating => write!(f, "fail"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/xxx-godeps"))
            .with_cell(
                CellConfig::new("rustdeps", "/nix/store/xxx-rustdeps")
                    .with_manifest("rust-deps.toml")
                    .with_editable(true),
            )
            .with_layout("buck2")
            .with_consistency_mode(ConsistencyMode::AllowStale)
            .with_editing(true);

        assert_eq!(config.mount_point, PathBuf::from("/firefly/turnkey"));
        assert_eq!(config.cells.len(), 2);
        assert_eq!(config.layout, "buck2");
        assert_eq!(config.consistency_mode, ConsistencyMode::AllowStale);
        assert!(config.enable_editing);
    }

    #[test]
    fn test_config_validation() {
        let config = CompositionConfig::new("/mount", "/repo");
        assert!(config.validate().is_ok());

        let mut bad_config = CompositionConfig::new("/mount", "/repo");
        bad_config.mount_point = PathBuf::new();
        assert!(bad_config.validate().is_err());
    }

    #[test]
    fn test_duplicate_cell_detection() {
        let config = CompositionConfig::new("/mount", "/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/a"))
            .with_cell(CellConfig::new("godeps", "/nix/store/b")); // Duplicate!

        assert!(config.validate().is_err());
        assert!(config.validate().unwrap_err().contains("duplicate"));
    }

    #[test]
    fn test_consistency_mode_display() {
        assert_eq!(ConsistencyMode::BlockUntilReady.to_string(), "block");
        assert_eq!(ConsistencyMode::AllowStale.to_string(), "stale");
        assert_eq!(ConsistencyMode::FailIfUpdating.to_string(), "fail");
    }
}
