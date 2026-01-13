# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

## Quality Gates

**Before pushing any code changes**, you MUST run these checks in a fresh Nix devenv environment:

```bash
# Ensure fresh environment (especially after Nix changes)
direnv reload
# OR if direnv caching is stale:
nix develop

# Build all targets
buck2 build //...

# Run all tests
buck2 test //...
```

**When to run quality gates:**
- After modifying any `.nix` files (especially `rust-deps-cell.nix`, `go-deps-cell.nix`)
- After modifying any Rust, Go, or Python code
- After changing dependency declarations (`rust-deps.toml`, `go-deps.toml`, etc.)
- Before pushing ANY code changes

**If builds fail after Nix changes:**
1. Try `direnv reload` to refresh the environment
2. If still failing, use `nix develop` to force fresh evaluation
3. Run `buck2 clean` to clear Buck2's file cache
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
   bd sync
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

