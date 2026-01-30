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

## Adding Go Dependencies to Internal Tools

When adding new Go dependencies or changing `go.mod`, you MUST update the Nix package definitions. Nix's `buildGoModule` uses a fixed-output derivation for vendoring, which requires a hash of the dependencies.

**CRITICAL: This project does NOT use `go mod vendor`. All vendoring happens through Nix cells.**

### Step-by-step process:

1. **Update Go dependencies**:
   ```bash
   go get github.com/example/package
   go mod tidy  # ALWAYS run this to sync direct/indirect deps
   ```

2. **Regenerate go-deps.toml** (if godeps-gen is available):
   ```bash
   godeps-gen --prefetch -o go-deps.toml
   ```

3. **Find affected Nix packages** - Check which packages in `nix/packages/` import the changed Go code:
   - `tk.nix` - the tk CLI wrapper
   - `tw.nix` - the tw CLI wrapper
   - Other packages that use Go modules

4. **Update vendorHash using fakeHash trick**:
   ```bash
   # In the affected .nix file, temporarily set:
   vendorHash = lib.fakeHash;

   # Build to get correct hash:
   nix build .#packagename 2>&1
   # Error will show: got: sha256-XXXX...

   # Update with the correct hash:
   vendorHash = "sha256-XXXX...";
   ```

5. **Update fileset if needed** - If you added a new Go package that's imported by a CLI tool, add it to the fileset in the Nix package:
   ```nix
   fileset = fs.unions [
     (root + "/go.mod")
     (root + "/go.sum")
     (root + "/src/cmd/tk")
     (root + "/src/go/pkg/newpackage")  # ADD new packages here
   ];
   ```

6. **Verify the build**:
   ```bash
   nix build .#packagename  # Should succeed without errors
   ```

### Common pitfalls:

- **Forgetting `go mod tidy`**: Leads to indirect deps being marked wrong in vendor/modules.txt
- **Not updating vendorHash**: Build fails with hash mismatch
- **Missing packages in fileset**: Build fails because source files aren't included
- **Running `go mod vendor`**: NEVER do this - vendoring is handled by Nix

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
- After modifying any `.nix` files (especially `rust-deps-cell.nix`, `go-deps-cell.nix`)
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

