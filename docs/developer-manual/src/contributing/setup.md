# Development Setup

Set up your environment to contribute to Turnkey.

## Prerequisites

- Nix with flakes enabled
- direnv (recommended)
- Git

## Clone and Enter Shell

```bash
git clone https://github.com/firefly-engineering/turnkey.git
cd turnkey
direnv allow  # or: nix develop
```

## Repository Layout

```
turnkey/
├── flake.nix           # Main flake (self-usage example)
├── toolchain.toml      # Example toolchain config
├── nix/
│   ├── flake-parts/    # Flake-parts module
│   ├── devenv/         # Devenv module
│   ├── registry/       # Default registry
│   ├── buck2/          # Buck2 integration
│   └── packages/       # Tool packages
├── cmd/                # CLI tools (Go)
├── docs/               # Documentation
└── examples/           # Example projects
```

## Making Changes

### Nix Code

1. Edit files in `nix/`
2. Stage changes: `git add nix/`
3. Re-enter shell to test: `exit && nix develop`

### Go Code

1. Edit files in `cmd/`
2. Build: `tk build //cmd/...`
3. Run: `tk run //cmd/mytool:mytool`

### Documentation

1. Edit files in `docs/`
2. Build book: `tk build //docs/user-manual:user-manual`
3. Preview: `tk run //docs/user-manual:user-manual`

## Running Tests

```bash
# All tests
tk test //...

# Specific package
tk test //go/pkg/syncer:syncer_test
```

## Pre-commit Hooks

Turnkey uses pre-commit hooks for:

- Nix flake check
- Monorepo dependency check
- Rust edition check

Hooks run automatically on commit.
