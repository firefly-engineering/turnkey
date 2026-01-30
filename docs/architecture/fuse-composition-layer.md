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
├── src/                    # Pass-through to repo source
│   ├── go/
│   ├── rust/
│   └── ...
├── external/               # Composed dependency view
│   ├── godeps/             # Go dependencies
│   │   └── vendor/
│   ├── rustdeps/           # Rust dependencies
│   │   └── vendor/
│   └── ...
├── .buckconfig             # Generated for this mount
└── .buckroot
```

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

### 2. Consistency Layer

Guarantees filesystem consistency during updates:

**States:**
1. `READY` - Filesystem is consistent, all reads allowed
2. `UPDATING` - Manifest changed, blocking new reads to changed paths
3. `BUILDING` - Nix derivation building, reads to affected paths block
4. `TRANSITIONING` - Atomically switching to new derivation

**Behavior:**
- Reads to unaffected paths always succeed immediately
- Reads to affected paths during update either:
  - Block until new version ready (default)
  - Return stale data with warning (configurable)
  - Fail with EAGAIN (for non-blocking access)

```rust
enum AccessMode {
    BlockUntilReady,      // Default: wait for consistency
    AllowStale,           // Return old version during update
    FailIfUpdating,       // Return EAGAIN if path is updating
}
```

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
- [ ] Composition trait/interface (Rust)
- [ ] Symlink backend (refactor existing code)
- [ ] FUSE backend skeleton (Linux only)
- [ ] Daemon lifecycle (start/stop)

### Phase 2: Basic FUSE
- [ ] Pass-through for src/
- [ ] Read-only external/ from Nix store
- [ ] Basic .buckconfig generation
- [ ] Linux testing

### Phase 3: Consistency Layer
- [ ] Manifest watcher (inotify/fsevents)
- [ ] State machine implementation
- [ ] Blocking reads during update
- [ ] Atomic view transitions

### Phase 4: macOS Support
- [ ] FUSE-T backend
- [ ] Platform detection
- [ ] Cross-platform testing

### Phase 5: Edit Layer
- [ ] Copy-on-write overlay
- [ ] Patch generation
- [ ] Nix fixup integration
- [ ] Edit workflow CLI

### Phase 6: Layout Plugins
- [ ] Layout trait definition
- [ ] Buck2 layout (current)
- [ ] Bazel layout prototype
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
