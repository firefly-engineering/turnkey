# Custom Layouts

The composition layer uses a pluggable layout system to support different build systems. While Turnkey ships with Buck2 and Bazel layouts, you can create custom layouts for other build systems or specialized requirements.

## Layout Architecture

Layouts control how the composed filesystem presents dependencies:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Layout System                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  LayoutContext ──────────────► Layout.map_dep()                 │
│  (mount point,                    │                              │
│   repo root,                      ▼                              │
│   cells)            /firefly/project/external/godeps/vendor/... │
│                                                                  │
│  LayoutContext ──────────────► Layout.generate_config()         │
│                                   │                              │
│                                   ▼                              │
│                            .buckconfig, .buckroot, etc.          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## The Layout Trait

All layouts implement the `Layout` trait:

```rust
use composition::layout::{Layout, LayoutContext, ConfigFile, CellInfo};
use std::path::{Path, PathBuf};

pub trait Layout: Send + Sync {
    /// Layout name (e.g., "buck2", "bazel", "custom")
    fn name(&self) -> &'static str;

    /// Map a dependency path to its composed location
    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf>;

    /// Generate configuration files for this build system
    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile>;

    /// List of cells this layout supports
    fn supported_cells(&self, ctx: &LayoutContext) -> Vec<String>;
}
```

## LayoutContext

The `LayoutContext` provides all information needed for layout operations:

```rust
pub struct LayoutContext {
    /// Mount point (e.g., "/firefly/turnkey")
    pub mount_point: PathBuf,

    /// Repository root (actual filesystem path)
    pub repo_root: PathBuf,

    /// Name of the source overlay directory (default: "root")
    pub source_dir_name: String,

    /// Prefix for cell directories (default: "external")
    pub cell_prefix: String,

    /// Available cells
    pub cells: Vec<CellInfo>,
}

pub struct CellInfo {
    /// Cell name (e.g., "godeps")
    pub name: String,

    /// Source path (Nix store path)
    pub source_path: PathBuf,

    /// Whether editing is enabled
    pub editable: bool,
}
```

### Helper Methods

```rust
impl LayoutContext {
    /// Get the path to a cell's directory
    /// e.g., "/firefly/turnkey/external/godeps"
    pub fn cell_path(&self, cell: &str) -> PathBuf;

    /// Get the root source directory path
    /// e.g., "/firefly/turnkey/root"
    pub fn source_path(&self) -> PathBuf;

    /// Check if a cell exists
    pub fn has_cell(&self, name: &str) -> bool;
}
```

## Creating a Custom Layout

### Basic Example

```rust
use composition::layout::{Layout, LayoutContext, ConfigFile};
use std::path::{Path, PathBuf};

pub struct PleaseLayout;

impl Layout for PleaseLayout {
    fn name(&self) -> &'static str {
        "please"
    }

    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
        // Map cell paths to Please's third_party structure
        // e.g., godeps -> third_party/go
        let target_dir = match cell {
            "godeps" => "third_party/go",
            "rustdeps" => "third_party/rust",
            "pydeps" => "third_party/python",
            _ => return None,
        };
        Some(ctx.mount_point.join(target_dir).join(path))
    }

    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
        // Generate .plzconfig at the root
        let config = format!(
            r#"[please]
version = >=17.0.0

[build]
path = {}

[go]
importpath = github.com/example/project
"#,
            ctx.source_path().display()
        );

        vec![ConfigFile::new(".plzconfig", config)]
    }

    fn supported_cells(&self, ctx: &LayoutContext) -> Vec<String> {
        ctx.cells
            .iter()
            .filter(|c| matches!(c.name.as_str(), "godeps" | "rustdeps" | "pydeps"))
            .map(|c| c.name.clone())
            .collect()
    }
}
```

### Using SimpleLayout

For quick prototyping, use `SimpleLayout` without implementing the full trait:

```rust
use composition::layout::{SimpleLayout, LayoutContext, ConfigFile};

let layout = SimpleLayout::new(
    "pants",
    |ctx, cell, path| {
        // Custom path mapping
        Some(ctx.mount_point.join("3rdparty").join(cell).join(path))
    },
    |ctx| {
        // Generate config files
        vec![
            ConfigFile::new("pants.toml", "[GLOBAL]\npants_version = \"2.18.0\""),
            ConfigFile::new("BUILD", "# Root BUILD file"),
        ]
    },
);
```

## Registering Custom Layouts

### Using the Global Registry

Register your layout at application startup:

```rust
use composition::layout::{global_registry, BoxedLayout};

fn register_layouts() {
    let registry = global_registry();
    registry.register(Box::new(PleaseLayout));
    registry.register(Box::new(PantsLayout::new()));
}
```

### Using a Custom Registry

For more control, create your own registry:

