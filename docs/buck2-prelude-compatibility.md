# Buck2 and Prelude Version Compatibility

The Buck2 binary and its prelude must be version-matched. Using a mismatched prelude can cause cryptic Starlark errors like "Unexpected parameter" or "Missing named-only parameter".

## Current Versions

| Component | Version | Source |
|-----------|---------|--------|
| buck2 binary | 2025-12-01 | nixpkgs (commit `75e4243c93877a3db4acf55f20d2e80a32523233`) |
| buck2-prelude | 2025-11-28 | `nix/buck2/prelude.nix` (commit `0fabd579c12c585c612ecab4f397b50aae334099`) |

## Why Version Matching Matters

The buck2 binary defines Starlark APIs (like `ErlangErrorHandlers`, `ActionErrorCtx`, etc.) that the prelude consumes. When these APIs change:

- **Prelude too new**: Uses parameters the binary doesn't recognize → "Unexpected parameter"
- **Prelude too old**: Missing parameters the binary expects → "Missing named-only parameter"

## How to Find Compatible Versions

### 1. Check current buck2 version

```bash
buck2 --version
# Output: buck2 2025-12-01-75e4243c93877a3db4acf55f20d2e80a32523233
```

The format is `buck2 <date>-<commit>`.

### 2. Find matching prelude commit

The buck2-prelude repo is updated in sync with buck2. Find a prelude commit from the same date or slightly before:

```bash
# Get commits from around the buck2 release date
curl -s "https://api.github.com/repos/facebook/buck2-prelude/commits?until=2025-12-02T00:00:00Z&per_page=5" | \
  jq -r '.[] | "\(.sha) \(.commit.committer.date)"'
```

Choose a commit from the same day or 1-2 days before the buck2 release.

### 3. Update prelude.nix

Edit `nix/buck2/prelude.nix`:

```nix
version = "2025-11-28";  # Date of the prelude commit
rev = "0fabd579c12c585c612ecab4f397b50aae334099";  # Prelude commit hash

upstreamPrelude = pkgs.fetchFromGitHub {
  owner = "facebook";
  repo = "buck2-prelude";
  inherit rev;
  hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";  # Will fail, get correct hash from error
};
```

### 4. Get the correct hash

Run a build to get the correct hash:

```bash
nix build .#turnkey-prelude 2>&1 | grep "got:"
# Output: got:    sha256-h/NYUh+vcESfb8LpvTSoiCoSOnqg0birTseNXAxlt6Q=
```

Update the hash in `prelude.nix`.

### 5. Test the new prelude

```bash
# Kill any cached daemon state
buck2 kill

# Test a build
buck2 build //examples/go-hello:go-hello

# Run the test suite
buck2 test //...
```

## When to Update

Update both buck2 and prelude when:

1. **nixpkgs updates buck2** - Check `nix flake update` changes
2. **You need new prelude features** - Requires matching buck2 version
3. **Build errors appear after nixpkgs update** - Version mismatch

## Checklist for Updates

- [ ] Check new buck2 version: `buck2 --version`
- [ ] Find matching prelude commit (same date or earlier)
- [ ] Update `nix/buck2/prelude.nix` with new rev and hash
- [ ] Update version comment in prelude.nix
- [ ] Kill buck2 daemon: `buck2 kill`
- [ ] Test: `buck2 test //...`
- [ ] Run E2E tests: `./e2e/harness/runner.sh all`
- [ ] Update this document's "Current Versions" table

## Troubleshooting

### "Unexpected parameter named `X`"

The prelude is newer than the buck2 binary. Use an older prelude commit.

### "Missing named-only parameter `X`"

The prelude is older than the buck2 binary. Use a newer prelude commit.

### Changes not taking effect

Buck2 caches cell state. Kill the daemon:

```bash
buck2 kill
```

### Finding the bundled prelude version

The bundled prelude (used with `prelude.strategy = "bundled"`) is embedded in the buck2 binary. To see its content:

```bash
# After a build, the bundled prelude is extracted to:
ls buck-out/v2/external_cells/bundled/prelude/
```

Compare files between bundled and your nix prelude to debug mismatches.
