//! Layout trait for pluggable build system support
//!
//! This module defines the `Layout` trait that allows different build systems
//! to have their own directory structure and configuration file generation.
//!
//! # Supported Layouts
//!
//! - **Buck2Layout**: The default layout for Buck2 projects (`.buckconfig`, `.buckroot`)
//! - **BazelLayout**: Bazel-compatible layout (`WORKSPACE`, `BUILD.bazel`)
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
//!
//! # Custom Layout API
//!
//! You can create custom layouts by implementing the `Layout` trait:
//!
//! ```ignore
//! use composition::layout::{Layout, LayoutContext, ConfigFile, LayoutRegistry};
//! use std::path::{Path, PathBuf};
//!
//! // Define your custom layout
//! struct MyBuildSystemLayout {
//!     workspace_name: String,
//! }
//!
//! impl MyBuildSystemLayout {
//!     fn new(workspace_name: &str) -> Self {
//!         Self {
//!             workspace_name: workspace_name.to_string(),
//!         }
//!     }
//! }
//!
//! impl Layout for MyBuildSystemLayout {
//!     fn name(&self) -> &'static str {
//!         "my-build-system"
//!     }
//!
//!     fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
//!         // Map dependencies to your build system's expected paths
//!         Some(ctx.cell_path(cell).join(path))
//!     }
//!
//!     fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
//!         // Generate your build system's configuration files
//!         vec![
//!             ConfigFile::new("my.config", format!(
//!                 "workspace = {}\ndeps = {}\n",
//!                 self.workspace_name,
//!                 ctx.cells.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
//!             )),
//!         ]
//!     }
//! }
//!
//! // Register and use the custom layout
//! let mut registry = LayoutRegistry::new();
//! registry.register("my-build-system", || Box::new(MyBuildSystemLayout::new("my-project")));
//!
//! let layout = registry.get("my-build-system").unwrap();
//! ```
//!
//! # Layout Registry
//!
//! The `LayoutRegistry` allows runtime registration of custom layouts:
//!
//! ```ignore
//! use composition::layout::{LayoutRegistry, global_registry};
//!
//! // Get the global registry (pre-populated with buck2 and bazel)
//! let registry = global_registry();
//!
//! // List available layouts
//! for name in registry.list() {
//!     println!("Available: {}", name);
//! }
//!
//! // Get a layout by name
//! if let Some(layout) = registry.get("buck2") {
//!     let configs = layout.generate_config(&ctx);
//! }
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

/// Bazel layout implementation
///
/// This layout is for Bazel projects. It generates:
/// - `WORKSPACE` - External repository definitions
/// - `BUILD.bazel` - Root build file marker (empty)
///
/// Dependencies are mapped to `@repo_name//path` style references.
/// Cell paths follow the pattern: `<mount>/<cell_prefix>/<cell_name>/`
#[derive(Debug, Clone, Default)]
pub struct BazelLayout;

impl BazelLayout {
    /// Create a new Bazel layout
    pub fn new() -> Self {
        Self
    }
}

impl Layout for BazelLayout {
    fn name(&self) -> &'static str {
        "bazel"
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

        // Generate WORKSPACE
        configs.push(self.generate_workspace(ctx));

        // Generate empty BUILD.bazel as root marker
        configs.push(ConfigFile::new(
            "BUILD.bazel",
            "# Bazel root build file\n# Auto-generated by turnkey\n",
        ));

        configs
    }
}

impl BazelLayout {
    /// Generate the WORKSPACE content
    fn generate_workspace(&self, ctx: &LayoutContext) -> ConfigFile {
        let mut content = String::new();

        // Header
        content.push_str("# Bazel WORKSPACE file\n");
        content.push_str("# Auto-generated by turnkey\n\n");

        content.push_str("workspace(name = \"root\")\n\n");

        // Add local_repository for each dependency cell
        // WORKSPACE lives in the source directory (overlay on repo root).
        // Paths are relative to where WORKSPACE lives:
        // - `../<cell_prefix>/<cell>` (sibling directory)
        for cell in &ctx.cells {
            content.push_str(&format!(
                "local_repository(\n    name = \"{}\",\n    path = \"../{}/{}\",\n)\n\n",
                cell.name, ctx.cell_prefix, cell.name
            ));
        }

        ConfigFile::new("WORKSPACE", content)
    }

