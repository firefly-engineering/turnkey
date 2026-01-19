# FUSE-Based Repository Composition: Research Findings

This document captures research into using FUSE (Filesystem in Userspace) to create a transparent composition layer for turnkey's dependency management.

## Executive Summary

**Recommendation**: Use a **Rust-based FUSE implementation** with platform-specific backends:
- **Linux**: Native FUSE via `fuser` crate (pure Rust, no libfuse dependency)
- **macOS**: FUSE-T via NFS protocol (kext-less, stable)

This approach provides the best balance of performance, maintainability, and cross-platform support.

## Problem Statement

Currently, turnkey manages dependency cells as symlinked Nix store paths:

```
.turnkey/
├── godeps -> /nix/store/xxx-godeps-cell
├── rustdeps -> /nix/store/yyy-rustdeps-cell
└── toolchains -> /nix/store/zzz-toolchains-cell
```

**Pain points**:
1. Cells must be explicitly refreshed when dependencies change
2. Requires re-entering `nix develop` to pick up new derivations
3. Build system is tightly coupled to Buck2's cell layout
4. No support for alternative build systems (Bazel, etc.)

## Technology Options Evaluated

### 1. FUSE-T (macOS)

**Website**: https://www.fuse-t.org/

**Architecture**: Kext-less FUSE implementation using NFSv4 local server.

```
User Process → FUSE-T NFS Server → NFS RPC → FUSE Requests → Filesystem Implementation
```

**Pros**:
- No kernel extension required (major win for macOS stability)
- Uses macOS's native, optimized NFSv4 client
- Drop-in replacement for macFUSE API
- Better performance than macFUSE in many cases

**Cons**:
- macOS-only
- Requires NFS protocol overhead (minimal for local operations)
- Less mature than Linux FUSE

**API Compatibility**: Compatible with osxfuse/macFUSE libfuse headers.

### 2. Linux FUSE (libfuse / fuse3)

**Architecture**: Kernel module + userspace library.

```
User Process → Kernel FUSE Module → /dev/fuse → libfuse → Filesystem Implementation
```

**Pros**:
- Mature, battle-tested (20+ years)
- Excellent performance with modern kernel optimizations
- Rich ecosystem of tools and libraries

**Cons**:
- Requires FUSE kernel module (usually available by default)
- Different API from macOS implementations

### 3. 9P Protocol (Plan 9 Filesystem)

**Architecture**: Network filesystem protocol, simpler than NFS.

```
User Process → Kernel v9fs → 9P Protocol → 9P Server → Virtual Filesystem
```

**Pros**:
- Very simple protocol, easy to implement
- Built into Linux kernel (`mount -t 9p`)
- Good for network-transparent filesystems
- Excellent Rust libraries (`rs9p`, `rust-9p`)

**Cons**:
- No native macOS support (would need FUSE wrapper)
- Less feature-rich than FUSE
- Performance overhead for local use

### 4. NFS (Network File System)

**Pros**:
- Native support on all platforms
- FUSE-T already uses this approach successfully
- Well-understood, robust protocol

**Cons**:
- Complex protocol, harder to implement custom semantics
- Overkill for local filesystem composition

### 5. OverlayFS (Linux-only)

**Pros**:
- Kernel-native, excellent performance
- Perfect for layered filesystem composition

**Cons**:
- Linux-only (no macOS support)
- Requires root/CAP_SYS_ADMIN

## Language & Library Evaluation

### Rust Libraries

