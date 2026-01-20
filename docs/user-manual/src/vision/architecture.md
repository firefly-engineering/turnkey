# Architecture Overview

Turnkey combines three powerful technologies - **Nix**, an **incremental build system**, and **devenv** - into a cohesive developer experience. This chapter explains how these pieces fit together.

> **Current Implementation:** Turnkey uses Buck2 as its incremental build system. The architecture is designed to potentially support other systems like Bazel in the future.

## The Three Pillars

```
┌─────────────────────────────────────────────────────────────┐
│                     Developer Experience                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ go build    │  │ tk build    │  │ IDE / LSP           │  │
│  │ cargo test  │  │ tk test     │  │ Autocomplete        │  │
│  │ pytest      │  │ tk run      │  │ Go to definition    │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         Turnkey                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ tw wrappers │  │ tk CLI      │  │ Dep generators      │  │
│  │ Auto-sync   │  │ Build wrap  │  │ godeps-gen, etc.    │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Core Technologies                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │    Nix      │  │Build System │  │      devenv         │  │
│  │ Hermetic    │  │ Incremental │  │ Shell environment   │  │
│  │ packages    │  │ builds      │  │ configuration       │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Nix: Hermetic Package Management

Nix provides **reproducible package management**. Every tool, compiler, and library has a precise version controlled by the `flake.nix` and `flake.lock` files.

**What Nix provides:**
- Exact versions of go, cargo, python, node, etc.
- System libraries and compilers
- Build tools (buck2 itself)
- Dependency fetching with verified hashes

**Key benefit:** When you enter the development shell, you have the *exact same tools* as every other developer and CI system.

### Incremental Build System (Buck2)

The build system provides **fast, incremental, and correct builds**. It tracks dependencies at a fine-grained level and only rebuilds what's necessary.

**What the build system provides:**
- Dependency tracking between files and targets
- Parallel execution of independent tasks
- Remote caching (share builds across machines)
- Remote execution (distribute builds to a cluster)

**Key benefit:** After initial setup, builds are dramatically faster because unchanged code isn't rebuilt.

### devenv: Developer Shell Configuration

devenv provides a **declarative shell environment** configured through Nix. It handles:

- Environment variable setup
- Shell hooks and initialization
- Service management (databases, etc.)
- Integration with direnv for automatic activation

**Key benefit:** Entering a project directory automatically sets up the complete development environment.

## The Flow of Data

### From Lock Files to Build System Cells

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  go.mod/go.sum   │────▶│   godeps-gen     │────▶│  go-deps.toml    │
│  (native lock)   │     │  (generator)     │     │  (intermediate)  │
└──────────────────┘     └──────────────────┘     └──────────────────┘
                                                           │
                                                           ▼
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  .turnkey/godeps │◀────│      Nix         │◀────│  go-deps.toml    │
│  (Buck2 cell)    │     │  (fetcher)       │     │  (with hashes)   │
└──────────────────┘     └──────────────────┘     └──────────────────┘
```

1. **Native lock files** (go.sum, Cargo.lock, pnpm-lock.yaml) define exact dependency versions
2. **Dependency generators** (godeps-gen, rustdeps-gen, etc.) parse lock files and output intermediate TOML
3. **Nix** fetches dependencies with verified hashes and creates build-system-compatible cells
4. **The build system** treats these cells as source code, enabling full incrementality

### The tw Wrapper Flow

```
Developer runs: tw go get github.com/foo/bar
                        │
                        ▼
              ┌─────────────────┐
              │  Snapshot state │  (hash go.mod, go.sum)
              └─────────────────┘
                        │
                        ▼
              ┌─────────────────┐
              │  Run go get     │  (native command)
              └─────────────────┘
                        │
                        ▼
              ┌─────────────────┐
              │ Check for diff  │  (did lock files change?)
              └─────────────────┘
                        │
              ┌─────────┴─────────┐
              ▼                   ▼
        [No change]         [Files changed]
              │                   │
              │                   ▼
              │         ┌─────────────────┐
              │         │ Run godeps-gen  │
              │         └─────────────────┘
              │                   │
              └───────────────────┘
                        │
                        ▼
                    [Done]
```

The `tw` wrapper ensures the build system's view of dependencies stays synchronized with native tools, without requiring developer intervention.

## Directory Structure

A typical Turnkey-enabled project looks like:

```
project/
├── .buckconfig              → Symlink to generated config (Buck2)
├── .buckroot                → Marks project root for build system
├── .envrc                   → Activates devenv via direnv
├── flake.nix                → Nix flake configuration
├── flake.lock               → Locked Nix dependencies
├── toolchain.toml           → Turnkey toolchain declaration
│
├── src/                     → Your source code
│   ├── cmd/
│   ├── pkg/
│   └── rules.star           → Build rules
│
├── go.mod                   → Go module definition
├── go.sum                   → Go dependency lock
├── go-deps.toml            → Generated dependency manifest
│
├── Cargo.toml              → Rust workspace definition
├── Cargo.lock              → Rust dependency lock
├── rust-deps.toml          → Generated dependency manifest
│
└── .turnkey/               → Generated artifacts (gitignored)
    ├── prelude/            → Build system prelude cell
    ├── toolchains/         → Toolchain definitions
    ├── godeps/             → Go dependency cell
    ├── rustdeps/           → Rust dependency cell
    └── sync.toml           → Sync configuration
```

### What's Committed to Git

- Source code (`src/`)
- Native project files (`go.mod`, `Cargo.toml`, etc.)
- Lock files (`go.sum`, `Cargo.lock`, etc.)
- Turnkey configuration (`toolchain.toml`)
- Nix configuration (`flake.nix`, `flake.lock`)
- Generated dependency manifests (`go-deps.toml`, etc.)

### What's Generated (Not Committed)

- `.turnkey/` directory (regenerated from lock files)
- `.buckconfig` (symlinked to Nix store, Buck2-specific)
- Build outputs (`buck-out/`)

## The Toolchain Flow

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ toolchain.toml  │────▶│    Registry     │────▶│  Nix packages   │
│                 │     │   (mapping)     │     │                 │
│ [toolchains]    │     │ go = pkgs.go    │     │ /nix/store/...  │
│ go = {}         │     │ rust = pkgs...  │     │                 │
│ rust = {}       │     │                 │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                                         │
                                                         ▼
                        ┌─────────────────┐     ┌─────────────────┐
                        │  Buck2 targets  │◀────│    mappings     │
                        │                 │     │                 │
                        │ toolchains//:go │     │ Generate rules  │
                        │ toolchains//... │     │ from registry   │
                        └─────────────────┘     └─────────────────┘
```

1. `toolchain.toml` declares what toolchains you need
2. The **registry** maps toolchain names to Nix packages
3. **mappings** translate these into build system toolchain targets
4. The build system uses the toolchain targets for builds

## Summary

Turnkey's architecture achieves its goals through careful layering:

| Layer | Responsibility | Technology |
|-------|---------------|------------|
| Top | Developer UX | Native tools, tw/tk wrappers |
| Middle | Orchestration | Turnkey, dependency generators |
| Bottom | Execution | Nix (packages), build system (builds), devenv (shell) |

Each layer can be understood independently, and the boundaries are clean enough that you can use partial features without understanding the whole system.

For detailed information about specific components, see the reference documentation.
