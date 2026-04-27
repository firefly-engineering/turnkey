# Managing Dependencies

This guide covers how external dependencies are managed in Turnkey projects.

## Core Principles

### 1. No In-Repo Vendoring

Dependencies are **never** vendored into the repository. All dependency sources
live in the Nix store.

- No `vendor/` directories committed to git
- No `node_modules/`, `__pycache__/`, or similar cached dependencies
- The repository contains only source code and dependency declarations

### 2. Language-Native Declarations Are the Source of Truth

Each language has its own dependency declaration format. These are the **sole
source of truth** for what dependencies are needed:

| Language | Declaration Files           |
| -------- | --------------------------- |
| Go       | `go.mod`, `go.sum`          |
| Rust     | `Cargo.toml`, `Cargo.lock`  |
| Python   | `pyproject.toml`, `uv.lock` |

These files define the dependency graph at the **module level** (not
package/subpackage level).

### 3. Per-Module Fetching with Deterministic Hashes

Dependencies are fetched individually by Nix, each with its own content hash:

```
go.mod/go.sum  →  godeps-gen  →  go-deps.toml  →  Nix fetches each module
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
- Generated rules.star files for Buck2 to consume
- Any scaffolding needed by build tools (e.g., `modules.txt` for Go)

## Data Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Source of Truth                                │
│                                                                         │
│   go.mod / go.sum          Cargo.toml / Cargo.lock       pyproject.toml │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Hash Generation Tools                            │
│                                                                         │
│   godeps-gen                      rustdeps-gen             pydeps-gen   │
│                                                                         │
│   Reads dependency declaration, fetches each module via nix-prefetch-*  │
│   Outputs TOML with per-module SRI hashes                               │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Dependency TOML Files                            │
│                                                                         │
│   go-deps.toml                rust-deps.toml           python-deps.toml │
│                                                                         │
│   [deps."github.com/foo/bar"]                                           │
│   version = "v1.2.3"                                                    │
│   hash = "sha256-..."                                                   │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Nix Cell Builders                                │
│                                                                         │
│   go-deps-cell.nix         rust-deps-cell.nix      python-deps-cell.nix │
│                                                                         │
│   - Reads TOML, fetches each module via fetchFromGitHub/fetchurl        │
│   - Assembles into directory structure                                  │
│   - Generates rules.star files                                          │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Buck2 Cells (in .turnkey/)                       │
│                                                                         │
│   .turnkey/godeps/           .turnkey/rustdeps/       .turnkey/pydeps/  │
│   (symlinks to Nix store)                                               │
│                                                                         │
│   Contains: source files, rules.star files, cell config                 │
└─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                             Buck2 Build                                 │
│                                                                         │
│   buck2 build //my/package:target                                       │
│                                                                         │
│   References deps as: godeps//vendor/github.com/foo/bar:bar             │
│   All sources already in Nix store - no network access needed           │
└─────────────────────────────────────────────────────────────────────────┘
```

## Auto-Sync with Wrapped Tools

When using `go`, `cargo`, or `uv` in a Turnkey shell, the tools are
transparently wrapped to trigger automatic dependency synchronization when
dependency files change.

```bash
# These trigger auto-sync when dependency files change
go get github.com/some/package
cargo add serde
uv add requests
```

### How Auto-Sync Works

1. The wrapper captures a hash of dependency files before running the command
2. The actual tool runs (e.g., `go get`)
3. After completion, the wrapper checks if dependency files changed
4. If changed, `tk sync` is triggered automatically

### Verbose Mode

Use verbose mode to see what the wrapper is doing:

```bash
tw -v go get github.com/some/package
```

## Manual Sync

Force a full dependency sync with:

```bash
tk sync
```

Or sync specific languages:

```bash
tk sync --go
tk sync --rust
tk sync --python
```

## Go Dependencies

### Configuration

```nix
turnkey.toolchains.buck2.go = {
  enable = true;
  depsFile = ./go-deps.toml;
};
```

### Generating go-deps.toml

```bash
godeps-gen --prefetch -o go-deps.toml
```

Options:

- `--prefetch`: Fetch Nix hashes using nix-prefetch-github (required for valid
  hashes)
- `--indirect`: Include indirect (transitive) dependencies (default: true)
- `-o`: Output file (default: stdout)

### Using Dependencies in Build Files

```python
go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/spf13/cobra:cobra",
    ],
)
```

### Local Replace Directives

Turnkey supports `replace` directives in `go.mod` that point to local paths.
This is essential for monorepo setups.

**In go.mod:**

```go
replace github.com/company/shared-lib => ../shared-lib
```

**In go-deps.toml** (generated by godeps-gen):

```toml
[replace."github.com/company/shared-lib"]
import_path = "github.com/company/shared-lib"
local_path = "../shared-lib"
```

**Configure the mapping in flake.nix:**