    /// Convert a cell reference to Bazel's @repo// syntax
    ///
    /// For example: ("godeps", "vendor/github.com/foo/bar") -> "@godeps//vendor/github.com/foo/bar"
    pub fn to_bazel_label(cell: &str, path: &str) -> String {
        format!("@{}//{}",  cell, path)
    }
}

/// Type alias for boxed layout trait objects
pub type BoxedLayout = Box<dyn Layout>;

/// Factory function type for creating layouts
pub type LayoutFactory = Box<dyn Fn() -> BoxedLayout + Send + Sync>;

/// Registry for layout plugins
///
/// The registry allows runtime registration of custom layouts. It comes
/// pre-populated with built-in layouts (buck2, bazel) and allows users
/// to register additional layouts.
///
/// # Example
///
/// ```ignore
/// use composition::layout::{LayoutRegistry, Layout, ConfigFile, LayoutContext};
/// use std::path::{Path, PathBuf};
///
/// // Create a custom layout
/// struct CustomLayout;
///
/// impl Layout for CustomLayout {
///     fn name(&self) -> &'static str { "custom" }
///     fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
///         Some(ctx.cell_path(cell).join(path))
///     }
///     fn generate_config(&self, _ctx: &LayoutContext) -> Vec<ConfigFile> {
///         vec![ConfigFile::new("custom.config", "# custom config\n")]
///     }
/// }
///
/// // Register it
/// let mut registry = LayoutRegistry::new();
/// registry.register("custom", || Box::new(CustomLayout));
///
/// // Use it
/// let layout = registry.get("custom").unwrap();
/// ```
pub struct LayoutRegistry {
    /// Registered layout factories
    factories: HashMap<String, LayoutFactory>,
}

impl LayoutRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Create a registry pre-populated with built-in layouts
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register_builtin("buck2", || Box::new(Buck2Layout::new()));
        registry.register_builtin("bazel", || Box::new(BazelLayout::new()));
        registry
    }

    /// Register a layout factory
    ///
    /// The factory function is called each time `get()` is called to create
    /// a fresh layout instance. This allows layouts to have per-instance state.
    ///
    /// # Arguments
    ///
    /// * `name` - The name to register the layout under
    /// * `factory` - A function that creates a new layout instance
    ///
    /// # Returns
    ///
    /// `true` if the layout was registered, `false` if a layout with that
    /// name already exists (registration failed).
    pub fn register<F>(&mut self, name: &str, factory: F) -> bool
    where
        F: Fn() -> BoxedLayout + Send + Sync + 'static,
    {
        if self.factories.contains_key(name) {
            return false;
        }
        self.factories.insert(name.to_string(), Box::new(factory));
        true
    }

    /// Register a built-in layout (internal use)
    fn register_builtin<F>(&mut self, name: &str, factory: F)
    where
        F: Fn() -> BoxedLayout + Send + Sync + 'static,
    {
        self.factories.insert(name.to_string(), Box::new(factory));
    }

    /// Get a layout by name
    ///
    /// Returns a new instance of the layout, or `None` if no layout
    /// is registered with that name.
    pub fn get(&self, name: &str) -> Option<BoxedLayout> {
        self.factories.get(name).map(|f| f())
    }

    /// Check if a layout is registered
    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }

    /// List all registered layout names
    pub fn list(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Get the number of registered layouts
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }

    /// Remove a layout from the registry
    ///
    /// Returns `true` if the layout was removed, `false` if it wasn't registered.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.factories.remove(name).is_some()
    }
}

