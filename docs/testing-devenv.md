# Testing in the Devenv Environment

This document describes the recommended pattern for running commands that need the full devenv environment, particularly useful for automated testing and CI/CD.

## The Pattern: `direnv exec . <command>`

The recommended way to run commands within the devenv environment is:

```bash
direnv exec . <command>
```

### Examples

```bash
# Build a Buck2 target
direnv exec . tk build //src/rust/starlark-parse:starlark-parse

# Run tests
direnv exec . tk test //src/rust/starlark-parse:starlark-parse-test

# Run a binary
direnv exec . tk run //src/cmd/check-source-coverage-rs:check-source-coverage-rs

# Use native tools
direnv exec . cargo check
direnv exec . go test ./...

# Run shell commands
direnv exec . bash -c 'echo $PATH | tr ":" "\n" | head -5'
```

## Why This Pattern Works

### 1. Consistent Environment

`direnv exec .` loads the full devenv environment before running the command:
- Sets `PATH` to include all Nix-provided tools
- Sets environment variables like `TURNKEY_DIRENV_LIB`
- Provides access to `tk`, `tw`, and all registered toolchains

### 2. Automatic Cell Symlink Management

The turnkey direnv library automatically:
- Creates `.turnkey/` directory if missing
- Creates/updates cell symlinks (rustdeps, godeps, pydeps, etc.)
- Regenerates cells when dependency files change

This means you don't need to manually run `direnv reload` or rebuild cells.

### 3. Works for All Tool Types

The same pattern works for:
- **Buck2 via tk**: `direnv exec . tk build //...`
- **Native Rust**: `direnv exec . cargo build`
- **Native Go**: `direnv exec . go test ./...`
- **Native Python**: `direnv exec . python -m pytest`
- **Any shell command**: `direnv exec . bash -c '...'`

### 4. Idempotent

Running `direnv exec . <command>` multiple times is safe:
- Cell symlinks are only updated if needed
- No side effects from repeated invocations
- Fast when environment is already up-to-date

## Comparison with Alternatives

| Approach | Pros | Cons |
|----------|------|------|
| `direnv exec . <cmd>` | Consistent, automatic cell setup, works everywhere | Slight overhead on first run |
| `nix develop -c <cmd>` | Pure Nix, no direnv needed | Slower, doesn't run turnkey hooks |
| Manual `direnv reload` | Fine for interactive use | Easy to forget, cells may be stale |
| Running directly in shell | Fast | Only works if env already loaded |

## Best Practices

### For CI/CD Scripts

```bash
#!/bin/bash
set -euo pipefail

# Ensure direnv is available
command -v direnv >/dev/null || { echo "direnv not found"; exit 1; }

# Allow direnv for this directory (needed in CI)
direnv allow .

# Run your commands
direnv exec . tk build //...
direnv exec . tk test //...
```

### For Local Development

When working interactively, you can rely on direnv's automatic loading. But for scripted operations or when debugging environment issues, use `direnv exec .`:

```bash
# Debug: check what environment is loaded
direnv exec . env | grep TURNKEY

# Debug: check cell symlinks
direnv exec . ls -la .turnkey/

# Force rebuild of a specific cell
rm .turnkey/rustdeps
direnv exec . ls -la .turnkey/rustdeps  # Will recreate
```

### For Testing After Nix Changes

When you modify Nix files that affect the devenv (e.g., fixups, cell builders), use `nix develop --impure` first to rebuild, then `direnv exec .` for subsequent commands:

```bash
# After modifying nix/lib/deps-cell/fixups/rust/tree-sitter.nix
rm -rf .turnkey/rustdeps
nix develop --impure -c bash -c 'ls -la .turnkey/rustdeps'

# Subsequent commands can use direnv exec
direnv exec . tk build //...
```

## Caveats

### 1. First Run Overhead

The first `direnv exec .` after changes may take a few seconds to:
- Evaluate Nix expressions
- Build any changed derivations
- Update cell symlinks

### 2. Nix Store Caching

`direnv exec .` uses the cached Nix evaluation. If you modify Nix files that aren't watched by direnv, changes won't be picked up until you run `nix develop --impure` or touch a watched file.

Watched files include:
- `flake.nix`, `flake.lock`
- `*.toml` dependency files (go-deps.toml, rust-deps.toml, etc.)
- Key Nix files in `nix/` directory

### 3. direnv Must Be Allowed

In CI or fresh checkouts, you need to run `direnv allow .` first:

```bash
direnv allow .
direnv exec . tk build //...
```

## Summary

Use `direnv exec . <command>` as the standard pattern for:
- Running builds and tests in scripts
- CI/CD pipelines
- Debugging environment issues
- Any situation where you need guaranteed environment consistency

This pattern ensures your commands run with the correct environment, tools, and cell configurations regardless of the current shell state.
