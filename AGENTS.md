# Agent Instructions

## Work Management

This project tracks work in GitHub Issues, with priority and status in the turnkey org project. See `docs/agents/issue-tracker.md` for the commands.

Committing, closing issues, and pushing are part of completing a task — not separate actions requiring additional permission.

## Adding Go Dependencies

turnkey's own tools are Rust, built by Nix from the Cargo workspace, so no
Nix package builds Go code and there is no `vendorHash` to update. The root
`go.mod` serves the Go examples (`src/examples/go-*`), which Buck2 builds
against the godeps cell.

This project does not use `go mod vendor`: vendoring happens through Nix cells.

```bash
go get github.com/example/package
go mod tidy  # sync direct/indirect deps
tk sync      # regenerates go-deps.toml
```

## Quality Gates

Before pushing code changes, run these checks in a fresh Nix devenv environment:

```bash
# Ensure fresh environment (especially after Nix changes)
direnv reload
# OR if direnv caching is stale:
nix develop

# Build all targets
tk build //...

# Run all tests
tk test //...
```

**When to run quality gates:**
- After modifying any `.nix` files (especially `nix/buck2/languages.nix` and `nix/lib/deps-cell/`)
- After modifying any Rust, Go, or Python code
- After changing dependency declarations (`rust-deps.toml`, `go-deps.toml`, etc.)

**If builds fail after Nix changes:**
1. Try `direnv reload` to refresh the environment
2. If still failing, use `nix develop` to force fresh evaluation
3. Run `tk clean` to clear Buck2's file cache
4. Rebuild and retest

## Landing the Plane (Session Completion)

A task is finished when its change is on the remote, because unpushed work is stranded locally:

1. File issues for remaining follow-up work.
2. Run the quality gates above if code changed.
3. Close finished issues; update in-progress ones.
4. Push, and confirm the remote has the change. If the push fails, resolve it and push again.
5. Leave a short handoff note for the next session.