impl Default for LayoutRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Get a global layout registry with built-in layouts
///
/// This returns a new registry instance each time (not a true singleton)
/// because layouts may have per-instance state. For a shared registry,
/// create one with `LayoutRegistry::with_builtins()` and share it.
pub fn global_registry() -> LayoutRegistry {
    LayoutRegistry::with_builtins()
}

/// Create a default Buck2 layout
pub fn default_layout() -> BoxedLayout {
    Box::new(Buck2Layout::new())
}

/// Create a layout by name (convenience function)
///
/// This uses the global registry with built-in layouts. For custom layouts,
/// use `LayoutRegistry` directly.
pub fn layout_by_name(name: &str) -> Option<BoxedLayout> {
    global_registry().get(name)
}

/// List all available built-in layout names
pub fn available_layouts() -> Vec<&'static str> {
    vec!["buck2", "bazel"]
}

/// A simple configurable layout for basic use cases
///
/// This layout allows specifying custom configuration without implementing
/// the full `Layout` trait. It's useful for simple build systems or testing.
///
/// # Example
///
/// ```ignore
/// use composition::layout::{SimpleLayout, ConfigFile};
///
/// let layout = SimpleLayout::new("my-build")
///     .with_config_file(".myconfig", |ctx| {
///         format!("cells = {}\n", ctx.cells.len())
///     })
///     .with_root_marker(".myroot");
///
/// let configs = layout.generate_config(&ctx);
/// ```
pub struct SimpleLayout {
    name: String,
    config_generators: Vec<(String, Box<dyn Fn(&LayoutContext) -> String + Send + Sync>)>,
    root_marker: Option<(String, String)>,
}

impl SimpleLayout {
    /// Create a new simple layout with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            config_generators: Vec::new(),
            root_marker: None,
        }
    }

    /// Add a configuration file with a generator function
    pub fn with_config_file<F>(mut self, name: impl Into<String>, generator: F) -> Self
    where
        F: Fn(&LayoutContext) -> String + Send + Sync + 'static,
    {
        self.config_generators
            .push((name.into(), Box::new(generator)));
        self
    }

    /// Add a static configuration file
    pub fn with_static_config(self, name: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        self.with_config_file(name, move |_| content.clone())
    }

    /// Add a root marker file (like .buckroot or BUILD.bazel)
    pub fn with_root_marker(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.root_marker = Some((
            name.clone(),
            format!("# {} root marker\n# Auto-generated by turnkey\n", self.name),
        ));
        self
    }

    /// Add a root marker file with custom content
    pub fn with_root_marker_content(
        mut self,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.root_marker = Some((name.into(), content.into()));
        self
    }
}

