//! Layout trait for pluggable build system support
//!
//! This module defines the `Layout` trait that allows different build systems
//! to have their own directory structure and configuration file generation.
//!
//! # Supported Layouts
//!
//! - **Buck2Layout**: The default layout for Buck2 projects (`.buckconfig`, `.buckroot`)
//! - Future: **BazelLayout**: Bazel-compatible layout (`WORKSPACE`, `external/`)
//!
//! # Architecture
//!
//! The layout trait abstracts:
//! 1. How dependency cells are mapped to filesystem paths
//! 2. What configuration files are generated and their content
//! 3. Which cells are supported by the build system
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        Layout Trait                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  map_dep()          - Cell path mapping                          │
//! │  generate_config()  - Build system config files                  │
//! │  supported_cells()  - Which cells this layout handles            │
//! └─────────────────────────────────────────────────────────────────┘
//!               │                           │
//!     ┌─────────┴─────────┐       ┌─────────┴─────────┐
//!     │   Buck2Layout     │       │   BazelLayout     │
//!     │   .buckconfig     │       │   WORKSPACE       │
//!     │   .buckroot       │       │   BUILD.bazel     │
//!     └───────────────────┘       └───────────────────┘
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Information about a dependency cell
#[derive(Debug, Clone)]
pub struct CellInfo {
    /// The cell name (e.g., "godeps", "rustdeps")
    pub name: String,
    /// The source path in the Nix store
    pub source_path: PathBuf,
    /// Whether this cell is editable
    pub editable: bool,
}

impl CellInfo {
    /// Create a new cell info
    pub fn new(name: impl Into<String>, source_path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            source_path: source_path.into(),
            editable: false,
        }
    }

    /// Mark this cell as editable
    pub fn with_editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }
}

/// A generated configuration file
#[derive(Debug, Clone)]
pub struct ConfigFile {
    /// The filename (e.g., ".buckconfig", ".buckroot", "WORKSPACE")
    pub name: String,
    /// The file content
    pub content: String,
}

impl ConfigFile {
    /// Create a new config file
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
        }
    }
}

/// Context for layout operations
///
/// This provides all the information a layout needs to generate
/// configuration files and map paths.
#[derive(Debug, Clone)]
pub struct LayoutContext {
    /// The mount point for the composed view (e.g., `/firefly/turnkey`)
    pub mount_point: PathBuf,
    /// The repository root path
    pub repo_root: PathBuf,
    /// Name of the directory exposing the repo root (e.g., "root")
    pub source_dir_name: String,
    /// Prefix for cell directories (e.g., "external")
    pub cell_prefix: String,
    /// All available cells
    pub cells: Vec<CellInfo>,
}

impl LayoutContext {
    /// Get a cell by name
    pub fn get_cell(&self, name: &str) -> Option<&CellInfo> {
        self.cells.iter().find(|c| c.name == name)
    }

    /// Get the path to a cell in the composed view
    pub fn cell_path(&self, cell_name: &str) -> PathBuf {
        self.mount_point
            .join(&self.cell_prefix)
            .join(cell_name)
    }

    /// Get the path to the source directory in the composed view
    pub fn source_path(&self) -> PathBuf {
        self.mount_point.join(&self.source_dir_name)
    }
}

/// Trait for build system layout plugins
///
/// A layout defines how dependencies are organized and what configuration
/// files are generated for a specific build system.
///
/// # Example
///
/// ```ignore
/// use composition::layout::{Layout, LayoutContext, ConfigFile};
///
/// struct MyLayout;
///
/// impl Layout for MyLayout {
///     fn name(&self) -> &'static str { "my-build-system" }
///
///     fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
///         Some(ctx.cell_path(cell).join(path))
///     }
///
///     fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
///         vec![ConfigFile::new("my.config", "# config content")]
///     }
///
///     fn supported_cells(&self, ctx: &LayoutContext) -> Vec<String> {
///         ctx.cells.iter().map(|c| c.name.clone()).collect()
///     }
/// }
/// ```
pub trait Layout: Send + Sync {
    /// Get the layout name (e.g., "buck2", "bazel")
    fn name(&self) -> &'static str;

