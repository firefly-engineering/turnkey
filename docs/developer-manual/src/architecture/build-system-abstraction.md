# Build System Abstraction

Turnkey supports multiple build systems through abstraction layers that separate
build-system-agnostic specifications from build-system-specific rule generation.

## Overview

The abstraction pattern allows:

1. **Single source of truth** - Dependency specifications remain
   build-system-agnostic
2. **Pluggable generators** - Each build system implements its own rule
   generation
3. **Easy extensibility** - Adding new build systems requires only implementing
   generator protocols

```
┌─────────────────────────────────────────────────────────────────┐
│                   Generic Specification                         │
│  (NativeLibrarySpec, CellInfo, etc.)                            │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
     ┌────────────┐   ┌────────────┐   ┌────────────┐
     │   Buck2    │   │   Bazel    │   │   Future   │
     │ Generator  │   │ Generator  │   │ Generators │
     └────────────┘   └────────────┘   └────────────┘
              │               │               │
              ▼               ▼               ▼
     ┌────────────┐   ┌────────────┐   ┌────────────┐
     │ rules.star │   │ BUILD.bazel│   │    ...     │
     │ .buckconfig│   │ WORKSPACE  │   │            │
     └────────────┘   └────────────┘   └────────────┘
```

## Native Library Generation

### The Problem

