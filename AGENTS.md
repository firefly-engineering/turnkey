# AGENTS.md - AI Assistant Guide for Turnkey

This document provides comprehensive guidance for AI assistants working on the Turnkey codebase.

## Project Overview

**Turnkey** is a toolchain management framework for Nix flakes that simplifies declaring and managing build tools in development environments.

### Core Purpose
- Bridges declarative TOML configuration (`toolchain.toml`) with Nix package resolution
- Provides reusable flake modules for other projects to import
- Integrates with `flake-parts` and `devenv` for modular development environments
- Primarily targets Buck2 build system integration (but extensible to any toolchain)

### What This Is NOT
- Not a standalone application
- Not a traditional build system
- Not a package manager replacement

### What This IS
- Infrastructure code designed to be imported by other Nix flake projects
- A "toolchain-as-code" system
- A bridge between simple TOML declarations and complex Nix package resolution

## Repository Structure

```
./
├── .envrc                          # direnv configuration for automatic flake activation
├── .gitignore                      # Git ignore patterns
├── flake.nix                       # Main Nix flake configuration
├── flake.lock                      # Locked flake dependencies
├── toolchain.toml                  # Example toolchain declaration file
├── docs/                           # Documentation
├── nix/                            # Nix modules, packages, and Buck2 prelude extensions
└── src/                            # Source code
    ├── cmd/                        # CLI tools (tk, tw, godeps-gen, etc.)
    ├── examples/                   # Example projects
    ├── rust/                       # Rust crates
    └── testdata/                   # Test fixtures
```

### Directory Organization Principles
- **Clean, minimal structure** - No unnecessary files or complexity
- **Separation of concerns** - Flake-parts integration, devenv integration, and registry are separate
- **Self-documenting** - The project uses itself as a working example

## Key Files

### `flake.nix`
**Primary flake configuration**
- Exposes `flakeModules.turnkey` (flake-parts), the one entry point; it configures the devenv module (`nix/devenv/turnkey/`) for each shell
- Supports 4 systems: x86_64-linux, aarch64-linux, x86_64-darwin, aarch64-darwin
- Demonstrates self-usage with local `toolchain.toml`
- Inputs: see the `inputs` block of `flake.nix` (nixpkgs, flake-parts, devenv, teller, toolbox, nix-pins, …)

### `nix/flake-parts/turnkey/default.nix`
**Flake-parts integration module**
- Provides perSystem-level integration
- Configures the default devenv shell automatically
- Exposes configuration options for toolchain management
- Acts as a convenience layer over the devenv module

### `nix/devenv/turnkey/default.nix`
**Devenv shell module**
- Shell-specific configuration
- Parses `toolchain.toml` to extract toolchain requirements
- Resolves toolchain names to actual packages via registry
- Adds resolved packages to the development shell

### `toolchain.toml`
**Example toolchain declaration**
```toml
[toolchains]
buck2 = {}
nix = {}
```
Simple TOML format for declaring which toolchains are needed.

### `docs/developer-manual/src/architecture/buck2.md`
- Comprehensive guide to Buck2 cell resolution
- Includes source code references with exact file paths and line numbers
- Provides working solutions to Buck2/Nix integration challenges
- Excellent example of documentation quality expected in this project

### CLI Tools

#### `src/cmd/tk/` - Buck2 Wrapper
The `tk` command (Rust) wraps `buck2` with automatic dependency sync:
- Runs `tk sync`, then rules sync (`src/rust/rules-syncer`), before commands that read the build graph (`build`, `test`, `run`, etc.)
- Pass-through for commands that don't need sync (`clean`, `kill`, etc.)
- Configured via `.turnkey/sync.toml`

```bash
tk build //some:target    # Syncs deps first, then runs buck2 build
tk --no-sync build ...    # Skip sync
```

#### `src/cmd/tw/` - Native Tool Wrapper
The `tw` command wraps native language tools (`go`, `cargo`, `uv`) with auto-sync:
- Detects when dependency files change after running commands
- Triggers appropriate sync operation (e.g., `godeps-gen` after `go get`)
- Used internally by transparent shell wrappers

