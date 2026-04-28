# FUSE Composition Layer

The FUSE composition layer provides a unified filesystem view of repositories
and their dependencies. This document covers the architecture for developers
extending or maintaining the composition system.

## Architecture Overview

```
┌────────────────────────────────────────────────────────────────┐
│                   CompositionBackend trait                     │
├────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────┐       ┌─────────────────────┐         │
│  │   FUSE Backend      │       │   Symlink Backend   │         │
│  │   (Development)     │       │   (CI / Fallback)   │         │
│  └─────────────────────┘       └─────────────────────┘         │
│              │                           │                     │
│              └───────────┬───────────────┘                     │
│                          ▼                                     │
│              ┌───────────────────────┐                         │
│              │   Composition API     │                         │
│              │   (shared interface)  │                         │
│              └───────────────────────┘                         │
└────────────────────────────────────────────────────────────────┘
```

## Core Components

### Backend Trait

The `CompositionBackend` trait (`src/rust/composition/src/backend.rs`) defines
the interface for all backends:

```rust
pub trait CompositionBackend: Send + Sync {
    fn mount(&mut self) -> Result<()>;
    fn unmount(&mut self) -> Result<()>;
    fn status(&self) -> BackendStatus;
    fn cell_path(&self, cell: &str) -> Option<PathBuf>;
    fn refresh(&mut self) -> Result<()>;
}
```

### Backend Selection

The selector (`src/rust/composition/src/selector.rs`) automatically chooses the
appropriate backend:

```rust
pub fn select_backend(requested: BackendType) -> BackendSelection {
    match requested {
        BackendType::Auto => {
            if is_fuse_available() {
                BackendSelection::fuse("Auto-selected FUSE")
            } else {
                BackendSelection::symlink("Auto-selected symlinks (FUSE unavailable)")
            }
        }
        BackendType::Fuse => { /* ... */ }
        BackendType::Symlink => { /* ... */ }
    }
}
```

### State Machine

The consistency state machine (`src/rust/composition/src/state.rs`) manages
transitions:

```
Settled ──manifest change──► Syncing ──nix build──► Building
   ▲                                                    │
   │                                               build done
   │                                                    │
   └───────────────────── Transitioning ◄───────────────┘
```

Key types:

- `ConsistencyStateMachine` - Thread-safe state management
- `StateObserver` - Trait for state change notifications
- `CellUpdate` - Pending cell updates during transitions

### Policy System

The policy system (`src/rust/composition/src/policy.rs`) controls access during
updates. See [FUSE Access Policy](./fuse-policy.md) for details.

### Layout System

Layouts (`src/rust/composition/src/layout.rs`) control how files are presented:

- `Layout` trait - Core interface for layouts
- `LayoutRegistry` - Runtime layout registration
- `Buck2Layout` - Default Buck2 layout
- `BazelLayout` - Bazel layout

See [Custom Layouts](../extending/custom-layouts.md) for creating new layouts.

## Module Structure

```
src/rust/nix-eval/src/          # Nix client abstraction (replaceable)
├── lib.rs                      # NixClient trait
├── cli.rs                      # CliNixClient (shells out to nix binary)
└── error.rs

src/rust/composition/src/
├── lib.rs                      # Public API exports
├── backend.rs                  # CompositionBackend trait
├── compose_config.rs           # compose.toml parser (single-mount legacy)
├── config.rs                   # CompositionConfig, CellConfig
├── discover.rs                 # Cell discovery via NixClient trait
├── error.rs                    # Error types
├── layout.rs                   # Layout system (Buck2Layout, BazelLayout)
├── policy.rs                   # Access policies
├── recovery.rs                 # Error recovery utilities
├── selector.rs                 # Backend selection logic
├── serve_config.rs             # Service config ([[mounts]] TOML format)
├── service.rs                  # Launchd/systemd service generation
├── state.rs                    # Consistency state machine
├── status.rs                   # BackendStatus enum
├── symlink.rs                  # Symlink backend
├── synthetic.rs                # macOS synthetic firmlink management
├── tracing.rs                  # Logging and debugging
├── watcher.rs                  # File watching (optional)
└── fuse/                       # FUSE backend (feature-gated)
    ├── mod.rs                  # Re-exports FuseBackend (platform-conditional)
    ├── fs_core.rs              # Platform-agnostic filesystem logic
    ├── platform.rs             # Platform detection and FUSE availability
    ├── filesystem.rs           # Linux: fuser Filesystem trait impl
    ├── backend.rs              # Linux: FuseBackend using fuser crate
    ├── edit_overlay.rs         # Copy-on-write editing layer
    ├── patch_generator.rs      # Unified diff generation for edits
    └── fuse_t/                 # macOS: direct libfuse-t backend
        ├── mod.rs
        ├── bindings.rs         # Hand-written FFI bindings to libfuse3
        ├── operations.rs       # FUSE operation callbacks (path-based API)
        └── backend.rs          # FuseTBackend implementing CompositionBackend

nix/home-manager/
└── turnkey-composed.nix        # Home-manager module for service management
```