```nix
turnkey.toolchains.buck2.go = {
  enable = true;
  depsFile = ./go-deps.toml;
  localReplaces = {
    "github.com/company/shared-lib" = "//src/shared-lib:shared-lib";
  };
};
```

See the [Go language guide](../languages/go.md#local-replace-directives) for
detailed documentation.

### External Fork Replace Directives

Turnkey also supports `replace` directives that point to external forks:

**In go.mod:**

```go
replace github.com/original/pkg => github.com/myfork/pkg v1.2.3
```

**In go-deps.toml** (generated by godeps-gen):

```toml
[deps."github.com/original/pkg@v1.2.3"]
import_path = "github.com/original/pkg"
fetch_path = "github.com/myfork/pkg"
version = "v1.2.3"
hash = "sha256-..."
```

The cell builder fetches from `fetch_path` but stores under `import_path`, so
your code continues importing from the original path while using the fork's
source.

See the [Go language guide](../languages/go.md#external-fork-replacements) for
detailed documentation.

## Rust Dependencies

### Configuration

```nix
turnkey.toolchains.buck2.rust = {
  enable = true;
  depsFile = ./rust-deps.toml;
};
```

### Generating rust-deps.toml

```bash
rustdeps-gen --cargo-lock Cargo.lock -o rust-deps.toml
```

Options:

- `--cargo-lock`: Path to Cargo.lock file (default: Cargo.lock)
- `--no-prefetch`: Skip prefetching (produces incorrect hashes)
- `-o`: Output file (default: stdout)

### Handling Special Cases

Some Rust crates require additional configuration. See the
[Rust Dependency Handling](../../developer-manual/src/extending/dependency-generators.md)
guide for:

- Build scripts that emit rustc flags
- Generated source files
- Native code compilation

## Python Dependencies

### Configuration

```nix
turnkey.toolchains.buck2.python = {
  enable = true;
  depsFile = ./python-deps.toml;
};
```

### Recommended Workflow (using uv)

```bash
# 1. Generate lock file from pyproject.toml
uv lock

# 2. Export to PEP 751 format
uv export --format pylock.toml -o pylock.toml

# 3. Generate python-deps.toml with Nix hashes
pydeps-gen --lock pylock.toml -o python-deps.toml
```

### Input Formats

| Format                | Flag             | Reproducibility | Notes                                      |
| --------------------- | ---------------- | --------------- | ------------------------------------------ |
| pylock.toml (PEP 751) | `--lock`         | Best            | Exact versions and URLs                    |
| pyproject.toml        | `--pyproject`    | Varies          | Uses latest matching versions              |
| requirements.txt      | `--requirements` | Varies          | Pin versions with `==` for reproducibility |

### CLI Options

```
--lock <PATH>          Path to pylock.toml (PEP 751 lock file) - RECOMMENDED
--pyproject <PATH>     Path to pyproject.toml
--requirements <PATH>  Path to requirements.txt
-o, --output <PATH>    Output file (default: stdout)
--no-prefetch          Skip prefetching (produces placeholder hashes)
--include-dev          Include dev dependencies from optional-dependencies.dev
```

## Anti-Patterns to Avoid

### Never Use vendorHash

Nix's `buildGoModule` has a `vendorHash` that hashes the output of
`go mod vendor`. This is problematic:

1. **Implementation-dependent**: The hash changes based on which packages are
   actually imported
2. **Opaque**: You can't know the hash without running the build and letting it
   fail
3. **Unstable**: Adding a new import from an existing module can change the hash

Instead, use per-module fetching where each module has its own deterministic
hash.

### Never Vendor in Repository

Even temporarily. If you see a `vendor/` directory in the repo, something is
wrong.

### Never Compute Hashes from Vendored Output

The hash should come from the source (e.g., GitHub tarball), not from
transformed/vendored output.

## Troubleshooting

### Dependencies Not Found

If Buck2 can't find a dependency:

1. Check that the deps TOML file is up to date:
   ```bash
   tk sync
   ```

2. Verify the cell symlink exists:
   ```bash
   ls -la .turnkey/godeps
   ```

3. Check the target path format:
   ```bash
   # Correct format
   godeps//vendor/github.com/spf13/cobra:cobra

   # Wrong - missing vendor/ prefix
   godeps//github.com/spf13/cobra:cobra
   ```

### Hash Mismatch Errors

If you get hash mismatch errors when building:

1. Regenerate the deps file with `--prefetch`:
   ```bash
   godeps-gen --prefetch -o go-deps.toml
   ```

2. Re-enter the dev shell:
   ```bash
   exit
   nix develop
   ```

### Stale Dependencies

If dependency changes aren't picked up:

1. Kill the Buck2 daemon:
   ```bash
   buck2 kill
   ```

2. Force a full sync:
   ```bash
   tk sync
   ```
