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
│    godeps-gen --prefetch        rustdeps-gen             pydeps-gen      │
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
│    go-deps-cell.nix         rust-deps-cell.nix      python-deps-cell.nix │
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

When dependencies change, use `tk sync` to regenerate all dependency files:

```bash
# Update language-native declaration, then sync
go get github.com/new/dependency@v1.0.0
tk sync
```

Or regenerate specific languages manually:

```bash
# Go
godeps-gen --prefetch -o go-deps.toml

# Rust
rustdeps-gen --cargo-lock Cargo.lock -o rust-deps.toml

# Python (recommended: use lock file for reproducibility)
uv lock && uv export --format pylock.toml -o pylock.toml
pydeps-gen --lock pylock.toml -o python-deps.toml
```

## Dependency Generator Tools

### godeps-gen (Go)

Generates `go-deps.toml` from `go.mod` and `go.sum`.

```bash
godeps-gen [--prefetch] [-o go-deps.toml]
```

Options:
- `--prefetch`: Fetch Nix hashes using nix-prefetch-github (required for valid hashes)
- `--indirect`: Include indirect (transitive) dependencies (default: true)
- `-o`: Output file (default: stdout)

### rustdeps-gen (Rust)

Generates `rust-deps.toml` from `Cargo.lock`.

```bash
rustdeps-gen --cargo-lock Cargo.lock [-o rust-deps.toml]
```

Options:
- `--cargo-lock`: Path to Cargo.lock file (default: Cargo.lock)
- `--no-prefetch`: Skip prefetching (produces incorrect hashes)
- `-o`: Output file (default: stdout)

### pydeps-gen (Python)

Generates `python-deps.toml` from Python dependency files. Supports three input formats:

**Input Formats:**

| Format | Flag | Reproducibility | Notes |
|--------|------|-----------------|-------|
| pylock.toml (PEP 751) | `--lock` | ✅ Best | Exact versions and URLs |
| pyproject.toml | `--pyproject` | ⚠️ Varies | Uses latest matching versions |
| requirements.txt | `--requirements` | ⚠️ Varies | Pin versions with `==` for reproducibility |

**Recommended Workflow (using uv):**

```bash
# 1. Generate lock file from pyproject.toml
uv lock

# 2. Export to PEP 751 format
uv export --format pylock.toml -o pylock.toml

# 3. Generate python-deps.toml with Nix hashes
pydeps-gen --lock pylock.toml -o python-deps.toml
```

**CLI Options:**

```
--lock <PATH>          Path to pylock.toml (PEP 751 lock file) - RECOMMENDED
--pyproject <PATH>     Path to pyproject.toml
--requirements <PATH>  Path to requirements.txt
-o, --output <PATH>    Output file (default: stdout)
--no-prefetch          Skip prefetching (produces placeholder hashes)
--include-dev          Include dev dependencies from optional-dependencies.dev
```

**Output Format:**

```toml
# python-deps.toml
[deps.requests]
version = "2.31.0"
hash = "sha256-..."
url = "https://files.pythonhosted.org/..."

[deps.six]
version = "1.16.0"
hash = "sha256-..."
url = "https://files.pythonhosted.org/..."
```

**Examples:**

```bash
# From PEP 751 lock file (best for reproducibility)
pydeps-gen --lock pylock.toml -o python-deps.toml

# From pyproject.toml (resolves to latest matching versions)
pydeps-gen --pyproject pyproject.toml -o python-deps.toml

# From requirements.txt
pydeps-gen --requirements requirements.txt -o python-deps.toml

# Include dev dependencies
pydeps-gen --lock pylock.toml --include-dev -o python-deps.toml

# Quick check without fetching (placeholder hashes)
pydeps-gen --lock pylock.toml --no-prefetch
```

## Building Tools vs Dependency Cells

There's an important distinction between:

1. **Building tools** (like `godeps-gen`) - Use standard `buildGoModule` with `vendorHash`
2. **Dependency cells** (like `go-deps-cell.nix`) - Use per-module fetching

For tools, `buildGoModule` is the standard Nix pattern. The vendoring happens ephemerally in the Nix build (not in-repo), and the `vendorHash` is deterministic for a given go.mod/go.sum. To get the hash, let the build fail once and copy the expected value.

For dependency cells consumed by Buck2, we use per-module fetching because:
- Buck2 needs to reference individual modules as targets
- Each module's hash should be independently verifiable
- The cell structure must match Buck2's expectations