### Daemon Architecture

The `turnkey-composed` daemon supports two modes:

- **`start`**: Single mount, ad-hoc usage
- **`serve`**: Multi-mount service mode, reads config file, watches for
  changes

In service mode, the daemon:
1. Reads `~/.config/turnkey/composed.toml` for mount declarations
2. For each mount: discovers cells via `nix-eval` crate, builds them,
   creates the FUSE mount
3. Watches manifest files for dependency changes (triggers cell rebuild)
4. Watches the config file for new/removed mounts (hot-reload)
5. On macOS, manages synthetic firmlinks for mount points under `/`

### Nix Integration

Cell derivations are exposed as flake packages (`godeps-cell`,
`rustdeps-cell`, etc.) by the flake-parts module. The daemon builds them
via the `NixClient` trait (currently `CliNixClient` which shells out to
`nix`). This abstraction allows replacing the CLI with a direct Nix daemon
client when one becomes available.

## FUSE Backend Implementation

The FUSE backend uses a layered architecture with a platform-agnostic core and
platform-specific adapters.

### FsCore (Platform-Agnostic)

`FsCore` (`fs_core.rs`) contains all filesystem logic with **zero dependency on
the `fuser` crate**:

- **Path resolution**: `resolve_path(path) -> ResolvedPath` maps FUSE paths to
  logical locations (Root, Source, CellPrefix, Cell, VirtualFile, etc.)
- **Inode management**: Allocation, mapping, and lookup using plain `u64` inode
  numbers
- **Virtual file generation**: `.buckconfig` and `.buckroot` content
- **Policy checking**: Access control during dependency updates
- **Edit overlay**: Copy-on-write editing of external dependencies

Both the Linux and macOS backends delegate to `FsCore` for all filesystem logic,
converting between their own FUSE types and FsCore's neutral types.

### Linux Backend (fuser crate)

Uses the `fuser` crate's low-level inode-based API:

- `CompositionFs` wraps `FsCore` and implements `fuser::Filesystem`
- Converts between `fuser::INodeNo`/`FileAttr` and FsCore's `u64`/`FsAttr`
- Feature flag: `fuse` (enables `dep:fuser`)

### macOS Backend (FUSE-T FFI)

Uses direct C FFI to FUSE-T's libfuse3, bypassing the `fuser` crate entirely.
This is necessary because `fuser` reads the FUSE file descriptor directly, which
is incompatible with FUSE-T's NFS-based socket protocol.

- **`bindings.rs`**: Hand-written FFI bindings to libfuse3 (44-field
  `fuse_operations` struct at 352 bytes, `fuse_new`, `fuse_mount`, `fuse_loop`,
  etc.)
- **`operations.rs`**: `extern "C"` callbacks using the high-level path-based
  API. Each callback retrieves `FsCore` via a global `AtomicPtr` and delegates
  to `resolve_path()`
- **`backend.rs`**: `FuseTBackend` spawns a thread calling `fuse_new` +
  `fuse_mount` + `fuse_loop`
- Feature flag: `fuse-t` (only `dep:libc` needed)
- Links against `/usr/local/lib/libfuse3.dylib` (from FUSE-T)

**FUSE-T quirks discovered during implementation:**

- `fuse_get_context()->private_data` does not reliably pass the `user_data` from
  `fuse_new`. A global `AtomicPtr<FsCore>` is used instead.
- `readdir` filler must pass null for the stat buffer. FUSE-T's NFS translation
  rejects certain stat formats with "RPC struct is bad".
- The `fuse_operations` struct must include the newer `statx` and `syncfs`
  fields even if unused, to match the 352-byte C ABI.

