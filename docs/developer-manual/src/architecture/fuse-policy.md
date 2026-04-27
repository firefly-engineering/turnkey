# FUSE Access Policy System

The FUSE composition layer includes a pluggable access policy system that
controls how file operations behave during dependency updates. This allows
developers to tune the trade-off between consistency and availability based on
their workflow.

## Overview

When the composition system is updating (rebuilding Nix derivations), file
access to dependency cells may need to be controlled. The policy system
determines whether to:

- **Allow** the operation immediately
- **Block** until the system becomes stable
- **Deny** with an error (e.g., EAGAIN)
- **Allow with stale data** and log a warning

```
┌─────────────────────────────────────────────────────────────┐
│                     FUSE Operation                          │
│   (lookup, getattr, read, readdir, write, create, ...)      │
└─────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                   Classify Request                          │
│  path/inode → FileClass                                     │
│  state machine → SystemState                                │
│  operation → OperationType                                  │
└─────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                   Policy.check()                            │
│  (FileClass, SystemState, OperationType) → PolicyDecision   │
└─────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                   Execute Decision                          │
│  Allow → proceed                                            │
│  Block → wait then retry                                    │
│  Deny → return errno                                        │
│  AllowStale → proceed with warning                          │
└─────────────────────────────────────────────────────────────┘
```

## Core Concepts

### File Classes

Files in the composition view are classified by their behavioral
characteristics:

| Class               | Description                 | Examples                        |
| ------------------- | --------------------------- | ------------------------------- |
| `SourcePassthrough` | Repository source files     | `src/main.rs`, `docs/README.md` |
| `CellContent`       | Dependency cell content     | `external/godeps/vendor/...`    |
| `VirtualGenerated`  | Generated virtual files     | `.buckconfig`, `.buckroot`      |
| `VirtualDirectory`  | Virtual directory structure | Mount root, cell prefix         |
| `EditLayer`         | User modifications (future) | Local patches to dependencies   |

**Key insight:** `SourcePassthrough` and virtual files are always accessible
regardless of system state. Only `CellContent` access is subject to policy
decisions.

### System States

The composition system transitions through these states:

```
Settled ──manifest change──► Syncing ──nix build──► Building
   ▲                                                    │
   │                                               build done
   │                                                    │
   └───────────────────── Transitioning ◄───────────────┘
```

| State           | Description                            |
| --------------- | -------------------------------------- |
| `Settled`       | System is stable, no pending changes   |
| `Syncing`       | Manifest changed, preparing for update |
| `Building`      | Nix derivation is building             |
| `Transitioning` | Atomically switching to new view       |
| `Error`         | System encountered an error            |

### Operation Types

| Operation                     | Description                   |
| ----------------------------- | ----------------------------- |
| `Lookup`                      | Path lookup (finding a file)  |
| `Getattr`                     | Get file/directory attributes |
| `Read`                        | Read file content             |
| `Readdir`                     | Read directory entries        |
| `Readlink`                    | Read symbolic link target     |
| `Open` / `Opendir`            | Open file/directory           |
| `Write` / `Create` / `Unlink` | Write operations (future)     |

## Built-in Policies

### StrictPolicy

**Best for:** CI pipelines, production builds where correctness is critical

Blocks all cell access during any update phase. Reads will never return stale
data, but may block for the duration of the Nix build.

```rust
StrictPolicy::new()                    // Default 5-minute timeout
StrictPolicy::with_timeout(Duration::from_secs(120))  // Custom timeout
```

| State         | CellContent | SourcePassthrough |
| ------------- | ----------- | ----------------- |
| Settled       | Allow       | Allow             |
| Syncing       | Block       | Allow             |
| Building      | Block       | Allow             |
| Transitioning | Block       | Allow             |

### LenientPolicy

**Best for:** Interactive development where latency matters

Allows stale reads during syncing and building phases, only blocks during the
brief transition phase.

```rust
LenientPolicy::new()
```

| State         | CellContent | SourcePassthrough |
| ------------- | ----------- | ----------------- |
| Settled       | Allow       | Allow             |
| Syncing       | AllowStale  | Allow             |
| Building      | AllowStale  | Allow             |
| Transitioning | Block       | Allow             |

### CIPolicy

**Best for:** CI/CD environments where blocking is undesirable

Never blocks - immediately returns EAGAIN if the operation would need to wait.
The caller can retry or handle the error.

```rust
CIPolicy::new()
```

| State         | CellContent   | SourcePassthrough |
| ------------- | ------------- | ----------------- |
| Settled       | Allow         | Allow             |
| Syncing       | Deny (EAGAIN) | Allow             |
| Building      | Deny (EAGAIN) | Allow             |
| Transitioning | Deny (EAGAIN) | Allow             |

### DevelopmentPolicy (Default)

**Best for:** Day-to-day development work

A balanced approach:

- Syncing: Allow stale reads (quick phase)
- Building: Block (wait for fresh data)
- Error: Allow stale (degrade gracefully)

```rust
DevelopmentPolicy::new()
```

