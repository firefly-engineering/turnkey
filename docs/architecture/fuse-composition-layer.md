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

**Selection criteria** (automatic via `selector.rs`):
- FUSE: When FUSE is available (Linux native, macOS FUSE-T) and not explicitly disabled
- Symlinks: CI environments, containers without FUSE, explicit `--backend=symlink`

**Backend selection API:**
```rust
use composition::{create_backend, BackendType, CompositionConfig};

// Auto-select best backend based on platform and availability
let backend = create_backend(BackendType::Auto, config)?;

// Or explicitly request a specific backend
let backend = create_backend(BackendType::Fuse, config)?;    // Requires FUSE
let backend = create_backend(BackendType::Symlink, config)?; // Always available
```

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
6. Next Nix rebuild includes the patch automatically

**Nix Integration:**

Patches generated by `tk compose patch` are automatically applied during cell builds
when `turnkey.toolchains.buck2.tk.userPatchesDir` is configured:

```nix
# In flake.nix or turnkey config
turnkey.toolchains.buck2.tk.userPatchesDir = ./.turnkey/patches;
```

The `genericMkDepsCell` function in `nix/lib/deps-cell/default.nix` applies patches
after copying dependencies but before running language-specific merge commands:

```
Copy deps → Apply user patches → Create symlinks → Run merge commands → Generate .buckconfig
```

Patch files use unified diff format with paths like `a/vendor/...` and `b/vendor/...`,
enabling `-p1` stripping during application. Patch naming follows the pattern:
`<cellName>/<path-with-slashes-as-dashes>.patch`

Example: A patch to `vendor/serde@1.0.219/src/lib.rs` would be named:
`rustdeps/vendor-serde@1.0.219-src-lib.rs.patch`

### 4. Platform Backends

**Linux (fuser):**
- Native FUSE via `/dev/fuse`
- Uses `fusermount3` or `fusermount` for unmount
- No external dependencies (FUSE is typically available)
- Best performance

**macOS (FUSE-T):**
- NFS-based, no kernel extension required
- Works on Apple Silicon (M1/M2/M3)
- Uses standard `umount` for unmount
- Installation: `brew install macos-fuse-t/homebrew-cask/fuse-t`
- Slightly higher latency due to NFS layer

**Platform detection** (`platform.rs`):
```rust
// Check FUSE availability
let availability = check_fuse_availability();
match availability {
    FuseAvailability::Available { implementation, version } => {
        // FUSE is ready to use
    }
    FuseAvailability::NotInstalled { install_instructions } => {
        // Show installation guidance
    }
    FuseAvailability::UnsupportedPlatform => {
        // Fall back to symlinks
    }
}
```

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
- [x] FUSE-T backend - `platform.rs` with macOS-specific mount/unmount
- [x] Platform detection - `Platform::detect()` and `check_fuse_availability()`
- [ ] Cross-platform testing

### Phase 5: Edit Layer
- [x] Copy-on-write overlay - `edit_overlay.rs` with `EditOverlay` struct
- [x] Patch generation - `patch_generator.rs` with LCS-based unified diff
- [x] Nix fixup integration - `userPatchesDir` parameter in `genericMkDepsCell` and adapters
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

## Build System Change Notification

A critical challenge for the FUSE backend is **notifying Buck2 when cell content
changes**. This section documents the problem, the constraints imposed by the
Linux kernel, and the available strategies.

### The problem

When the VFS daemon switches a cell from one Nix store path to another (e.g.
after `nix flake update`), the mount path stays stable but the content behind it
changes. Buck2 needs to know which files changed so its DICE engine can
invalidate the right cache entries. Without notification, Buck2 serves stale
data from its in-memory cache indefinitely.

### Why inotify doesn't work automatically

inotify fires events only in response to **explicit VFS operations** (write,
rename, unlink). When the FUSE daemon silently starts serving different content
for the same path, no VFS write occurs — the kernel has no way to know the
content changed, and no inotify event is generated.

The `fuser` crate's `Notifier` API reflects this kernel limitation:

| Method | Kernel cache effect | inotify effect |
|--------|-------------------|----------------|
| `inval_inode()` | Drops page cache | **None** |
| `inval_entry()` | Drops dentry cache | **None** |
| `delete()` | Drops dentry, unhashes | **IN_DELETE only** |
| `store()` | Updates cached data | **None** |

