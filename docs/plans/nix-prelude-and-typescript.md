# Plan: Nix-Backed Prelude Cell + TypeScript Support

## Goal

Replace the bundled prelude with a Nix-backed prelude cell that:
1. Fetches upstream buck2-prelude at a pinned commit
2. Can apply patches for turnkey-specific customizations
3. Can be extended with custom rules (starting with TypeScript)

This enables TypeScript integration and future language customizations without forking the prelude.

## Architecture

```
.turnkey/
├── toolchains -> /nix/store/...-turnkey-toolchains-cell
├── godeps     -> /nix/store/...-go-deps-cell
├── rustdeps   -> /nix/store/...-rust-deps-cell
├── pydeps     -> /nix/store/...-python-deps-cell
└── prelude    -> /nix/store/...-turnkey-prelude  # NEW
```

The prelude derivation combines:
- Upstream buck2-prelude (fetched at pinned commit)
- Turnkey patches (optional, in `nix/patches/prelude/`)
- Custom rules (in `nix/buck2/prelude-extensions/`)

## Implementation Phases

### Phase 1: Create Nix-Backed Prelude Derivation

**Files to create:**

| File | Purpose |
|------|---------|
| `nix/buck2/prelude.nix` | Main derivation that builds turnkey-prelude |
| `nix/buck2/prelude-extensions/` | Directory for custom Starlark rules |

**nix/buck2/prelude.nix structure:**
```nix
{ pkgs, lib }:

let
  # Pin to specific buck2-prelude commit (should match buck2 version)
  upstreamPrelude = pkgs.fetchFromGitHub {
    owner = "facebook";
    repo = "buck2-prelude";
    rev = "...";  # Pin to commit matching buck2 version
    hash = "sha256-...";
  };

  # Find patches to apply
  patchDir = ../patches/prelude;
  patches = if builtins.pathExists patchDir
    then builtins.filter (p: lib.hasSuffix ".patch" p) (builtins.attrNames (builtins.readDir patchDir))
    else [];
in
pkgs.runCommand "turnkey-prelude" {
  inherit upstreamPrelude;
} ''
  cp -r $upstreamPrelude $out
  chmod -R u+w $out

  # Apply patches if any exist
  ${lib.concatMapStringsSep "\n" (p: "patch -d $out -p1 < ${patchDir}/${p}") patches}

  # Copy custom extensions (merged into prelude)
  # Extensions can add new .bzl files or override existing ones
  if [ -d ${./prelude-extensions} ]; then
    cp -r ${./prelude-extensions}/* $out/
  fi
''
```

### Phase 2: Integrate with Turnkey Module

**Files to modify:**

| File | Change |
|------|--------|
| `nix/flake-parts/turnkey/default.nix` | Add prelude derivation, change default strategy |
| `nix/devenv/turnkey/buck2.nix` | Consume prelude derivation from flake-parts |
| `flake.nix` | Export prelude package |

**Key changes:**
1. Build turnkey-prelude derivation in flake-parts
2. Change `prelude.strategy` default from `"bundled"` to `"nix"`
3. Auto-set `prelude.path` to the turnkey-prelude derivation
4. Keep `"bundled"` as opt-in fallback for compatibility

### Phase 3: Pin Prelude Version

**Research needed:**
- Determine correct buck2-prelude commit to match nixpkgs buck2 version
- Document version compatibility requirements

The buck2 binary and prelude should be version-matched. Check:
```bash
buck2 --version  # Get buck2 version
# Find corresponding prelude commit in buck2 repo
```

### Phase 4: Add TypeScript Support

**Files to create:**

| File | Purpose |
|------|---------|
| `nix/buck2/prelude-extensions/typescript/` | TypeScript rules |
| `nix/packages/tsdeps-gen.nix` | TypeScript deps generator (like pydeps-gen) |
| `nix/buck2/typescript-deps-cell.nix` | TypeScript deps cell builder |
| `e2e/fixtures/multi-language/typescript_app/` | TypeScript fixture |

**TypeScript rules needed:**
- `typescript_library` - compile TS to JS
- `typescript_binary` - runnable TS application
- `typescript_test` - TS test runner

**Deps cell pattern:**
- Input: `package.json` + `package-lock.json` (or `pnpm-lock.yaml`)
- Output: `typescript-deps.toml` with npm package URLs and hashes
- Cell: Pre-fetched npm packages for hermetic builds

### Phase 5: Update E2E Tests

**Files to modify:**

| File | Change |
|------|--------|
| `e2e/tests/04-multi-language.sh` | Add TypeScript to the mix |
| `e2e/tests/07-reproducibility.sh` | Verify TS reproducibility |

## Task Breakdown

### Epic: Nix-Backed Prelude (turnkey-prelude)

1. **[P2] Create prelude.nix derivation**
   - Fetch upstream buck2-prelude
   - Set up patch application mechanism
   - Create prelude-extensions directory structure
   - Export from flake.nix

2. **[P2] Pin prelude to matching buck2 version**
   - Research buck2/prelude version compatibility
   - Determine correct commit hash
   - Document version policy

3. **[P2] Integrate prelude into turnkey module**
   - Update flake-parts/default.nix
   - Update devenv/buck2.nix
   - Change default strategy to "nix"
   - Keep "bundled" as fallback option

4. **[P2] Update E2E tests for new prelude**
   - Verify all existing tests pass with nix prelude
   - Add test for prelude strategy switching

### Epic: TypeScript Support (turnkey-typescript)

5. **[P3] Create TypeScript rules in prelude-extensions**
   - typescript_library rule
   - typescript_binary rule
   - typescript_test rule
   - Toolchain definition

6. **[P3] Create tsdeps-gen tool**
   - Parse package-lock.json / pnpm-lock.yaml
   - Generate typescript-deps.toml with SRI hashes
   - Prefetch npm packages

7. **[P3] Create typescript-deps-cell.nix**
   - Build deps cell from typescript-deps.toml
   - Generate BUCK files for npm packages

8. **[P3] Add TypeScript to turnkey module**
   - Add typescript options to buck2 config
   - Add tsdeps-gen to registry
   - Generate tsdeps cell symlink

9. **[P3] Add TypeScript E2E tests**
   - Add typescript_app to multi-language fixture
   - Update multi-language and reproducibility tests

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Prelude/Buck2 version mismatch | Pin prelude commit to match buck2 version in nixpkgs |
| Breaking existing projects | Keep "bundled" as fallback, test migration path |
| TypeScript ecosystem complexity | Start simple (no monorepo, basic npm), expand later |
| Prelude API changes | Pin to stable commit, update deliberately |

## Success Criteria

1. All existing E2E tests pass with `prelude.strategy = "nix"`
2. Can build/run TypeScript targets in multi-language project
3. TypeScript builds are reproducible (hash-identical)
4. Downstream projects can opt into TypeScript with minimal config

## Open Questions

1. Should we use npm or pnpm for the deps cell? (pnpm has better lockfile format)
2. Do we need to support TypeScript project references for monorepos?
3. Should TypeScript rules live in prelude-extensions or a separate cell?