impl Layout for SimpleLayout {
    fn name(&self) -> &'static str {
        // This is a limitation - we can't return &'static str for dynamic names
        // For now, return a generic name. Users needing custom names should
        // implement Layout directly.
        "simple"
    }

    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
        if ctx.get_cell(cell).is_none() {
            return None;
        }
        Some(ctx.cell_path(cell).join(path))
    }

    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
        let mut configs: Vec<ConfigFile> = self
            .config_generators
            .iter()
            .map(|(name, generator)| ConfigFile::new(name.clone(), generator(ctx)))
            .collect();

        if let Some((name, content)) = &self.root_marker {
            configs.push(ConfigFile::new(name.clone(), content.clone()));
        }

        configs
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
        assert!(layout_by_name("bazel").is_some());
        assert!(layout_by_name("unknown").is_none());
    }

    // Bazel layout tests

    #[test]
    fn test_bazel_layout_name() {
        let layout = BazelLayout::new();
        assert_eq!(layout.name(), "bazel");
    }

    #[test]
    fn test_bazel_layout_map_dep() {
        let layout = BazelLayout::new();
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
    fn test_bazel_layout_generate_config() {
        let layout = BazelLayout::new();
        let ctx = test_context();

        let configs = layout.generate_config(&ctx);
        assert_eq!(configs.len(), 2);

        // Check WORKSPACE
        let workspace = configs.iter().find(|c| c.name == "WORKSPACE").unwrap();
        assert!(workspace.content.contains("workspace(name = \"root\")"));
        assert!(workspace.content.contains("local_repository("));
        assert!(workspace.content.contains("name = \"godeps\""));
        assert!(workspace.content.contains("path = \"../external/godeps\""));
        assert!(workspace.content.contains("name = \"rustdeps\""));
        assert!(workspace.content.contains("path = \"../external/rustdeps\""));

        // Check BUILD.bazel
        let build_bazel = configs.iter().find(|c| c.name == "BUILD.bazel").unwrap();
        assert!(build_bazel.content.contains("Bazel root build file"));
    }

    #[test]
    fn test_bazel_layout_supported_cells() {
        let layout = BazelLayout::new();
        let ctx = test_context();

        let cells = layout.supported_cells(&ctx);
        assert_eq!(cells.len(), 2);
        assert!(cells.contains(&"godeps".to_string()));
        assert!(cells.contains(&"rustdeps".to_string()));
    }

    #[test]
    fn test_bazel_layout_supports_cell() {
        let layout = BazelLayout::new();
        let ctx = test_context();

        assert!(layout.supports_cell(&ctx, "godeps"));
        assert!(layout.supports_cell(&ctx, "rustdeps"));
        assert!(!layout.supports_cell(&ctx, "unknown"));
    }

    #[test]
    fn test_bazel_to_bazel_label() {
        assert_eq!(
            BazelLayout::to_bazel_label("godeps", "vendor/github.com/foo/bar"),
            "@godeps//vendor/github.com/foo/bar"
        );
        assert_eq!(
            BazelLayout::to_bazel_label("rustdeps", "vendor/serde@1.0.0"),
            "@rustdeps//vendor/serde@1.0.0"
        );
    }

    // Layout Registry tests

    #[test]
    fn test_registry_new_empty() {
        let registry = LayoutRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_with_builtins() {
        let registry = LayoutRegistry::with_builtins();
        assert!(!registry.is_empty());
        assert!(registry.contains("buck2"));
        assert!(registry.contains("bazel"));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_registry_register_custom() {
        let mut registry = LayoutRegistry::new();

        // Register a custom layout
        let registered = registry.register("custom", || Box::new(Buck2Layout::new()));
        assert!(registered);
        assert!(registry.contains("custom"));

        // Can't register same name twice
        let registered_again = registry.register("custom", || Box::new(BazelLayout::new()));
        assert!(!registered_again);
    }

    #[test]
    fn test_registry_get() {
        let registry = LayoutRegistry::with_builtins();

        let buck2 = registry.get("buck2");
        assert!(buck2.is_some());
        assert_eq!(buck2.unwrap().name(), "buck2");

        let bazel = registry.get("bazel");
        assert!(bazel.is_some());
        assert_eq!(bazel.unwrap().name(), "bazel");

        let unknown = registry.get("unknown");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_registry_list() {
        let registry = LayoutRegistry::with_builtins();
        let names = registry.list();
        assert!(names.contains(&"buck2"));
        assert!(names.contains(&"bazel"));
    }

    #[test]
    fn test_registry_unregister() {
        let mut registry = LayoutRegistry::with_builtins();
        assert!(registry.contains("buck2"));

        let removed = registry.unregister("buck2");
        assert!(removed);
        assert!(!registry.contains("buck2"));

        let removed_again = registry.unregister("buck2");
        assert!(!removed_again);
    }

    #[test]
    fn test_global_registry() {
        let registry = global_registry();
        assert!(registry.contains("buck2"));
        assert!(registry.contains("bazel"));
    }

    #[test]
    fn test_available_layouts() {
        let layouts = available_layouts();
        assert!(layouts.contains(&"buck2"));
        assert!(layouts.contains(&"bazel"));
    }

    // SimpleLayout tests

    #[test]
    fn test_simple_layout_basic() {
        let layout = SimpleLayout::new("my-build")
            .with_static_config("my.config", "# my config\n")
            .with_root_marker(".myroot");

        assert_eq!(layout.name(), "simple");
    }

    #[test]
    fn test_simple_layout_generate_config() {
        let layout = SimpleLayout::new("test")
            .with_config_file("cells.txt", |ctx| {
                ctx.cells
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .with_root_marker(".testroot");

        let ctx = test_context();
        let configs = layout.generate_config(&ctx);

        assert_eq!(configs.len(), 2);

        let cells_txt = configs.iter().find(|c| c.name == "cells.txt").unwrap();
        assert!(cells_txt.content.contains("godeps"));
        assert!(cells_txt.content.contains("rustdeps"));

        let root = configs.iter().find(|c| c.name == ".testroot").unwrap();
        assert!(root.content.contains("root marker"));
    }

    #[test]
    fn test_simple_layout_map_dep() {
        let layout = SimpleLayout::new("test");
        let ctx = test_context();

        let path = layout.map_dep(&ctx, "godeps", Path::new("vendor/foo"));
        assert_eq!(
            path,
            Some(PathBuf::from("/firefly/turnkey/external/godeps/vendor/foo"))
        );

        let unknown = layout.map_dep(&ctx, "unknown", Path::new("something"));
        assert!(unknown.is_none());
    }

    #[test]
    fn test_simple_layout_with_static_config() {
        let layout = SimpleLayout::new("test")
            .with_static_config("VERSION", "1.0.0\n");

        let ctx = test_context();
        let configs = layout.generate_config(&ctx);

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "VERSION");
        assert_eq!(configs[0].content, "1.0.0\n");
    }

    #[test]
    fn test_simple_layout_with_root_marker_content() {
        let layout = SimpleLayout::new("test")
            .with_root_marker_content(".marker", "custom content");

        let ctx = test_context();
        let configs = layout.generate_config(&ctx);

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, ".marker");
        assert_eq!(configs[0].content, "custom content");
    }

    // Custom layout implementation test

    struct TestCustomLayout {
        prefix: String,
    }

    impl TestCustomLayout {
        fn new(prefix: &str) -> Self {
            Self {
                prefix: prefix.to_string(),
            }
        }
    }

    impl Layout for TestCustomLayout {
        fn name(&self) -> &'static str {
            "test-custom"
        }

        fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
            Some(ctx.cell_path(cell).join(&self.prefix).join(path))
        }

        fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
            vec![ConfigFile::new(
                format!("{}.config", self.prefix),
                format!("cells = {}\n", ctx.cells.len()),
            )]
        }
    }

    #[test]
    fn test_custom_layout_implementation() {
        let layout = TestCustomLayout::new("deps");
        let ctx = test_context();

        assert_eq!(layout.name(), "test-custom");

        let path = layout.map_dep(&ctx, "godeps", Path::new("vendor/foo"));
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/firefly/turnkey/external/godeps/deps/vendor/foo"
            ))
        );

        let configs = layout.generate_config(&ctx);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "deps.config");
        assert!(configs[0].content.contains("cells = 2"));
    }

    #[test]
    fn test_custom_layout_in_registry() {
        let mut registry = LayoutRegistry::new();
        registry.register("test-custom", || {
            Box::new(TestCustomLayout::new("my-prefix"))
        });

        let layout = registry.get("test-custom").unwrap();
        assert_eq!(layout.name(), "test-custom");

        let ctx = test_context();
        let configs = layout.generate_config(&ctx);
        assert_eq!(configs[0].name, "my-prefix.config");
    }
}
