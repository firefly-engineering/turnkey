# Dependency Generators

Tools that generate deps TOML files from native lock files.

## Overview

Each language has a generator that:

1. Reads native lock files (go.sum, Cargo.lock, uv.lock)
2. Extracts dependency information
3. Outputs deps TOML for Nix cell building

## Generator Structure

### Input

Native lock file format (varies by language).

### Output

TOML file with dependencies:

```toml
# go-deps.toml example
[deps]
[deps."github.com/pkg/errors"]
version = "v0.9.1"
hash = "sha256-xyz..."

[deps."golang.org/x/sys"]
version = "v0.15.0"
hash = "sha256-abc..."
```

## Existing Generators

### godeps-gen (Go)

Located at `cmd/godeps-gen/`.

```bash
godeps-gen > go-deps.toml
```

Reads: `go.mod`, `go.sum`

### rustdeps-gen (Rust)

Located at `cmd/rustdeps-gen/`.

```bash
rustdeps-gen > rust-deps.toml
```

Reads: `Cargo.lock`

### pydeps-gen (Python)

Located at `cmd/pydeps-gen/`.

```bash
pydeps-gen > python-deps.toml
```

Reads: `uv.lock` or `requirements.txt`

## Creating a New Generator

1. Create CLI tool in `cmd/newlang-gen/`
2. Parse native lock file
3. Prefetch packages to get Nix hashes
4. Output deps TOML
5. Create cell builder in `nix/buck2/newlang-deps-cell.nix`

### Cell Builder

```nix
{ pkgs, lib, depsFile }:

let
  deps = builtins.fromTOML (builtins.readFile depsFile);
in
pkgs.runCommand "newlang-deps" {} ''
  # Generate rules.star for each dependency
  # ...
''
```

## Testing

```bash
# Generate deps file
newlang-gen > newlang-deps.toml

# Verify it's valid TOML
nix eval --expr 'builtins.fromTOML (builtins.readFile ./newlang-deps.toml)'
```
