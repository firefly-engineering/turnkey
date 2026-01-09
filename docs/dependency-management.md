# Dependency Management in Turnkey

This document describes the core principles for how external dependencies are managed in Turnkey. These principles apply uniformly across all supported languages.

## Core Principles

### 1. No In-Repo Vendoring

Dependencies are **never** vendored into the repository. All dependency sources live in the Nix store.

- No `vendor/` directories committed to git
- No `node_modules/`, `__pycache__/`, or similar cached dependencies
- The repository contains only source code and dependency declarations

### 2. Language-Native Declarations Are the Source of Truth

Each language has its own dependency declaration format. These are the **sole source of truth** for what dependencies are needed:

| Language | Declaration Files |
|----------|-------------------|
| Go       | `go.mod`, `go.sum` |
| Rust     | `Cargo.toml`, `Cargo.lock` |
| Python   | `pyproject.toml`, `requirements.txt` |
| Node.js  | `package.json`, `package-lock.json` |

These files define the dependency graph at the **module level** (not package/subpackage level).

### 3. Per-Module Fetching with Deterministic Hashes

Dependencies are fetched individually by Nix, each with its own content hash:

```
go.mod/go.sum  →  godeps-gen --prefetch  →  go-deps.toml  →  Nix fetches each module
```

The intermediate TOML file (`go-deps.toml`, `rust-deps.toml`, etc.) contains:
- Module/crate/package identifiers
- Versions (from lock file)
- Nix-compatible SRI hashes (from prefetching)

### 4. Dependency Cells for Buck2

Dependencies are assembled into Buck2 cells by Nix:

```
go-deps.toml  →  go-deps-cell.nix  →  .turnkey/godeps/  (symlink to Nix store)
```

The cell contains:
- Fetched source files for each dependency
- Generated BUCK files for Buck2 to consume
- Any scaffolding needed by build tools (e.g., `modules.txt` for Go)

### 5. Build Tools Read from Cells, Not Network

Buck2 rules reference dependencies from the cell:

```python
go_library(
    name = "mylib",
    deps = ["godeps//vendor/github.com/google/uuid:uuid"],
)
```

No network access during builds. All sources are pre-fetched into Nix store.

## Data Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Source of Truth                                │
│                                                                          │
│    go.mod / go.sum          Cargo.toml / Cargo.lock       pyproject.toml │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Hash Generation Tools                            │
│                                                                          │
│    godeps-gen --prefetch       (rust equivalent)        (python equiv)   │
│                                                                          │
│    Reads dependency declaration, fetches each module via nix-prefetch-*  │
│    Outputs TOML with per-module SRI hashes                               │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Dependency TOML Files                            │
│                                                                          │
│    go-deps.toml                rust-deps.toml           python-deps.toml │
│                                                                          │
│    [deps."github.com/foo/bar"]                                           │
│    version = "v1.2.3"                                                    │
│    hash = "sha256-..."                                                   │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Nix Cell Builders                                │
│                                                                          │
│    go-deps-cell.nix           rust-deps-cell.nix       python-deps-cell  │
│                                                                          │
│    - Reads TOML, fetches each module via fetchFromGitHub/fetchurl        │
│    - Assembles into directory structure                                  │
│    - Generates BUCK files (via gobuckify or equivalent)                  │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Buck2 Cells (in .turnkey/)                       │
│                                                                          │
│    .turnkey/godeps/           .turnkey/rustdeps/       .turnkey/pydeps/  │
│    (symlinks to Nix store)                                               │
│                                                                          │
│    Contains: source files, BUCK files, cell config                       │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              Buck2 Build                                 │
│                                                                          │
│    buck2 build //my/package:target                                       │
│                                                                          │
│    References deps as: godeps//vendor/github.com/foo/bar:bar             │
│    All sources already in Nix store - no network access needed           │
└─────────────────────────────────────────────────────────────────────────┘
```

## Anti-Patterns to Avoid

### Never Use vendorHash

Nix's `buildGoModule` has a `vendorHash` that hashes the output of `go mod vendor`. This is problematic:

1. **Implementation-dependent**: The hash changes based on which packages are actually imported, not just what's declared
2. **Opaque**: You can't know the hash without running the build and letting it fail
3. **Unstable**: Adding a new import from an existing module can change the hash

Instead, use per-module fetching where each module has its own deterministic hash based on its source content.

### Never Vendor in Repository

Even temporarily. If you see a `vendor/` directory in the repo, something is wrong.

### Never Compute Hashes from Vendored Output

The hash should come from the source (e.g., GitHub tarball), not from transformed/vendored output.

## Regenerating Dependencies

When dependencies change:

```bash
# 1. Update the language-native declaration
go get github.com/new/dependency@v1.0.0

# 2. Regenerate the TOML with hashes
godeps-gen --prefetch > go-deps.toml

# 3. Rebuild the cell (happens automatically via Nix)
# The .turnkey/godeps symlink will point to new store path
```

## Bootstrap Considerations

Tools like `godeps-gen` themselves have dependencies. For these bootstrap tools:

1. Dependencies are hardcoded in the Nix package definition
2. Each dependency is fetched individually (same per-module pattern)
3. A vendor directory is assembled ephemerally during the Nix build
4. The tool is built with `go build -mod=vendor`

This vendor directory only exists during the Nix build - it's never committed to the repo.
