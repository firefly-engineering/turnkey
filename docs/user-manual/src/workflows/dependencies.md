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
go-deps.toml  →  one Nix package per module  →  .turnkey/godeps/  (tk materialize)
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
│   nix/lib/deps-cell/adapters/{go,rust,python,javascript,solidity}.nix   │
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

`tk sync` regenerates it when `go.mod` or `go.sum` changes. To run the
generator yourself:

```bash
godeps-gen -o go-deps.toml
```

Options:

- `--no-prefetch`: Skip fetching the Nix hashes of the modules'
  proxy.golang.org zips, the source the godeps cell fetches from (the hashes
  are then invalid)
- `--no-cache`: Always fetch from the network, bypassing the prefetch cache
- `--indirect`: Include indirect (transitive) dependencies (default: true)
- `-o, --output`: Output file (default: stdout)

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

### Multiple Modules and Local Replaces

Several Go modules in one repo are declared as a `go.work` workspace at the
project root. Go resolves them together into one `go-deps.toml` and one
`godeps` cell, and rules sync maps imports of a member to its targets in the
repo. A local-path `replace` directive must point at a workspace member.

See the [Go language guide](../languages/go.md#multiple-modules-gowork) for
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
- `--no-cache`: Always fetch from the network, bypassing the prefetch cache
- `-o, --output`: Output file (default: stdout)

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
--no-cache             Always fetch from the network, bypassing the prefetch cache
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

## The Go, Rust, Python, Solidity and JavaScript Cells Are Real Directories

`.turnkey/godeps`, `.turnkey/rustdeps`, `.turnkey/pydeps`, `.turnkey/soldeps` and `.turnkey/jsdeps` are directories that `tk materialize` keeps in line with the cell index the shell builds ([ADR 0004](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0004-deps-cells-are-write-once-directories.md)).

- **`_store/<store path name>`**: one symlink per crate, Go module, Python distribution or Solidity package, to its own store path. It is only ever created or deleted, never pointed elsewhere.
- **`vendor/<crate>@<version>/rules.star`** and **`vendor/<crate>/rules.star`**: `alias()` targets that forward to a store link's crate. Labels such as `rustdeps//vendor/anyhow:anyhow` are unchanged.
- **`vendor/<name>/rules.star`**: one per Python distribution, forwarding to its store link ([ADR 0010](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0010-pydeps-stores-one-distribution-per-store-link.md)). Labels such as `pydeps//vendor/six:six` are unchanged.
- **`vendor/<import path>/rules.star`**: one per Go package, forwarding to its package in its module's store link ([ADR 0008](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0008-godeps-stores-one-module-per-store-link.md)). Labels such as `godeps//vendor/golang.org/x/sys/unix:unix` are unchanged. Where a dependency imports a `go.work` member's package, its alias forwards to the member's target in the repo instead.
- **`vendor/<name>@<version>/rules.star`**: one per npm package, forwarding to its files in its store link. The cell's root `rules.star`, which the index carries, declares the package graph over them: an instance per pnpm snapshot, and `jsdeps//:<npm name>` per direct dependency ([ADR 0012](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md)).
- **`vendor/<name>/rules.star`**: one per Solidity package, forwarding to its store link, beside links to the package's files, which native `forge` reads through the root `remappings.txt` ([ADR 0011](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0011-soldeps-stores-one-package-per-store-link.md)). Labels are unchanged: `soldeps//:<package>` and `soldeps//:bundle` come from the cell's root `rules.star`, which the index carries, and `soldeps//vendor/<name>:<package>` from the alias package.

Only what a change touches is rewritten, so a dependency bump recompiles the bumped crate's, module's or distribution's dependents and re-runs only their tests. An npm package bump re-runs the instances that depend on it and their consumers. A Solidity package bump re-runs every Solidity action, since each one stages the whole bundle, and nothing in another language. Everything else stays cached, with no daemon restart, and plain `buck2` reads the new version. Don't edit the directory: the shell rewrites it on every load.

- **Switching over:** the first shell load after upgrading turnkey replaces the old `.turnkey/<cell>` symlink with the directory. `tk` restarts the buck2 daemon once, and the next build is a full one.
- **Going back to an older turnkey:** run `rm -rf .turnkey/<cell>`, then reload the shell.
- **`tk: warning: the rustdeps cell was built from another rust-deps.toml`** (or `godeps` and `go-deps.toml`, `pydeps` and `python-deps.toml`, `soldeps` and `solidity-deps.toml`, `jsdeps` and `js-deps.toml`): the deps file changed since the shell last loaded. Run `direnv reload`, or re-enter the shell, to rebuild the cell.

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

1. Regenerate the deps file with fresh hashes:
   ```bash
   godeps-gen --no-cache -o go-deps.toml
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