### Conditional Compilation

Platform selection happens at compile time:

```rust
// In fuse/mod.rs:
#[cfg(target_os = "linux")]
pub use backend::FuseBackend;           // fuser-based

#[cfg(target_os = "macos")]
pub use fuse_t::backend::FuseTBackend as FuseBackend;  // libfuse-t FFI
```

The `selector.rs` gates on `#[cfg(any(feature = "fuse", feature = "fuse-t"))]`
so both feature flags enable the FUSE code path.

### Platform Detection

Runtime FUSE availability checking in `platform.rs`:

- **Linux**: Checks for `/dev/fuse`
- **macOS**: Checks for FUSE-T bundle (`/Library/Filesystems/fuse-t.fs`) or
  library (`/usr/local/lib/libfuse-t.dylib`)

## Recovery System

The recovery module (`src/rust/composition/src/recovery.rs`) provides:

### Retry Logic

```rust
pub async fn retry_with_backoff<T, F, Fut>(
    config: &RetryConfig,
    operation: F,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
```

### Error Classification

```rust
pub fn is_transient_error(error: &Error) -> bool {
    matches!(error, Error::Timeout(_) | Error::PathUpdating(_) | ...)
}
```

### Recovery Actions

```rust
pub enum RecoveryAction {
    Retry { delay: Duration },
    ForceUnmount,
    RestartDaemon,
    ManualIntervention { instructions: String },
}
```

## Tracing and Debugging

The tracing module (`src/rust/composition/src/tracing.rs`) provides:

### Configuration

```rust
pub struct TracingConfig {
    pub enable_fuse_ops: bool,
    pub enable_state_transitions: bool,
    pub enable_metrics: bool,
    pub log_level: String,
}
```

### State Logger

Implements `StateObserver` to log state transitions:

```rust
impl StateObserver for StateLogger {
    fn on_state_change(&self, from: SystemState, to: SystemState) {
        info!("State: {:?} -> {:?}", from, to);
    }
}
```

### Metrics

Tracks performance metrics:

- Operation counts (lookup, read, readdir, etc.)
- Latency histograms
- Cache hit rates

### Debug Information

```rust
pub struct DebugInfo {
    pub backend_type: String,
    pub mount_point: Option<PathBuf>,
    pub cells: Vec<CellDebugInfo>,
    pub state: SystemState,
    pub metrics: Option<Metrics>,
}
```

## Testing

### Unit Tests

Each module has unit tests:

```bash
cargo test -p composition
```

### Integration Tests

Test with actual FUSE mounts (requires FUSE):

```bash
# Linux
cargo test -p composition --features fuse -- --ignored

# macOS (FUSE-T)
cargo test -p composition --features fuse-t -- --ignored
```

### Mock Backend

For testing without FUSE:

```rust
use composition::testing::MockBackend;

let backend = MockBackend::new()
    .with_cell("godeps", "/nix/store/xxx-godeps")
    .with_status(BackendStatus::Ready);
```

## Feature Flags

The crate uses feature flags:

```toml
[features]
default = []
fuse = ["fuser"]       # Enable FUSE backend
watcher = ["notify"]   # Enable file watching
```

## Error Handling

The `Error` enum in `error.rs` covers all failure modes:

```rust
pub enum Error {
    AlreadyMounted(PathBuf),
    NotMounted,
    MountPointInaccessible { path, source },
    CellNotFound(String),
    FuseUnavailable(String),
    // ...
}
```

Errors include recovery suggestions:

```rust
impl Error {
    pub fn is_transient(&self) -> bool { /* ... */ }
    pub fn recovery_suggestion(&self) -> Option<String> { /* ... */ }
}
```

## Configuration

The `CompositionConfig` struct holds all settings:

```rust
pub struct CompositionConfig {
    pub mount_point: PathBuf,
    pub cells: HashMap<String, CellConfig>,
    pub consistency_mode: ConsistencyMode,
    pub layout: String,
}

pub struct CellConfig {
    pub source_path: PathBuf,
    pub editable: bool,
}
```

## Related Documentation

- [FUSE Access Policy](./fuse-policy.md) - Access control during updates
- [Custom Layouts](../extending/custom-layouts.md) - Creating build system
  layouts
- [Architecture Proposal](../../../../architecture/fuse-composition-layer.md) -
  Original design document
