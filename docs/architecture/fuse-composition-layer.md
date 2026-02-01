# FUSE Composition Layer: Architecture Proposal

## Overview

This document describes the architecture for an **optional** FUSE-based repository composition layer that provides:
- Fixed mount locations for predictable remote caching
- Pluggable layouts for different build systems (Buck2, Bazel, etc.)
- Transparent external dependency editing with automatic patch generation
- Consistency guarantees when underlying Nix derivations are updating

## Design Principles

### 1. Optional Enhancement, Not Replacement

The FUSE layer is an **optional enhancement** on top of the existing symlink-based approach:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Composition Backend                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────┐       ┌─────────────────────┐         │
│  │   FUSE Backend      │       │   Symlink Backend   │         │
│  │   (Development)     │       │   (CI / Fallback)   │         │
│  │                     │       │                     │         │
│  │  - Fixed paths      │       │  - .turnkey/ dir    │         │
│  │  - Edit support     │       │  - Nix store links  │         │
│  │  - Consistency      │       │  - Current approach │         │
│  └─────────────────────┘       └─────────────────────┘         │
│              │                           │                      │
│              └───────────┬───────────────┘                      │
│                          │                                      │
│              ┌───────────┴───────────┐                          │
│              │   Composition API     │                          │
│              │   (shared interface)  │                          │
│              └───────────────────────┘                          │
└─────────────────────────────────────────────────────────────────┘
```

**Selection criteria:**
- FUSE: When `turnkey.fuse.enable = true` and FUSE is available
- Symlinks: CI environments, containers without FUSE, explicit opt-out

### 2. Pluggable Layout System

Different build systems expect different directory structures. The layout system is pluggable:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Layout Plugins                                │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │  Buck2 Layout   │  │  Bazel Layout   │  │  Custom Layout  │ │
│  │                 │  │                 │  │                 │ │
│  │  /mount/        │  │  /mount/        │  │  (user-defined) │ │
│  │  ├── src/       │  │  ├── src/       │  │                 │ │
│  │  ├── external/  │  │  ├── external/  │  │                 │ │
│  │  │   ├── godeps/│  │  │   ├── @go//  │  │                 │ │
│  │  │   ├── rust/  │  │  │   ├── @rust//│  │                 │ │
│  │  │   └── ...    │  │  │   └── ...    │  │                 │ │
│  │  └── .buckconfig│  │  └── WORKSPACE  │  │                 │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘ │
│            │                  │                    │            │
│            └──────────────────┼────────────────────┘            │
│                               │                                 │
│               ┌───────────────┴───────────────┐                 │
│               │     Layout Trait/Interface    │                 │
│               │                               │                 │
│               │  - map_dep(cell, path) → path │                 │
│               │  - generate_config() → files  │                 │
│               │  - supported_cells() → list   │                 │
│               └───────────────────────────────┘                 │
└─────────────────────────────────────────────────────────────────┘
```

**Implementation:** The `Layout` trait is defined in `src/rust/composition/src/layout.rs`:

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

The `LayoutContext` provides all information needed for layout operations:
- `mount_point` - Where the composed view is mounted (e.g., `/firefly/turnkey`)
- `repo_root` - The repository root path
- `source_dir_name` - Name of the source overlay directory (e.g., "root")
- `cell_prefix` - Prefix for cell directories (e.g., "external")
- `cells` - List of `CellInfo` with name, source path, and editable flag

**Current Layouts:**
- `Buck2Layout` - Default layout generating `.buckconfig` and `.buckroot`
- `BazelLayout` - Bazel layout generating `WORKSPACE` and `BUILD.bazel`

**Creating Custom Layouts:**
```rust
use composition::layout::{Layout, LayoutContext, ConfigFile};

struct MyLayout;

impl Layout for MyLayout {
    fn name(&self) -> &'static str { "my-build-system" }

    fn map_dep(&self, ctx: &LayoutContext, cell: &str, path: &Path) -> Option<PathBuf> {
        Some(ctx.cell_path(cell).join(path))
    }

    fn generate_config(&self, ctx: &LayoutContext) -> Vec<ConfigFile> {
        vec![ConfigFile::new("my.config", "# config content")]
    }

    fn supported_cells(&self, ctx: &LayoutContext) -> Vec<String> {
        ctx.cells.iter().map(|c| c.name.clone()).collect()
    }
}
```

### 3. Fixed Mount Location

The FUSE layer mounts at a **configurable fixed location**, enabling:
- Predictable paths in built binaries → remote cache compatibility
- No "impure" Nix evaluation (paths are deterministic)
- Consistent paths across machines

**Example configuration:**
```nix
turnkey.fuse = {
  enable = true;
  mountPoint = "/firefly/turnkey";  # or derived from project name
  layout = "buck2";  # or "bazel", "custom"
};
```

