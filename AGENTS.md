# Agent Instructions

## Work Management

This project tracks work in GitHub Issues, with priority and status in the turnkey org project. See `docs/agents/issue-tracker.md` for the commands.

Committing, closing issues, and pushing are part of completing a task — not separate actions requiring additional permission.

## Adding Go Dependencies

turnkey's own tools are Rust, built by Nix from the Cargo workspace, so no
Nix package builds Go code and there is no `vendorHash` to update. The root
`go.mod` serves the Go examples (`src/examples/go-*`), which Buck2 builds
against the godeps cell.

**CRITICAL: This project does NOT use `go mod vendor`. All vendoring happens through Nix cells.**

```bash
go get github.com/example/package
go mod tidy  # ALWAYS run this to sync direct/indirect deps
tk sync      # regenerates go-deps.toml
```

## Quality Gates

**Before pushing any code changes**, you MUST run these checks in a fresh Nix devenv environment:

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
- Before pushing ANY code changes

**If builds fail after Nix changes:**
1. Try `direnv reload` to refresh the environment
2. If still failing, use `nix develop` to force fresh evaluation
3. Run `tk clean` to clear Buck2's file cache
4. Rebuild and retest

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - See "Quality Gates" section below
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