```bash
tw go get github.com/foo/bar    # Runs go, syncs if go.mod changed
tw -v cargo add serde           # Verbose mode
```

**Key packages:**
- `src/cmd/tw/` - `tw` itself (Rust): running the tool, and hashing the watched files for change detection
- `src/rust/project-sync/` - `.turnkey/sync.toml` parsing, staleness and sync execution (shared by `tw` and `tk`)
- `nix/packages/tw-wrappers.nix` - Shell wrappers that shadow real tools

See `docs/user-manual/src/reference/cli.md` (the `tw` section) for full documentation.

## Architecture Patterns

### Layered Module Design

```
flake-parts module (nix/flake-parts/turnkey/default.nix)
    ↓ configures
devenv module (nix/devenv/turnkey/default.nix)
    ↓ uses
teller registry (external flake: github:firefly-engineering/teller)
    ↓ maps to
nixpkgs packages
```

### Key Architectural Decision (v1 → v2 Refactor)

**v1 (initial)**: flake-parts module directly added packages to devenv shell
**v2 (current)**: flake-parts module configures devenv module, which adds packages

**Why this matters**:
- Better separation of concerns
- Allows multiple shells with different toolchain configurations
- Cleaner API for consumers
- More flexible and composable

### The Registry Pattern

The default registry lives in [teller](https://github.com/firefly-engineering/teller), a standalone flake providing versioned toolchain registry library functions and a default set of standard nixpkgs toolchains. Turnkey-specific tools (`tw`, `turnkey-composed`) are added via `registryExtensions` in turnkey's `flake.nix`; jrsonnet comes from the toolbox overlay.

**Design principles**:
- Versioned format with `versions` and `default` attributes
- Easy to understand and extend
- Lazy evaluation for performance
- Can be overridden by consumers via `registryExtensions` or custom overlays

## Nix Code Conventions

### Formatting Style
- **Indentation**: 2 spaces (consistent throughout)
- **Function parameters**: Explicit, on separate lines
- **Attribute sets**: Multi-line with aligned braces

**Example**:
```nix
{
  config,
  pkgs,
  system,
  ...
}:
```

### The `inherit` Pattern
Used extensively to reduce namespace clutter:
```nix
let
  inherit (lib) mkOption types;
  inherit (flake-parts-lib) mkPerSystemOption;
in
```

### Module System Pattern
Follows NixOS module system conventions:
```nix
{
  options = {
    # Configuration options
  };

  config = lib.mkIf cfg.enable {
    # Implementation
  };
}
```

### Default Value Handling
```nix
registry = if cfg.registry == { } then defaultRegistry else cfg.registry;
```
Always provide sensible defaults while allowing overrides.

### TOML Parsing Pattern
```nix
toolchainDeclaration = builtins.fromTOML (builtins.readFile cfg.declarationFile);
toolchainNames = builtins.attrNames toolchainDeclaration.toolchains;
resolvedPackages = map (name: cfg.registry.${name}) toolchainNames;
```
Declarative configuration → runtime resolution.

## Monorepo Dependency Management

**CRITICAL RULE**: All dependencies in this monorepo MUST be declared at the **root level** using each language's workspace/module mechanism. Sub-projects reference these root-level dependencies rather than declaring their own versions.

This ensures:
- **Version consistency** across all code
- **Single update point** for each dependency
- **No version conflicts** in lockfiles
- **Buck2 alignment** with how dependency cells work

### Go

**Root file**: `go.mod` at repo root. This is a choice for turnkey's own repo, not a framework limit: a turnkey project can hold several Go modules as a `go.work` workspace ([ADR 0007](docs/adr/0007-go-modules-share-one-resolution-through-go-work.md)).

```
/turnkey/
├── go.mod                        # Single module: github.com/firefly-engineering/turnkey
├── go.sum                        # All dependency hashes
├── go-deps.toml                  # Generated for Nix/Buck2
└── src/examples/go-hello-deps/   # NO go.mod here - uses root module
```

- All Go code shares one module. turnkey's own tools are Rust; the Go code left is the examples (`src/examples/go-*`)
- No nested go.mod files, except test fixtures (`src/cmd/godeps-gen/testdata/godeps/*`), which are outside the Go build
- No Nix package builds Go code, so there is no `vendorHash` to update; Buck2 builds the examples against the godeps cell
- The project does not use `go mod vendor`: vendoring happens through Nix cells
- Add deps from the repo root:

```bash
go get github.com/example/package
go mod tidy  # sync direct/indirect deps
tk sync      # regenerates go-deps.toml
```

### Rust

**Root file**: `Cargo.toml` with `[workspace]` and `[workspace.dependencies]`

```
/turnkey/
├── Cargo.toml                    # [workspace.dependencies] declares ALL shared deps
├── Cargo.lock                    # Single lockfile
├── rust-deps.toml                # Generated for Nix/Buck2
└── src/cmd/jsdeps-gen/
    └── Cargo.toml                # Uses: serde.workspace = true
```

Adding a dependency:
```toml
# 1. Root Cargo.toml
[workspace.dependencies]
my-dep = "1.2.3"

# 2. Member Cargo.toml - MUST use workspace reference
[dependencies]
my-dep.workspace = true       # ✅ Correct
# my-dep = "1.2.3"            # ❌ WRONG - never specify version directly
```

### Python

**Layout**: uv workspace. The root `pyproject.toml` aggregates members; each Python package owns its own `pyproject.toml` with its own dependencies. A single `uv.lock` at the root resolves everything together.

```
/turnkey/
├── pyproject.toml                       # [tool.uv.workspace] + members as deps
├── uv.lock                              # Single resolved lockfile
├── pylock.toml                          # PEP 751 export from uv.lock
├── python-deps.toml                     # Generated for Nix/Buck2 from pylock.toml
├── src/python/<member>/
│   ├── pyproject.toml                   # Real package, hatchling backend
│   └── turnkey/<member>/                # Source under shared turnkey.* namespace
└── src/examples/python-<name>/
    └── pyproject.toml                   # Non-packaged ([tool.uv] package = false)
```

- All Python source lives under the shared `turnkey.*` PEP 420 namespace package. Members never define a `turnkey/__init__.py`.
- Cross-member deps are declared with `[tool.uv.sources]` workspace markers, mirroring `Cargo.toml`'s `workspace = true` pattern.
- Externals are declared in the member that consumes them. The lockfile reconciles versions across the workspace.
- Adding/removing deps: `uv add`/`uv remove`, or edit the member's `pyproject.toml` and run `tk sync`. `tk sync` re-exports `pylock.toml` from `uv.lock` (`uv export --all-packages --no-dev`), then regenerates `python-deps.toml` from it.
- Downstream monorepos that adopt this framework pick their own namespace (e.g. `acme.<name>`); see `docs/user-manual/src/workflows/python-workspace.md`.

### TypeScript/JavaScript

**Root file**: `package.json` at repo root (when applicable)

```
/turnkey/
├── package.json              # All dependencies declared here
├── pnpm-lock.yaml            # Lockfile (managed by pnpm)
├── js-deps.toml              # Generated for Nix/Buck2
└── ts/mypackage/             # NO package.json here (or uses workspace protocol)
```

- Use pnpm workspaces for multi-package setups
- Add deps with `pnpm add` from repo root

## Importing External Software

When incorporating external tools that need modifications, **never duplicate source code locally**. Instead, use Nix to fetch from upstream and apply patches.

### Directory Structure

```
nix/
├── packages/
│   └── tool-name.nix     # Package definition (fetches + patches)
└── patches/
    └── tool-name/        # One directory per patched software
        └── fix-something.patch
```

### Pattern: Fetch and Patch

```nix
{ pkgs, lib }:

let
  # Pin to specific commit for reproducibility
  version = "2025-01-01";
  rev = "abc123...";
  hash = "sha256-...";

  src = pkgs.fetchFromGitHub {
    owner = "upstream-org";
    repo = "upstream-repo";
    inherit rev hash;
    # Optional: fetch only needed subdirectory
    sparseCheckout = [ "path/to/tool" ];
  };

in
pkgs.buildGoModule {  # or stdenv.mkDerivation, etc.
  pname = "tool-name";
  inherit version src;
  sourceRoot = "${src.name}/path/to/tool";

  patches = [
    ../patches/tool-name/my-modification.patch
  ];

  # ... rest of build config
}
```

### Key Principles

1. **Upstream is source of truth** - We only maintain patches, not copies
2. **Pin versions explicitly** - Use specific commits/tags, not branches
3. **Document patches** - Each patch file should explain what it changes and why
4. **Organize by software** - `nix/patches/<software-name>/<patch-name>.patch`
5. **Minimal patches** - Only change what's necessary, avoid unrelated modifications

### Creating Patches

```bash
# Clone upstream, make changes, generate patch
git clone https://github.com/upstream/repo
cd repo
# ... make your changes ...
git diff > /path/to/turnkey/nix/patches/tool-name/description.patch
```

### Example: the Buck2 prelude

`nix/buck2/prelude.nix` fetches the upstream Buck2 prelude and applies the patches in `nix/patches/prelude/`.

This pattern keeps upstream as source of truth while allowing local modifications.

## Development Workflows

### Git Workflow

**Commit Message Convention**:
- Use Conventional Commits style
- Prefixes: `feat:`, `docs:`, `fix:`, `refactor:`, `test:`
- Clear, descriptive messages
- Example: `feat: add support for cargo toolchain`

**Branch Naming**:
- All changes go through pull requests

### Development Environment

**Using direnv** (recommended):
```bash
cd "$(jj root 2>/dev/null || git rev-parse --show-toplevel)"
# Environment automatically activates via .envrc
```

**Using Nix directly**:
```bash
nix develop              # Enter dev shell
nix flake show           # See available outputs
nix flake check          # Check flake validity
nix flake update         # Update dependencies
```

**Testing changes locally**:
The flake uses itself as an example, so you can test changes by:
1. Modify the code
2. Exit and re-enter the dev shell (`exit` then `nix develop`)
3. Verify the toolchains are available

### As a Module Consumer

How other projects use Turnkey:
```nix
# In another project's flake.nix
{
  inputs.turnkey.url = "github:firefly-engineering/turnkey";

  outputs = { turnkey, ... }: {
    # Use the flake-parts module
    imports = [ turnkey.flakeModules.turnkey ];

    # Configure toolchains
    turnkey.toolchains = {
      enable = true;
      declarationFile = ./toolchain.toml;
    };
  };
}
```

## Testing

### Current State
CI runs `.github/workflows/ci.yaml` (flake check, nix-format check, package builds). Locally, run `direnv exec . tk test //...`. See `docs/developer-manual/src/contributing/testing.md`.

### Manual Testing

**Use `direnv exec . <command>` for all testing**. This is the standard pattern that:
- Loads the full devenv environment
- Automatically creates/updates cell symlinks
- Works consistently for all tools (tk, cargo, go, etc.)

```bash
# Build and test with Buck2
direnv exec . tk build //src/rust/starlark-parse:starlark-parse
direnv exec . tk test //src/rust/starlark-parse:starlark-parse-test

# Run a binary
direnv exec . tk run //src/cmd/check-source-coverage-rs:check-source-coverage-rs

# Use native tools
direnv exec . cargo check
direnv exec . go test ./...
```

**After modifying Nix files** (fixups, cell builders, etc.):
```bash
rm -rf .turnkey/rustdeps  # Remove stale cell
nix develop --impure -c bash -c 'tk build //...'  # Force rebuild
```

See `docs/testing-devenv.md` for complete documentation on this testing pattern.

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

## Documentation Standards

### Code Documentation
- **Inline comments**: Explain "why" not just "what"
- **Section headers**: Clear delineation of logical sections
- **Option descriptions**: Every module option must have a clear description

**Example**:
```nix
description = "Path to toolchain.toml declaration file for the default shell (convenience option)";
```

### External Documentation
Follow the model of `docs/developer-manual/src/architecture/buck2.md`:
- **Comprehensive**: Cover the topic thoroughly
- **Source code references**: Include exact file paths and line numbers
- **Working examples**: Provide copy-paste-able code
- **Problem/solution structure**: Clearly identify issues and solutions
- **Visual aids**: Use emojis for quick scanning (✅, ❌, ⚠️) when appropriate

### Missing Documentation
Currently needed:
- **CONTRIBUTING.md**: Guidelines for contributors
- **LICENSE**: Legal terms (project appears to be open source)
- **API documentation**: Detailed module option reference

## Common Tasks

### Adding a New Toolchain

For standard nixpkgs toolchains, add to [teller](https://github.com/firefly-engineering/teller)'s `registry/default.nix`.

For project-specific tools, add to `registryExtensions` in `flake.nix`:
```nix
registryExtensions = let
  single = pkg: { versions = { "default" = pkg; }; default = "default"; };
in {
  cargo = single pkgs.cargo;
};
```

Then add to `toolchain.toml` and rebuild the dev shell to verify.

### Modifying Module Behavior

1. **Identify the right module**:
   - User-facing API changes → `nix/flake-parts/turnkey/default.nix`; Buck2 options → `nix/buck2/options.nix` (declared once, used by both modules)
   - Shell behavior changes → `nix/devenv/turnkey/default.nix`
   - Default toolchain mappings → [teller](https://github.com/firefly-engineering/teller) repo
   - Turnkey-specific tools → `registryExtensions` in `flake.nix`

2. **Follow module system patterns**:
   - Add options in `options` section
   - Implement in `config` section
   - Use `lib.mkIf` for conditional configuration

3. **Test the change**:
   - Rebuild the dev shell
   - Verify expected behavior
   - Test with the self-usage in `flake.nix`

### Updating Dependencies

```bash
nix flake update           # Update all inputs
nix flake lock --update-input nixpkgs  # Update specific input
```

Then test that everything still works.

### Adding Documentation

1. **Code documentation**: Add inline comments and option descriptions
2. **Technical docs**: Create files in `docs/` directory
3. **Follow existing patterns**: Match the quality of `docs/developer-manual/src/architecture/buck2.md`
4. **Include examples**: Always provide working code examples

## Dependencies

### Flake Inputs
- **nixpkgs** (github:NixOS/nixpkgs/nixos-unstable) - Base package collection
- **flake-parts** (github:hercules-ci/flake-parts) - Modular flake organization
- **devenv** (github:cachix/devenv) - Development environment management

## Supported Systems

- `x86_64-linux`
- `aarch64-linux`
- `x86_64-darwin` (macOS on Intel)
- `aarch64-darwin` (macOS on Apple Silicon)

When adding functionality, ensure it works across all platforms.

## Key Insights for AI Assistants

### Understanding the Project
1. **This is a library/framework, not an application** - Users import it into their flakes
2. **Self-usage is the primary test** - The `flake.nix` uses itself as an example
3. **Simplicity is a feature** - Don't over-engineer solutions
4. **Buck2 integration is a key use case** - But the design is generic
5. **Dependencies live in Nix, not the repo** - See `docs/user-manual/src/workflows/dependencies.md` for the core principles. Never vendor in-repo, always use per-module fetching with deterministic hashes.

### When Making Changes
1. **Preserve the layered architecture** - Don't blur the lines between modules
2. **Keep the registry simple** - Resist the temptation to make it complex
3. **Test with self-usage** - If the flake can't use itself, something is wrong
4. **Document thoroughly** - Match the quality of existing docs
5. **The Buck2 prelude is ours to change** - Turnkey builds its own prelude
   (`nix/buck2/prelude.nix`: upstream + `nix/patches/prelude/` +
   `nix/buck2/prelude-extensions/`), and a turnkey-owned prelude is on the table.
   "Needs a prelude change" is a cost to weigh, never a reason to rule a design out.

### Code Quality Expectations
1. **Consistent formatting** - Follow existing Nix code style
2. **Clear option descriptions** - Every user-facing option needs documentation
3. **Lazy evaluation** - Use `lazyAttrsOf` for large attribute sets
4. **Sensible defaults** - Always provide good defaults, allow overrides

## Related Resources

- **Dependency Management**: See `docs/user-manual/src/workflows/dependencies.md` for core principles on how dependencies flow from language-native declarations through Nix to Buck2 cells. **Read this before working on any dependency-related code.**
- **Native Tool Wrappers**: See the `tw` section of `docs/user-manual/src/reference/cli.md` for how `go`, `cargo`, `uv` are transparently wrapped with auto-sync.
- **Testing in Devenv**: See `docs/testing-devenv.md` for the standard `direnv exec . <command>` pattern used for all testing.
- **Buck2 Cell Resolution**: See `docs/developer-manual/src/architecture/buck2.md` for deep dive
- **flake-parts**: https://flake.parts/
- **devenv**: https://devenv.sh/
- **Nix Flakes**: https://nixos.wiki/wiki/Flakes

## Questions to Ask

When uncertain about how to proceed:

1. **Does this change maintain the layered architecture?**
2. **Is this the right module to modify?** (flake-parts vs devenv vs registry)
3. **Does this work across all 4 supported systems?**
4. **Is the documentation quality consistent with existing docs?**
5. **Does the self-usage in flake.nix still work?**
6. **Is this change simple enough, or am I over-engineering?**

## Summary

Turnkey is a well-architected, early-stage toolchain management framework for Nix flakes. It exemplifies good Nix code practices with its clean modular design, separation of concerns, and declarative approach. When working on this codebase:

- Respect the simplicity
- Maintain the layered architecture
- Document thoroughly
- Test with self-usage
- Think about consumers (projects that will import these modules)

The goal is to make toolchain management in Nix flakes as simple as declaring what you need in a TOML file.

---

## Issue Tracking

Work is tracked in [GitHub Issues](https://github.com/firefly-engineering/turnkey/issues), with priority (P0–P4) and status in the [turnkey org project](https://github.com/orgs/firefly-engineering/projects/3). Epics are issues of type `Epic` with sub-issues; blockers are GitHub issue dependencies. `docs/agents/issue-tracker.md` has the full command set.

```bash
gh issue list --search "-is:blocked no:assignee"   # Ready work: open, unclaimed, unblocked
gh issue view <n> --comments                        # Full issue details
gh issue edit <n> --add-assignee @me                # Claim
gh issue comment <n> --body "..."                   # Breadcrumbs
```

A commit that finishes an issue carries `Fixes #<n>`. Issues from before 2026-09-26 carry their old beadwork ID (`turnkey-XYZ`, as seen in older commit messages) in their footer, and [#78](https://github.com/firefly-engineering/turnkey/issues/78) maps every migrated ID; the `beadwork` branch is a read-only archive of the old tracker.

Committing, closing issues, and pushing are part of completing a task — not separate actions requiring additional permission.

## Landing the Plane (Session Completion)

A task is finished when its change is on the remote, because unpushed work is stranded locally:

1. File issues for remaining follow-up work.
2. Run the quality gates above if code changed.
3. Close finished issues; update in-progress ones.
4. Push, and confirm the remote has the change. If the push fails, resolve it and push again.
5. Leave a short handoff note for the next session.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `firefly-engineering/turnkey`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), applied with `gh issue edit --add-label`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root (created lazily). See `docs/agents/domain.md`.