| Library | Platform | Notes |
|---------|----------|-------|
| [fuser](https://github.com/cberner/fuser) | Linux, FreeBSD, macOS | Pure Rust rewrite of libfuse. No C dependencies on Linux. Actively maintained (1.1k stars). |
| [rs9p](https://docs.rs/rs9p) | Linux | Async 9P2000.L server. Tokio-based. Good for network scenarios. |
| [rust-vfs](https://github.com/manuel-woelker/rust-vfs) | Cross-platform | Virtual filesystem abstraction. Includes OverlayFS, MemoryFS, PhysicalFS. |

**Recommendation**: `fuser` for FUSE implementation, with optional `rust-vfs` for internal abstractions.

### Go Libraries

| Library | Platform | Notes |
|---------|----------|-------|
| [go-fuse](https://github.com/hanwen/go-fuse) | Linux, macOS, FreeBSD | Mature, well-documented. Wire protocol-close API. |
| [go9p](https://pkg.go.dev/github.com/knusbaum/go9p) | Linux | 9P2000 server implementation. |
| [styx](https://github.com/droyo/styx) | Cross-platform | Stateful 9P2000 library. |

**Evaluation**: go-fuse is solid but macOS support has known limitations (STATFS overhead, no NOTIFY support).

### Language Recommendation: Rust

**Rationale**:
1. **Performance**: fuser's pure Rust implementation avoids C FFI overhead
2. **Safety**: Memory safety critical for filesystem code
3. **Ecosystem**: Better cross-platform FUSE support than Go
4. **Precedent**: envfs (similar problem domain) successfully uses Rust + FUSE

## Relevant Prior Art

### envfs (Mic92/envfs)

**What it does**: FUSE filesystem that dynamically populates `/bin` and `/usr/bin` with executables from the requesting process's PATH.

**Architecture patterns we can learn from**:
- Lazy resolution (only resolve on access, not on `readdir`)
- Process-context-aware responses
- Symlink generation to actual store paths

**Implementation**: Rust (79%), FUSE-based

### narfuse (taktoa/narfuse)

**What it does**: Mounts NAR (Nix archive) files as virtual Nix store.

**Relevance**: Shows that Nix store content can be exposed via FUSE. Different use case (archives vs. live derivations) but similar pattern.

**Implementation**: Haskell

## Proposed Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    turnkey-fuse daemon                       │
│                       (Rust binary)                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │ Layout:     │  │ Layout:     │  │ Layout:     │         │
│  │ Buck2       │  │ Bazel       │  │ Custom      │         │
│  │             │  │             │  │             │         │
│  │ .turnkey/   │  │ external/   │  │ ...         │         │
│  │ godeps/     │  │ @godeps//   │  │             │         │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘         │
│         │                │                │                 │
│         └────────────────┼────────────────┘                 │
│                          │                                  │
│              ┌───────────┴───────────┐                      │
│              │   Virtual FS Layer    │                      │
│              │   (rust-vfs traits)   │                      │
│              └───────────┬───────────┘                      │
│                          │                                  │
│  ┌───────────────────────┴───────────────────────┐         │
│  │              Nix Store Backend                 │         │
│  │                                                │         │
│  │  - Watch go-deps.toml, rust-deps.toml, etc.   │         │
│  │  - Evaluate Nix expressions on change         │         │
│  │  - Cache derivation outputs                   │         │
│  │  - Lazy build (only when accessed)            │         │
│  └───────────────────────────────────────────────┘         │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                   Platform Backend                           │
│  ┌─────────────────────┐  ┌─────────────────────┐          │
│  │   Linux: fuser      │  │   macOS: FUSE-T     │          │
│  │   (native FUSE)     │  │   (NFS backend)     │          │
│  └─────────────────────┘  └─────────────────────┘          │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

1. **Daemon Process**: Long-running process that:
   - Watches dependency manifests (go-deps.toml, etc.)
   - Triggers Nix rebuilds when sources change
   - Serves filesystem requests

2. **Lazy Evaluation**: Only build derivations when paths are accessed, not upfront.

3. **Layout Plugins**: Different build systems get different directory layouts, all backed by the same Nix derivations.

4. **Platform Abstraction**: Single codebase with compile-time or runtime platform selection.

## Implementation Phases

### Phase 1: Proof of Concept
- [ ] Basic Rust FUSE daemon using `fuser`
- [ ] Mount single godeps cell as read-only filesystem
- [ ] Linux-only initially

### Phase 2: Nix Integration
- [ ] Watch dependency manifests for changes
- [ ] Shell out to `nix build` on change
- [ ] Update filesystem view atomically

### Phase 3: macOS Support
- [ ] Integrate FUSE-T backend
- [ ] Test on Apple Silicon and Intel Macs
- [ ] Handle macOS-specific mount semantics

### Phase 4: Multi-Layout Support
- [ ] Buck2 layout (current cell structure)
- [ ] Bazel layout (`external/@repo//package`)
- [ ] Configuration for custom layouts

### Phase 5: Production Hardening
- [ ] Graceful daemon lifecycle (systemd/launchd integration)
- [ ] Error recovery and logging
- [ ] Performance optimization (caching, prefetching)
- [ ] CLI integration (`tk mount`, `tk unmount`)

## Performance Considerations

| Aspect | Approach |
|--------|----------|
| **Startup latency** | Lazy derivation building - only build what's accessed |
| **Read latency** | Direct passthrough to Nix store (just symlinks/hardlinks) |
| **Directory listing** | Cache and pre-compute for common patterns |
| **Change detection** | inotify/FSEvents on manifest files |
| **Memory usage** | Minimal - just metadata, not file contents |

## Open Questions

1. **Daemon lifecycle**: How to start/stop? Integrated with `nix develop` or separate?

2. **Failure modes**: What happens when Nix build fails? Show error file? Empty directory?

3. **Concurrent access**: Multiple processes building same derivation?

4. **Remote builds**: Can we integrate with Nix remote builders?

5. **Cache invalidation**: When does a derivation need rebuilding? Content hash vs. timestamp?

## References

- [FUSE-T](https://www.fuse-t.org/) - Kext-less FUSE for macOS
- [fuser](https://github.com/cberner/fuser) - Rust FUSE library
- [go-fuse](https://github.com/hanwen/go-fuse) - Go FUSE bindings
- [envfs](https://github.com/Mic92/envfs) - Dynamic executable resolution via FUSE
- [narfuse](https://github.com/taktoa/narfuse) - NAR files as virtual Nix store
- [rs9p](https://docs.rs/rs9p) - Rust 9P2000.L library
- [rust-vfs](https://github.com/manuel-woelker/rust-vfs) - Virtual filesystem abstractions

## Appendix: Platform-Specific Notes

### Linux

```bash
# Check FUSE support
ls -la /dev/fuse
modinfo fuse

# Mount options
mount -t fuse.turnkey-fuse /path/to/mountpoint -o allow_other
```

### macOS (FUSE-T)

```bash
# Install FUSE-T
brew install fuse-t

# Mount uses NFS under the hood
# Standard mount/umount commands work
```

### FreeBSD

```bash
# Load FUSE kernel module
kldload fusefs

# Similar to Linux from userspace perspective
```