```rust
use composition::layout::{LayoutRegistry, BoxedLayout};

let mut registry = LayoutRegistry::new();
registry.register(Box::new(PleaseLayout));
registry.register(Box::new(CustomLayout::with_options(opts)));

// Look up by name
let layout = registry.get("please").expect("layout not found");

// List available layouts
for name in registry.available() {
    println!("Layout: {}", name);
}
```

## ConfigFile

Generated configuration files use the `ConfigFile` struct:

```rust
pub struct ConfigFile {
    /// Relative path within the composed view
    pub path: PathBuf,

    /// File content
    pub content: String,
}

impl ConfigFile {
    pub fn new(path: impl Into<PathBuf>, content: impl Into<String>) -> Self;
}
```

Common patterns:

```rust
// Root config file
ConfigFile::new(".buckconfig", "...")

// Nested path
ConfigFile::new("build/config.bzl", "...")

// Per-cell config
ConfigFile::new(format!("{}/{}/BUILD", ctx.cell_prefix, cell), "...")
```

## Layout Selection

Layouts are selected via configuration:

### Nix Configuration

```nix
turnkey.fuse = {
  enable = true;
  layout = "please";  # Use custom layout
};
```

### Runtime Selection

```rust
use composition::layout::{layout_by_name, default_layout};

// Get specific layout
let layout = layout_by_name("please")?;

// Or use default (buck2)
let layout = default_layout();
```

### Available Layouts

```rust
use composition::layout::available_layouts;

for name in available_layouts() {
    println!("Available: {}", name);
}
```

## Built-in Layouts

### Buck2Layout

The default layout for Buck2 projects:

- Maps cells to `external/<cell>/`
- Generates `.buckconfig` with cell mappings
- Generates `.buckroot` marker

```rust
use composition::layout::Buck2Layout;

let layout = Buck2Layout::new();
// or with custom prelude cell
let layout = Buck2Layout::with_prelude("custom-prelude");
```

### BazelLayout

For Bazel-based projects:

- Maps cells to `external/<cell>/`
- Generates `WORKSPACE` file
- Generates root `BUILD.bazel`

```rust
use composition::layout::BazelLayout;

let layout = BazelLayout::new();
```

## Testing Custom Layouts

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use composition::layout::{LayoutContext, CellInfo};
    use std::path::PathBuf;

    fn test_context() -> LayoutContext {
        LayoutContext {
            mount_point: PathBuf::from("/firefly/test"),
            repo_root: PathBuf::from("/home/user/project"),
            source_dir_name: "root".to_string(),
            cell_prefix: "external".to_string(),
            cells: vec![
                CellInfo {
                    name: "godeps".to_string(),
                    source_path: PathBuf::from("/nix/store/xxx-godeps"),
                    editable: false,
                },
            ],
        }
    }

    #[test]
    fn test_map_dep() {
        let layout = PleaseLayout;
        let ctx = test_context();

        let mapped = layout.map_dep(&ctx, "godeps", Path::new("vendor/foo"));
        assert_eq!(
            mapped,
            Some(PathBuf::from("/firefly/test/third_party/go/vendor/foo"))
        );
    }

    #[test]
    fn test_generate_config() {
        let layout = PleaseLayout;
        let ctx = test_context();

        let configs = layout.generate_config(&ctx);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].path, PathBuf::from(".plzconfig"));
        assert!(configs[0].content.contains("[please]"));
    }

    #[test]
    fn test_supported_cells() {
        let layout = PleaseLayout;
        let ctx = test_context();

        let cells = layout.supported_cells(&ctx);
        assert!(cells.contains(&"godeps".to_string()));
    }
}
```

## Best Practices

1. **Keep `map_dep` simple** - Just path manipulation, no I/O
2. **Generate minimal configs** - Only what the build system needs
3. **Support all standard cells** - godeps, rustdeps, pydeps, jsdeps
4. **Use `cell_path()` helper** - For consistent path construction
5. **Test with real build systems** - Verify generated configs work
6. **Document cell expectations** - What each cell should contain

## API Reference

### Module: `composition::layout`

**Traits:**
- `Layout` - Core layout trait

**Structs:**
- `LayoutContext` - Context for layout operations
- `LayoutRegistry` - Registry for custom layouts
- `CellInfo` - Information about a cell
- `ConfigFile` - Generated configuration file
- `SimpleLayout` - Quick layout without full trait impl
- `Buck2Layout` - Built-in Buck2 layout
- `BazelLayout` - Built-in Bazel layout

**Functions:**
- `global_registry()` - Get the global layout registry
- `available_layouts()` - List available layout names
- `layout_by_name(name)` - Get a layout by name
- `default_layout()` - Get the default layout (Buck2)

**Type Aliases:**
- `BoxedLayout` - `Box<dyn Layout>`
- `LayoutFactory` - `fn() -> BoxedLayout`
