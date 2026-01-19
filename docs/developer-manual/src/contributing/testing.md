# Testing

## Running Tests

### All Tests

```bash
tk test //...
```

### Specific Packages

```bash
# Go packages
tk test //go/pkg/syncer:syncer_test

# Rust crates
tk test //rust/prefetch-cache:prefetch-cache-test

# Python modules
tk test //python/cargo:test_features
```

## Test Categories

### Unit Tests

Located alongside source code:

```
pkg/
├── syncer.go
└── syncer_test.go
```

### Integration Tests

Located in `e2e/`:

```
e2e/
├── fixtures/
│   ├── greenfield-go/
│   └── multi-language/
└── run_e2e.sh
```

## Nix Testing

### Flake Check

```bash
nix flake check
```

Validates:
- Module definitions
- Package builds
- Template validity

### Derivation Builds

```bash
# Build specific package
nix build .#godeps-gen

# Build prelude
nix build .#turnkey-prelude
```

## Manual Testing

### New Toolchain

1. Add to registry
2. Add to mappings
3. Add to toolchain.toml
4. Enter shell
5. Verify `tk targets toolchains//...`

### Prelude Extension

1. Create extension files
2. Stage: `git add nix/buck2/prelude-extensions/`
3. Rebuild: `nix build .#turnkey-prelude`
4. Verify files in output

### Dependency Cell

1. Generate deps file
2. Build cell: `nix build .#godeps-cell` (example)
3. Verify rules.star content

## CI

Pre-commit hooks run:

1. `nix flake check`
2. `monorepo-dep-check`
3. `rust-edition-check`

All hooks must pass before commit.