A `FUSE_FSNOTIFY` kernel patch that would enable full inotify from FUSE daemons
has been [RFC'd since 2021](https://patchwork.kernel.org/project/linux-fsdevel/cover/20211025204634.2517-1-iangelak@redhat.com/)
but is **not merged** as of Linux 6.18.

**References:**
- [libfuse wiki: Fsnotify and FUSE](https://github.com/libfuse/libfuse/wiki/Fsnotify-and-FUSE)
- [LWN: Inotify support in FUSE and virtiofs](https://lwn.net/Articles/874000/)
- [gocryptfs #215](https://github.com/rfjakob/gocryptfs/issues/215) — confirms the limitation

### What the VFS can guarantee today

Even without inotify, the daemon **can** ensure stale content is never served on
read. After switching a cell's backing store path:

1. Call `Notifier::inval_entry()` on every directory entry under the cell.
2. Call `Notifier::inval_inode()` on every file inode whose content changed.

This forces the kernel to re-fetch content from the daemon on the next access.
The result: any process that reads a file after the transition gets fresh data.
The problem is purely that **no watcher is told to re-read**.

### DICE early cutoff — why surgical notification matters

Buck2's DICE engine implements **early cutoff**: when a recomputed value equals
its previous value, reverse dependencies are not invalidated. This means that if
we can tell Buck2 "these files may have changed, please re-read them," DICE will
automatically limit the blast radius:

- Files whose content is identical across store paths → no downstream rebuild
- Files that actually changed → only affected targets rebuild

This makes surgical notification far superior to `buck2 kill`, which restarts the
daemon process even though DICE would have skipped most recomputation anyway.

### Notification strategies

#### Strategy 1: Stamp file + daemon kill (current, symlink backend)

The `cellfresh` package (`src/go/pkg/cellfresh/`) detects when `.turnkey/*`
symlinks change target and runs `buck2 kill`. This works for the symlink backend
and as a fallback for the FUSE backend:

1. VFS daemon writes updated targets to `.turnkey/.cell-targets` on the **real**
   filesystem (not the FUSE mount) after each transition.
2. `tk` reads the stamp file before delegating to Buck2 and kills the daemon if
   targets changed.

**Trade-off:** Kills the entire daemon. DICE early cutoff still limits rebuild
scope, but the daemon restart itself costs a few hundred milliseconds and loses
in-memory state.

#### Strategy 2: Sideband journal API (recommended, EdenFS pattern)

The VFS daemon maintains an internal journal of cell transitions with
per-file granularity. A custom Buck2 file watcher queries this journal
instead of relying on inotify.

This is the pattern Meta uses with EdenFS. Watchman has a special "eden" watcher
that queries EdenFS via a Thrift API rather than using inotify:

```thrift
// EdenFS Thrift API (for reference — our equivalent would be simpler)
FileDelta getFilesChangedSince(mountPoint, fromPosition)
JournalPosition getCurrentJournalPosition(mountPoint)
```

**For Turnkey, the equivalent would be:**

1. The VFS daemon exposes a Unix socket or file-based journal at
   `/run/turnkey-composed/<project>.sock` (already planned in the IPC
   interface).
2. On each cell transition, the daemon computes a file-level diff between
   old and new store paths and appends entries to the journal.
3. A custom Buck2 file watcher (or a Watchman plugin) queries the journal
   for changes since its last known position.
4. Changed files are fed into DICE as leaf invalidations.
5. DICE's early cutoff handles the rest — unchanged files cause no rebuilds.

**Trade-off:** Requires implementing a custom Buck2 file watcher, but gives
optimal incremental builds with zero daemon restarts.

#### Strategy 3: Custom Watchman watcher

If Watchman is already in use, implement a custom watcher SCM that queries
the VFS daemon's journal (identical to Strategy 2, but integrated through
Watchman's watcher plugin system rather than a custom Buck2 watcher).

### Recommended approach

**Short term:** Strategy 1 (stamp file + `buck2 kill`) works today and is
already implemented via `cellfresh`. The FUSE daemon should write
`.turnkey/.cell-targets` after each transition so `cellfresh` works
identically across both backends.

**Medium term:** Strategy 2 (sideband journal). The VFS daemon already has the
state machine, IPC socket, and per-cell transition tracking needed. The
remaining work is:
1. Computing file-level diffs during transitions (compare old vs new store paths)
2. Exposing a journal query API on the IPC socket
3. Writing a custom Buck2 file watcher that queries the journal

The file-level diff is cheap: Nix store paths are immutable, so a simple
recursive directory comparison with content hashing identifies exactly which
files changed. The VFS daemon can do this during the `Transitioning` state
(when reads are blocked anyway).

### Key insight

The FUSE backend eliminates the **cell path resolution** problem entirely:
mount paths are stable, so Buck2's cell resolver never goes stale. The only
remaining problem is **change notification** — telling Buck2 which files to
re-read. This is a strictly smaller problem than what the symlink backend
faces, and it has a clean solution via the sideband journal pattern.

## Open Questions

1. **Daemon startup**: Integrate with shell entry or separate command?
2. **Multiple projects**: One daemon per project or shared?
3. **Root permissions**: Can we avoid needing elevated permissions?
4. **Container support**: How to handle Docker/Podman environments?
5. **Journal format**: What wire format for the sideband change journal? (Protobuf, JSON lines, custom binary?)
6. **Buck2 file watcher**: Implement as a Watchman plugin or a native Buck2 watcher? (Buck2 supports pluggable watchers via `[buck2] file_watcher` in `.buckconfig`)
