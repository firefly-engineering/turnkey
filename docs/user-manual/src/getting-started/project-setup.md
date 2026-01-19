# Project Setup

This guide covers detailed project configuration options.

## Directory Structure

A typical Turnkey project has this structure:

```
my-project/
├── .buckconfig           # Buck2 configuration (generated)
├── .envrc                # direnv configuration
├── .turnkey/             # Generated cells (gitignored)
├── flake.nix             # Nix flake configuration
├── flake.lock            # Locked dependencies
├── toolchain.toml        # Toolchain declarations
├── go-deps.toml          # Go dependencies (if using Go)
├── rust-deps.toml        # Rust dependencies (if using Rust)
└── rules.star            # Root build file
```

## The .turnkey Directory

Turnkey generates several cells in `.turnkey/`:

- `prelude/` - Buck2 prelude (symlinked from Nix store)
- `toolchains/` - Language toolchains
- `godeps/` - Go dependency cell (if configured)
- `rustdeps/` - Rust dependency cell (if configured)

This directory should be gitignored as it's generated on shell entry.

## direnv Integration

For automatic environment activation, create `.envrc`:

```bash
use flake
```

Then allow it:

```bash
direnv allow
```

## Buck2 Configuration

The `.buckconfig` is generated automatically. For project-specific settings, create `.buckconfig.local`.