**Resulting structure:**
```
/firefly/turnkey/
├── root/                   # OVERLAY on repo root (run Buck2 from here)
│   ├── .buckconfig         # Virtual - generated, shadows real if exists
│   ├── .buckroot           # Virtual - marks Buck2 root
│   ├── src/                # Pass-through from actual repo
│   │   ├── go/
│   │   ├── rust/
│   │   └── ...
│   ├── prelude/            # Pass-through from actual repo
│   └── ...                 # All other repo files pass-through
└── external/               # Pure virtual - dependency cells
    ├── godeps/             # Go dependencies (from Nix store)
    │   └── vendor/
    ├── rustdeps/           # Rust dependencies (from Nix store)
    │   └── vendor/
    └── ...
```

**Key insight:** Buck2 is run from `/firefly/turnkey/root/` where `.buckroot` exists.
This means `//docs/user-manual` resolves correctly (relative to `.buckroot` location),
making targets identical between FUSE and symlink approaches.

## Core Components

### 1. Composition Daemon (`turnkey-composed`)

A long-running Rust daemon that:
- Manages FUSE mount lifecycle
- Watches dependency manifests for changes
- Coordinates with Nix for derivation builds
- Provides consistency guarantees

```
┌─────────────────────────────────────────────────────────────────┐
│                   turnkey-composed daemon                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │   Watcher    │  │   Builder    │  │   Server     │          │
│  │              │  │              │  │              │          │
│  │  - inotify   │  │  - nix build │  │  - FUSE ops  │          │
│  │  - fsevents  │  │  - caching   │  │  - passthru  │          │
│  │  - debounce  │  │  - locking   │  │  - overlay   │          │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘          │
│         │                 │                 │                   │
│         └─────────────────┼─────────────────┘                   │
│                           │                                     │
│               ┌───────────┴───────────┐                         │
│               │    State Machine      │                         │
│               │                       │                         │
│               │  IDLE → UPDATING →    │                         │
│               │  BUILDING → READY     │                         │
│               └───────────────────────┘                         │
│                                                                  │
├─────────────────────────────────────────────────────────────────┤
│                    IPC Interface                                 │
│  - Unix socket: /run/turnkey-composed/<project>.sock            │
│  - Commands: status, refresh, unmount, edit-mode                │
└─────────────────────────────────────────────────────────────────┘
```

### 2. Consistency Layer (Pluggable Policy System)

The consistency layer uses a **pluggable policy system** that classifies files and
determines access behavior based on system state. See the
[FUSE Access Policy](../developer-manual/src/architecture/fuse-policy.md) documentation
for full details.

**File Classes:**
- `SourcePassthrough` - Repository source files (always accessible)
- `CellContent` - Dependency cell content (subject to policy)
- `VirtualGenerated` - Generated files like `.buckconfig`
- `VirtualDirectory` - Virtual directory structure

**System States:**
1. `Settled` - Filesystem is consistent, all reads allowed
2. `Syncing` - Manifest changed, preparing for update
3. `Building` - Nix derivation building
4. `Transitioning` - Atomically switching to new derivation
5. `Error` - System encountered an error

**Built-in Policies:**

| Policy | Behavior |
|--------|----------|
| `StrictPolicy` | Block all cell access during updates |
| `LenientPolicy` | Allow stale reads, only block during transition |
| `CIPolicy` | Fail fast with EAGAIN on any conflict |
| `DevelopmentPolicy` | Balanced default (allow stale during sync, block during build) |

```rust
// Example: Using CI policy for fail-fast behavior
let fs = CompositionFs::with_policy(
    config,
    repo_root,
    state_machine,
    Box::new(CIPolicy::new()),
);
```

**Key design:** Source passthrough files are always accessible. Only dependency
cell content is subject to policy decisions, ensuring builds can always read
source code even during dependency updates.

### 3. Edit Layer (Copy-on-Write)

Enables editing external dependencies with automatic patch generation:

