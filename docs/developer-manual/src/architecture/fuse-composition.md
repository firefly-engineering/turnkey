# FUSE Composition Layer

The FUSE composition layer provides a unified filesystem view of repositories and their dependencies. This document covers the architecture for developers extending or maintaining the composition system.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    CompositionBackend trait                      │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────┐       ┌─────────────────────┐         │
│  │   FUSE Backend      │       │   Symlink Backend   │         │
│  │   (Development)     │       │   (CI / Fallback)   │         │
│  └─────────────────────┘       └─────────────────────┘         │
│              │                           │                      │
│              └───────────┬───────────────┘                      │
│                          ▼                                      │
│              ┌───────────────────────┐                          │
│              │   Composition API     │                          │
│              │   (shared interface)  │                          │
│              └───────────────────────┘                          │
└─────────────────────────────────────────────────────────────────┘
```

## Core Components

### Backend Trait

The `CompositionBackend` trait (`src/rust/composition/src/backend.rs`) defines the interface for all backends:

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

The selector (`src/rust/composition/src/selector.rs`) automatically chooses the appropriate backend:

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

The consistency state machine (`src/rust/composition/src/state.rs`) manages transitions:

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

The policy system (`src/rust/composition/src/policy.rs`) controls access during updates. See [FUSE Access Policy](./fuse-policy.md) for details.

### Layout System

Layouts (`src/rust/composition/src/layout.rs`) control how files are presented:

- `Layout` trait - Core interface for layouts
- `LayoutRegistry` - Runtime layout registration
- `Buck2Layout` - Default Buck2 layout
- `BazelLayout` - Bazel layout

See [Custom Layouts](../extending/custom-layouts.md) for creating new layouts.

## Module Structure

```
src/rust/composition/src/
├── lib.rs           # Public API exports
├── backend.rs       # CompositionBackend trait
├── config.rs        # CompositionConfig, CellConfig
├── error.rs         # Error types
├── layout.rs        # Layout system
├── policy.rs        # Access policies
├── recovery.rs      # Error recovery utilities
├── selector.rs      # Backend selection logic
├── state.rs         # Consistency state machine
├── status.rs        # BackendStatus enum
├── symlink.rs       # Symlink backend
├── tracing.rs       # Logging and debugging
├── watcher.rs       # File watching (optional)
└── fuse/            # FUSE backend (feature-gated)
    ├── mod.rs
    ├── filesystem.rs
    ├── platform.rs
    └── ...
```

## FUSE Backend Implementation

The FUSE backend (`src/rust/composition/src/fuse/`) implements the composition filesystem:

### Inode Management

The filesystem uses an inode-based approach:
- Root inode (1) represents the mount point
- Source directory has a dedicated inode
- Each cell gets a range of inodes
- Virtual files (config) get special inodes

### File Operations

Key FUSE operations:
- `lookup` - Resolve path to inode
- `getattr` - Get file attributes
- `read` - Read file content
- `readdir` - List directory entries
- `readlink` - Read symlink target

### Platform Abstraction

Platform-specific code is isolated in `platform.rs`:
- Linux: Native FUSE via `/dev/fuse`
- macOS: FUSE-T via NFS emulation

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
cargo test -p composition --features fuse -- --ignored
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
- [Custom Layouts](../extending/custom-layouts.md) - Creating build system layouts
- [Architecture Proposal](../../../../architecture/fuse-composition-layer.md) - Original design document