Pre-compiled native libraries (like ring's crypto code) need different rules for
each build system:

| Build System | Rule Type                              | Example                        |
| ------------ | -------------------------------------- | ------------------------------ |
| Buck2        | `prebuilt_cxx_library` + `export_file` | Static linking with visibility |
| Bazel        | `cc_import`                            | Native C/C++ import            |

### The Abstraction

#### NativeLibrarySpec

A build-system-agnostic specification for native libraries:

```python
@dataclass
class NativeLibrarySpec:
    """Build-system-agnostic specification for a native library."""

    lib_name: str           # Target name (e.g., "ring_core_0_17_14__")
    static_lib_path: str    # Path to .a file (e.g., "out_dir/libring_core.a")
    link_search_path: str   # Rustc -L path (default: "out_dir")
```

This contains only the information needed to describe the library, not how to
build it.

#### NativeLibraryGenerator Protocol

Build systems implement this protocol:

```python
class NativeLibraryGenerator(Protocol):
    """Protocol for generating native library rules."""

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        """Generate build rules for a native library."""
        ...

    @property
    def name(self) -> str:
        """The build system name (e.g., 'buck2', 'bazel')."""
        ...
```

#### GeneratedRules

The output from a generator:

```python
@dataclass
class GeneratedRules:
    """Result of generating native library rules."""

    rules_content: str          # Generated rule definitions
    rules_to_load: list[str]    # Rules to load (e.g., ["prebuilt_cxx_library"])
    extra_deps: list[str]       # Dependencies to add to the crate
    extra_rustc_flags: list[str] # Rustc flags for linking
```

### Implementation Examples

#### Buck2 Generator

```python
class Buck2NativeLibraryGenerator:
    @property
    def name(self) -> str:
        return "buck2"

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        lines = [
            "export_file(",
            f'    name = "{spec.lib_name}_file",',
            f'    src = "{spec.static_lib_path}",',
            '    visibility = ["PUBLIC"],',
            ")",
            "",
            "prebuilt_cxx_library(",
            f'    name = "{spec.lib_name}",',
            f'    static_lib = ":{spec.lib_name}_file",',
            "    link_whole = True,",
            '    preferred_linkage = "static",',
            '    visibility = ["PUBLIC"],',
            ")",
        ]

        return GeneratedRules(
            rules_content="\n".join(lines),
            rules_to_load=["prebuilt_cxx_library", "export_file"],
            extra_deps=[f":{spec.lib_name}"],
            extra_rustc_flags=[f"-Lnative={spec.link_search_path}"],
        )
```

**Generated output:**

```starlark
export_file(
    name = "ring_core_0_17_14___file",
    src = "out_dir/libring_core_0_17_14__.a",
    visibility = ["PUBLIC"],
)

prebuilt_cxx_library(
    name = "ring_core_0_17_14__",
    static_lib = ":ring_core_0_17_14___file",
    link_whole = True,
    preferred_linkage = "static",
    visibility = ["PUBLIC"],
)
```

#### Bazel Generator

```python
class BazelNativeLibraryGenerator:
    @property
    def name(self) -> str:
        return "bazel"

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        lines = [
            "cc_import(",
            f'    name = "{spec.lib_name}",',
            f'    static_library = "{spec.static_lib_path}",',
            '    visibility = ["//visibility:public"],',
            ")",
        ]

        return GeneratedRules(
            rules_content="\n".join(lines),
            rules_to_load=["cc_import"],
            extra_deps=[f":{spec.lib_name}"],
            extra_rustc_flags=[f"-Lnative={spec.link_search_path}"],
        )
```

**Generated output:**

```starlark
cc_import(
    name = "ring_core_0_17_14__",
    static_library = "out_dir/libring_core_0_17_14__.a",
    visibility = ["//visibility:public"],
)
```

### File Locations

| File                                       | Purpose                                                         |
| ------------------------------------------ | --------------------------------------------------------------- |
| `src/python/buildsystem/__init__.py`       | Module exports                                                  |
| `src/python/buildsystem/native_library.py` | `NativeLibrarySpec`, `GeneratedRules`, `NativeLibraryGenerator` |
| `src/python/buildsystem/buck2.py`          | Buck2 implementation                                            |
| `src/python/buildsystem/bazel.py`          | Bazel implementation (proof of concept)                         |

### Usage in Generator

The `generator.py` uses the abstraction:

```python
from python.buildsystem.native_library import NativeLibrarySpec
from python.buildsystem.buck2 import buck2_generator

def generate_buck_file(..., native_lib_info: dict | None = None) -> str:
    if native_lib_info:
        spec = NativeLibrarySpec.from_dict(native_lib_info)
        generated = buck2_generator.generate(spec)

        rules_to_load.extend(generated.rules_to_load)
        deps = deps + generated.extra_deps
        rustc_flags = rustc_flags + generated.extra_rustc_flags
```

## Layout Trait (FUSE Composition)

The composition layer uses a similar pattern for file system layouts.

### Layout Trait

```rust
pub trait Layout: Send + Sync {
    /// Get the layout name (e.g., "buck2", "bazel")
    fn name(&self) -> &'static str;

    /// Map a dependency path to its location in the composed view
    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf>;

    /// Generate configuration files for this build system
    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile>;

    /// Get the list of cells this layout supports
    fn supported_cells(&self, ctx: &LayoutContext) -> Vec<String>;
}
```

### Buck2Layout Implementation

```rust
impl Layout for Buck2Layout {
    fn name(&self) -> &'static str { "buck2" }

    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
        vec![
            self.generate_buckconfig(ctx),
            ConfigFile::new(".buckroot", "# Buck2 repository root marker\n"),
        ]
    }

    // ...
}
```

See `src/rust/composition/src/layout.rs` for the full implementation.

## Adding a New Build System

### 1. Create Native Library Generator

```python
# src/python/buildsystem/newbuild.py
from .native_library import NativeLibrarySpec, GeneratedRules

class NewBuildNativeLibraryGenerator:
    @property
    def name(self) -> str:
        return "newbuild"

    def generate(self, spec: NativeLibrarySpec) -> GeneratedRules:
        # Generate rules for your build system
        lines = [
            f'native_lib(name = "{spec.lib_name}", ...)',
        ]
        return GeneratedRules(
            rules_content="\n".join(lines),
            rules_to_load=["native_lib"],
            extra_deps=[f":{spec.lib_name}"],
            extra_rustc_flags=[f"-Lnative={spec.link_search_path}"],
        )

newbuild_generator = NewBuildNativeLibraryGenerator()
```

### 2. Create Layout Implementation (for FUSE)

```rust
// src/rust/composition/src/layouts/newbuild.rs
pub struct NewBuildLayout;

impl Layout for NewBuildLayout {
    fn name(&self) -> &'static str { "newbuild" }

    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
        // Generate config files for your build system
        vec![ConfigFile::new("BUILD.newbuild", "# config")]
    }

    // ...
}
```

### 3. Register the Layout

```rust
// src/rust/composition/src/layout.rs
pub fn layout_by_name(name: &str) -> Option<BoxedLayout> {
    match name {
        "buck2" => Some(Box::new(Buck2Layout::new())),
        "newbuild" => Some(Box::new(NewBuildLayout::new())),
        _ => None,
    }
}
```

## Design Principles

1. **Specification vs Generation** - Keep specifications generic, push
   build-system details to generators
2. **Protocol-based** - Use protocols/traits for loose coupling
3. **Singleton instances** - Generators are stateless, use module-level
   instances
4. **Incremental adoption** - New build systems can be added without changing
   existing code