```
┌─────────────────────────────────────────────────────────────────┐
│                      Edit Layer                                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  external/godeps/vendor/github.com/spf13/cobra/                 │
│                         │                                        │
│                         ▼                                        │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Is file modified locally?                                │   │
│  │                                                           │   │
│  │  NO ─────────────────┐                                    │   │
│  │                      ▼                                    │   │
│  │              Read from Nix store                          │   │
│  │              /nix/store/xxx-cobra/...                     │   │
│  │                                                           │   │
│  │  YES ────────────────┐                                    │   │
│  │                      ▼                                    │   │
│  │              Read from overlay                            │   │
│  │              .turnkey/edits/godeps/cobra/...              │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  On write:                                                       │
│  1. Copy original to .turnkey/edits/                            │
│  2. Apply modification                                           │
│  3. Generate patch: .turnkey/patches/godeps/cobra.patch         │
│  4. Update Nix fixup to apply patch                             │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Workflow:**
1. Developer opens file in external dep
2. Makes changes and saves
3. FUSE layer intercepts write, stores in overlay
4. Background process generates unified diff
5. Patch is stored in repo (`.turnkey/patches/`)
6. Nix fixup configuration updated to apply patch
7. Next Nix rebuild includes the patch

### 4. Platform Backends

**Linux (fuser):**
- Native FUSE via `/dev/fuse`
- No external dependencies
- Best performance

**macOS (FUSE-T):**
- NFS-based, no kernel extension
- Requires FUSE-T installation
- Slightly higher latency

**Fallback (symlinks):**
- No daemon, just symlinks
- Used when FUSE unavailable
- CI environments

## Configuration

### Nix Module

```nix
{
  turnkey.fuse = {
    # Enable FUSE composition layer
    enable = true;

    # Mount point (fixed location for remote caching)
    mountPoint = "/firefly/${config.turnkey.projectName}";

    # Layout plugin
    layout = "buck2";  # "buck2" | "bazel" | "custom"

    # Consistency mode
    consistencyMode = "block";  # "block" | "stale" | "fail"

    # Enable edit layer for external dependencies
    enableEditing = true;

    # Patch output directory
    patchDir = ".turnkey/patches";

    # Fallback to symlinks if FUSE unavailable
    fallbackToSymlinks = true;
  };
}
```

### CLI Integration

```bash
# Start composition daemon
tk compose up

# Check status
tk compose status

# Force refresh
tk compose refresh

# Enable edit mode for a dependency
tk compose edit godeps/github.com/spf13/cobra

# Generate patches from edits
tk compose patch

# Stop daemon
tk compose down
```

## Implementation Phases

### Phase 1: Core Infrastructure
- [x] Composition trait/interface (Rust) - `src/rust/composition/`
- [x] Symlink backend (refactor existing code) - `src/rust/composition/src/symlink.rs`
- [x] FUSE backend skeleton (Linux only) - `src/rust/composition/src/fuse/`
- [x] Daemon lifecycle (start/stop) - `src/cmd/turnkey-composed/`

### Phase 2: Basic FUSE
- [x] Pass-through for src/ - `filesystem.rs` with inode management
- [x] Read-only external/ from Nix store - cell lookup and file access
- [x] Basic .buckconfig generation - virtual files in `filesystem.rs`
- [x] Linux testing - daemon start/stop, file operations verified

### Phase 3: Consistency Layer
- [x] Manifest watcher (inotify/fsevents) - `watcher.rs` with debouncing
- [x] State machine implementation - `state.rs` with thread-safe transitions
- [x] Pluggable policy system - `policy.rs` with FileClass, SystemState, PolicyDecision
- [x] Blocking reads during update - integrated into FUSE operations
- [x] Atomic view transitions - `CellUpdate` struct and `apply_pending_updates()` in `filesystem.rs`

### Phase 4: macOS Support
- [ ] FUSE-T backend
- [ ] Platform detection
- [ ] Cross-platform testing

### Phase 5: Edit Layer
- [x] Copy-on-write overlay - `edit_overlay.rs` with `EditOverlay` struct
- [x] Patch generation - `patch_generator.rs` with LCS-based unified diff
- [ ] Nix fixup integration
- [x] Edit workflow CLI - `src/cmd/tk/compose.go` with status/edit/patch/reset commands

### Phase 6: Layout Plugins
- [x] Layout trait definition - `layout.rs` with `Layout` trait
- [x] Buck2 layout (current) - `Buck2Layout` implementation
- [x] Bazel layout prototype - `BazelLayout` with WORKSPACE generation
- [ ] Custom layout API

### Phase 7: Production Hardening
- [ ] Error recovery
- [ ] Logging and debugging
- [ ] Performance optimization
- [ ] Documentation

## Benefits Summary

| Feature | Current (Symlinks) | FUSE Layer |
|---------|-------------------|------------|
| Path predictability | No (varies per machine) | Yes (fixed mount) |
| Remote caching | Limited | Full support |
| Nix impure flag | Required | Not required |
| Edit external deps | Manual patches | Transparent |
| Consistency | Manual refresh | Automatic |
| CI support | Yes | Yes (fallback) |
| Build system | Buck2 only | Pluggable |

## Open Questions

1. **Daemon startup**: Integrate with shell entry or separate command?
2. **Multiple projects**: One daemon per project or shared?
3. **Root permissions**: Can we avoid needing elevated permissions?
4. **Container support**: How to handle Docker/Podman environments?