| State         | CellContent | SourcePassthrough |
| ------------- | ----------- | ----------------- |
| Settled       | Allow       | Allow             |
| Syncing       | AllowStale  | Allow             |
| Building      | Block       | Allow             |
| Transitioning | Block       | Allow             |
| Error         | AllowStale  | Allow             |

## Creating Custom Policies

Implement the `AccessPolicy` trait to create custom behavior:

```rust
use composition::policy::{
    AccessPolicy, FileClass, SystemState, OperationType, PolicyDecision,
};
use std::time::Duration;

pub struct MyPolicy {
    block_timeout: Duration,
}

impl AccessPolicy for MyPolicy {
    fn check(
        &self,
        class: &FileClass,
        state: SystemState,
        op: OperationType,
    ) -> PolicyDecision {
        // Source files always accessible
        if class.is_always_accessible() {
            return PolicyDecision::Allow;
        }

        // Custom logic based on state and operation
        match (state, op) {
            // Allow reads during syncing
            (SystemState::Syncing, OperationType::Read) => {
                PolicyDecision::AllowStale
            }
            // Block lookups during building
            (SystemState::Building, OperationType::Lookup) => {
                PolicyDecision::Block {
                    timeout: self.block_timeout,
                }
            }
            // Fail fast for directory listing during updates
            (_, OperationType::Readdir) if state.is_updating() => {
                PolicyDecision::eagain()
            }
            // Default: allow
            _ => PolicyDecision::Allow,
        }
    }

    fn name(&self) -> &'static str {
        "my-policy"
    }

    fn description(&self) -> &'static str {
        "Custom policy with special handling for readdir"
    }
}
```

## Policy Decision Types

| Decision            | Behavior                            | Use Case                                |
| ------------------- | ----------------------------------- | --------------------------------------- |
| `Allow`             | Proceed immediately                 | Stable state, always-accessible files   |
| `Block { timeout }` | Wait up to timeout for stable state | Ensuring consistency during builds      |
| `Deny { errno }`    | Return error immediately            | CI environments, fail-fast scenarios    |
| `AllowStale`        | Proceed with warning log            | Interactive development, quick feedback |

### Convenience Constructors

```rust
PolicyDecision::block()                    // Block with 5-minute timeout
PolicyDecision::block_with_timeout(dur)    // Block with custom timeout
PolicyDecision::eagain()                   // Deny with EAGAIN (11)
PolicyDecision::ebusy()                    // Deny with EBUSY (16)
```

## Configuring the Policy

### In Rust Code

When creating a `CompositionFs`, use the `with_policy` constructor:

```rust
use composition::{CompositionConfig, CompositionFs};
use composition::policy::{CIPolicy, StrictPolicy};

// With CI policy
let fs = CompositionFs::with_policy(
    config,
    repo_root,
    state_machine,
    Box::new(CIPolicy::new()),
);

// With strict policy and custom timeout
let fs = CompositionFs::with_policy(
    config,
    repo_root,
    state_machine,
    Box::new(StrictPolicy::with_timeout(Duration::from_secs(60))),
);
```

### Via Nix Configuration (Future)

```nix
turnkey.fuse = {
  enable = true;

  # Policy selection
  accessPolicy = "development";  # "strict" | "lenient" | "ci" | "development"

  # Custom timeout for blocking policies
  blockTimeout = 300;  # seconds
};
```

### Environment Variables (Future)

```bash
# Override policy at runtime
TURNKEY_ACCESS_POLICY=ci tk build //...

# Custom timeout
TURNKEY_BLOCK_TIMEOUT=60 tk build //...
```

## Debugging Policies

Policy decisions are logged at debug level. Enable debug logging to see:

```
DEBUG Policy 'development': blocking for up to 300s until stable
DEBUG Policy 'ci': denying Readdir on CellContent { cell: "godeps" } in state Building
WARN  Policy 'lenient': returning potentially stale data for cell 'godeps' during Building
```

## Guidelines for Choosing a Policy

| Scenario                | Recommended Policy                                |
| ----------------------- | ------------------------------------------------- |
| CI/CD pipelines         | `CIPolicy` - fail fast, let retry logic handle it |
| Production builds       | `StrictPolicy` - correctness over speed           |
| Interactive development | `DevelopmentPolicy` - balanced default            |
| Quick iteration         | `LenientPolicy` - maximum availability            |
| Custom requirements     | Implement `AccessPolicy` trait                    |

## API Reference

### Module: `composition::policy`

**Types:**

- `FileClass` - File classification enum
- `SystemState` - System state enum
- `OperationType` - Operation type enum
- `PolicyDecision` - Decision enum
- `AccessPolicy` - Policy trait
- `BoxedPolicy` - Type alias for `Box<dyn AccessPolicy>`

**Built-in Policies:**

- `StrictPolicy`
- `LenientPolicy`
- `CIPolicy`
- `DevelopmentPolicy`

**Functions:**

- `default_policy()` - Returns a boxed `DevelopmentPolicy`

**Constants:**

- `EAGAIN` - Resource temporarily unavailable (11)
- `EBUSY` - Device or resource busy (16)