    /// Map a dependency path to its location in the composed view
    ///
    /// Given a cell name and a relative path within that cell, returns
    /// the full path where the file will be accessible in the composed view.
    ///
    /// # Arguments
    ///
    /// * `ctx` - The layout context with mount point and cell info
    /// * `cell` - The cell name (e.g., "godeps")
    /// * `path` - The relative path within the cell (e.g., "vendor/github.com/foo/bar")
    ///
    /// # Returns
    ///
    /// The full path in the composed view, or `None` if the cell is not supported.
    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf>;

    /// Generate configuration files for this build system
    ///
    /// Returns a list of configuration files that should be placed in the
    /// source directory of the composed view.
    ///
    /// For Buck2, this includes `.buckconfig` and `.buckroot`.
    /// For Bazel, this would include `WORKSPACE` and potentially `BUILD.bazel`.
    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile>;

    /// Get the list of cells this layout supports
    ///
    /// Some layouts may only support specific types of cells. For example,
    /// a Go-specific layout might only support "godeps".
    ///
    /// By default, returns all cells from the context.
    fn supported_cells(&self, ctx: &LayoutContext) -> Vec<String> {
        ctx.cells.iter().map(|c| c.name.clone()).collect()
    }

    /// Check if a cell is supported by this layout
    fn supports_cell(&self, ctx: &LayoutContext, cell: &str) -> bool {
        self.supported_cells(ctx).iter().any(|c| c == cell)
    }

    /// Get additional files to generate in cell directories
    ///
    /// Some layouts may need to generate files within cell directories,
    /// not just in the source directory. Override this method to provide
    /// per-cell configuration files.
    ///
    /// Returns a map of cell name -> list of config files for that cell.
    fn generate_cell_config(&self, _ctx: &LayoutContext) -> HashMap<String, Vec<ConfigFile>> {
        HashMap::new()
    }
}

/// Buck2 layout implementation
///
/// This is the default layout for Buck2 projects. It generates:
/// - `.buckconfig` - Cell definitions and build file configuration
/// - `.buckroot` - Repository root marker
///
/// Cell paths follow the pattern: `<mount>/<cell_prefix>/<cell_name>/`
#[derive(Debug, Clone, Default)]
pub struct Buck2Layout;

impl Buck2Layout {
    /// Create a new Buck2 layout
    pub fn new() -> Self {
        Self
    }
}

impl Layout for Buck2Layout {
    fn name(&self) -> &'static str {
        "buck2"
    }

    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
        // Check if cell is supported
        if ctx.get_cell(cell).is_none() {
            return None;
        }

        Some(ctx.cell_path(cell).join(path))
    }

    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
        let mut configs = Vec::new();

        // Generate .buckconfig
        configs.push(self.generate_buckconfig(ctx));

        // Generate .buckroot
        configs.push(ConfigFile::new(
            ".buckroot",
            "# Buck2 repository root marker\n",
        ));

        configs
    }
}

impl Buck2Layout {
    /// Generate the .buckconfig content
    fn generate_buckconfig(&self, ctx: &LayoutContext) -> ConfigFile {
        let mut content = String::new();

        // Cell definitions
        // .buckconfig lives inside the source directory (overlay on repo root).
        // Paths are relative to where .buckconfig lives:
        // - `root = .` (current directory, the repo root)
        // - `prelude = prelude` (relative to repo root)
        // - `<cell> = ../<cell_prefix>/<cell>` (sibling directory)
        content.push_str("[cells]\n");
        content.push_str("    root = .\n");
        content.push_str("    prelude = prelude\n");

        // Add cells for each dependency
        for cell in &ctx.cells {
            content.push_str(&format!(
                "    {} = ../{}/{}\n",
                cell.name, ctx.cell_prefix, cell.name
            ));
        }

        content.push('\n');

        // Buildfile configuration
        content.push_str("[buildfile]\n");
        content.push_str("    name = rules.star\n");

        ConfigFile::new(".buckconfig", content)
    }
}

/// Type alias for boxed layout trait objects
pub type BoxedLayout = Box<dyn Layout>;

/// Create a default Buck2 layout
pub fn default_layout() -> BoxedLayout {
    Box::new(Buck2Layout::new())
}

