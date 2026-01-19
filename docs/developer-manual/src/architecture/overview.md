# Architecture Overview

Turnkey uses a layered architecture to transform simple TOML declarations into working development environments.

## Data Flow

```
toolchain.toml
    ↓
Flake-parts module (perSystem options)
    ↓
Devenv module (shell configuration)
    ↓
Registry (name → package resolution)
    ↓
Buck2 cell generation (toolchains, deps)
    ↓
Working development shell
```

## Layer Responsibilities

### toolchain.toml

Simple declaration of needed tools:

```toml
[toolchains]
go = {}
rust = {}
```

### Flake-Parts Module

- Exposes `turnkey.toolchains` options at perSystem level
- Builds dependency cells from deps files
- Creates devenv shell configurations
- Located at `nix/flake-parts/turnkey/default.nix`

### Devenv Module

- Receives registry and declaration file
- Resolves toolchain names to packages
- Adds packages to shell PATH
- Generates Buck2 cells on shell entry
- Located at `nix/devenv/turnkey/default.nix`

### Registry

- Maps toolchain names to Nix packages
- Simple attribute set: `{ go = pkgs.go; rust = pkgs.rustc; }`
- Extensible by users
- Located at `nix/registry/default.nix`

### Buck2 Cells

- Generated at shell entry time
- Toolchains cell with language-specific rules
- Dependency cells (godeps, rustdeps, pydeps)
- Symlinked into `.turnkey/`

## Key Design Decisions

1. **Nix for package resolution** - Leverages nixpkgs for reproducibility
2. **Devenv for shell management** - Proven shell environment tooling
3. **Generated Buck2 cells** - Dynamic, not committed to repo
4. **Prelude from Nix** - Pinned, patched, extended prelude