/// Create a layout by name
pub fn layout_by_name(name: &str) -> Option<BoxedLayout> {
    match name {
        "buck2" => Some(Box::new(Buck2Layout::new())),
        // Future: "bazel" => Some(Box::new(BazelLayout::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> LayoutContext {
        LayoutContext {
            mount_point: PathBuf::from("/firefly/turnkey"),
            repo_root: PathBuf::from("/home/user/repo"),
            source_dir_name: "root".to_string(),
            cell_prefix: "external".to_string(),
            cells: vec![
                CellInfo::new("godeps", "/nix/store/abc-godeps"),
                CellInfo::new("rustdeps", "/nix/store/xyz-rustdeps").with_editable(true),
            ],
        }
    }

    #[test]
    fn test_cell_info() {
        let cell = CellInfo::new("godeps", "/nix/store/abc")
            .with_editable(true);
        assert_eq!(cell.name, "godeps");
        assert_eq!(cell.source_path, PathBuf::from("/nix/store/abc"));
        assert!(cell.editable);
    }

    #[test]
    fn test_config_file() {
        let config = ConfigFile::new(".buckconfig", "[cells]\n");
        assert_eq!(config.name, ".buckconfig");
        assert_eq!(config.content, "[cells]\n");
    }

    #[test]
    fn test_layout_context_cell_path() {
        let ctx = test_context();
        assert_eq!(
            ctx.cell_path("godeps"),
            PathBuf::from("/firefly/turnkey/external/godeps")
        );
    }

    #[test]
    fn test_layout_context_source_path() {
        let ctx = test_context();
        assert_eq!(
            ctx.source_path(),
            PathBuf::from("/firefly/turnkey/root")
        );
    }

    #[test]
    fn test_layout_context_get_cell() {
        let ctx = test_context();
        assert!(ctx.get_cell("godeps").is_some());
        assert!(ctx.get_cell("unknown").is_none());
    }

    #[test]
    fn test_buck2_layout_name() {
        let layout = Buck2Layout::new();
        assert_eq!(layout.name(), "buck2");
    }

    #[test]
    fn test_buck2_layout_map_dep() {
        let layout = Buck2Layout::new();
        let ctx = test_context();

        // Known cell
        let path = layout.map_dep(&ctx, "godeps", Path::new("vendor/github.com/foo"));
        assert_eq!(
            path,
            Some(PathBuf::from("/firefly/turnkey/external/godeps/vendor/github.com/foo"))
        );

        // Unknown cell
        let path = layout.map_dep(&ctx, "unknown", Path::new("something"));
        assert!(path.is_none());
    }

    #[test]
    fn test_buck2_layout_generate_config() {
        let layout = Buck2Layout::new();
        let ctx = test_context();

        let configs = layout.generate_config(&ctx);
        assert_eq!(configs.len(), 2);

        // Check .buckconfig
        let buckconfig = configs.iter().find(|c| c.name == ".buckconfig").unwrap();
        assert!(buckconfig.content.contains("[cells]"));
        assert!(buckconfig.content.contains("root = ."));
        assert!(buckconfig.content.contains("prelude = prelude"));
        assert!(buckconfig.content.contains("godeps = ../external/godeps"));
        assert!(buckconfig.content.contains("rustdeps = ../external/rustdeps"));
        assert!(buckconfig.content.contains("[buildfile]"));
        assert!(buckconfig.content.contains("name = rules.star"));

        // Check .buckroot
        let buckroot = configs.iter().find(|c| c.name == ".buckroot").unwrap();
        assert!(buckroot.content.contains("root marker"));
    }

    #[test]
    fn test_buck2_layout_supported_cells() {
        let layout = Buck2Layout::new();
        let ctx = test_context();

        let cells = layout.supported_cells(&ctx);
        assert_eq!(cells.len(), 2);
        assert!(cells.contains(&"godeps".to_string()));
        assert!(cells.contains(&"rustdeps".to_string()));
    }

    #[test]
    fn test_buck2_layout_supports_cell() {
        let layout = Buck2Layout::new();
        let ctx = test_context();

        assert!(layout.supports_cell(&ctx, "godeps"));
        assert!(layout.supports_cell(&ctx, "rustdeps"));
        assert!(!layout.supports_cell(&ctx, "unknown"));
    }

    #[test]
    fn test_default_layout() {
        let layout = default_layout();
        assert_eq!(layout.name(), "buck2");
    }

    #[test]
    fn test_layout_by_name() {
        assert!(layout_by_name("buck2").is_some());
        assert!(layout_by_name("unknown").is_none());
    }
}
