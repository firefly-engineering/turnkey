# CLAUDE.md - AI Assistant Guide for Turnkey

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
/home/user/turnkey/
├── .envrc                          # direnv configuration for automatic flake activation
├── .gitignore                      # Git ignore patterns
├── flake.nix                       # Main Nix flake configuration
├── flake.lock                      # Locked flake dependencies
├── toolchain.toml                  # Example toolchain declaration file
├── cmd/
│   ├── tk/                         # tk CLI - Buck2 wrapper with auto-sync
│   └── tw/                         # tw CLI - Native tool wrapper for auto-sync
├── docs/
│   ├── buck2_cell_resolution.md    # Comprehensive Buck2 cell resolution documentation
│   └── native-tool-wrappers.md     # How go/cargo/uv are wrapped with auto-sync
└── nix/
    ├── devenv/
    │   └── turnkey/
    │       └── default.nix         # Devenv module for turnkey
    ├── flake-parts/
    │   └── turnkey/
    │       └── default.nix         # Flake-parts module for turnkey
    └── registry/
        └── default.nix             # Default toolchain registry mapping names to packages
```

### Directory Organization Principles
- **Clean, minimal structure** - No unnecessary files or complexity
- **Separation of concerns** - Flake-parts integration, devenv integration, and registry are separate
- **Self-documenting** - The project uses itself as a working example

## Key Files

### `/home/user/turnkey/flake.nix`
**Primary flake configuration**
- Exposes `flakeModules.turnkey` (flake-parts) and `devenvModules.turnkey` (devenv)
- Supports 4 systems: x86_64-linux, aarch64-linux, x86_64-darwin, aarch64-darwin
- Demonstrates self-usage with local `toolchain.toml`
- Inputs: nixpkgs (unstable), flake-parts, devenv

### `/home/user/turnkey/nix/flake-parts/turnkey/default.nix`
**Flake-parts integration module**
- Provides perSystem-level integration
- Configures the default devenv shell automatically
- Exposes configuration options for toolchain management
- Acts as a convenience layer over the devenv module

### `/home/user/turnkey/nix/devenv/turnkey/default.nix`
**Devenv shell module**
- Shell-specific configuration
- Parses `toolchain.toml` to extract toolchain requirements
- Resolves toolchain names to actual packages via registry
- Adds resolved packages to the development shell

### `/home/user/turnkey/nix/registry/default.nix`
**Toolchain registry**
- Simple mapping: toolchain name → Nix package
- Currently supports: `buck2`, `nix`
- Designed to be extensible (add new toolchains here)

### `/home/user/turnkey/toolchain.toml`
**Example toolchain declaration**
```toml
[toolchains]
buck2 = {}
nix = {}
```
Simple TOML format for declaring which toolchains are needed.

### `/home/user/turnkey/docs/buck2_cell_resolution.md`
**Production-grade technical documentation** (325 lines)
- Comprehensive guide to Buck2 cell resolution
- Includes source code references with exact file paths and line numbers
- Provides working solutions to Buck2/Nix integration challenges
- Excellent example of documentation quality expected in this project

### CLI Tools

#### `cmd/tk/` - Buck2 Wrapper
The `tk` command wraps `buck2` with automatic dependency sync:
- Runs `tk sync` before commands that read the build graph (`build`, `test`, `run`, etc.)
- Pass-through for commands that don't need sync (`clean`, `kill`, etc.)
- Configured via `.turnkey/sync.toml`

```bash
tk build //some:target    # Syncs deps first, then runs buck2 build
tk --no-sync build ...    # Skip sync
```

#### `cmd/tw/` - Native Tool Wrapper
The `tw` command wraps native language tools (`go`, `cargo`, `uv`) with auto-sync:
- Detects when dependency files change after running commands
- Triggers appropriate sync operation (e.g., `godeps-gen` after `go get`)
- Used internally by transparent shell wrappers

```bash
tw go get github.com/foo/bar    # Runs go, syncs if go.mod changed
tw -v cargo add serde           # Verbose mode
```

**Key packages:**
- `go/pkg/syncconfig/` - Configuration parsing for `.turnkey/sync.toml`
- `go/pkg/syncer/` - Sync execution logic
- `go/pkg/snapshot/` - File hashing for change detection
- `nix/packages/tw-wrappers.nix` - Shell wrappers that shadow real tools

See `docs/native-tool-wrappers.md` for full documentation.

## Architecture Patterns

### Layered Module Design

```
flake-parts module (nix/flake-parts/turnkey/default.nix)
    ↓ configures
devenv module (nix/devenv/turnkey/default.nix)
    ↓ uses
registry (nix/registry/default.nix)
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

The registry is intentionally simple:
```nix
{
  buck2 = pkgs.buck2;
  nix = pkgs.nix;
  # Add more toolchains here
}
```

**Design principles**:
- Simple attribute set, nothing fancy
- Easy to understand and extend
- Lazy evaluation for performance
- Can be overridden by consumers

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

## Go Module Layout (Monorepo)

This repository uses a **single go.mod at the root** for all Go code. This is a monorepo pattern where all Go tools and examples share the same module.

### Key Principles

1. **Single go.mod at repo root** - All Go code shares one module: `github.com/firefly-engineering/turnkey`
2. **No nested go.mod files** - Tools like `tools/godeps-gen` do NOT have their own go.mod
3. **Shared dependencies** - All Go dependencies are declared in the root go.mod/go.sum
4. **Subpackage imports** - Internal packages use full import paths like `github.com/firefly-engineering/turnkey/tools/godeps-gen`

### Directory Structure
```
/turnkey/
├── go.mod                    # Single module declaration
├── go.sum                    # All dependency hashes
├── go-deps.toml              # Generated Nix dependency declarations
├── tools/
│   └── godeps-gen/
│       └── main.go           # Tool code (no go.mod here!)
└── examples/
    └── hello-deps/
        └── main.go           # Example code (no go.mod here!)
```

### Building Go Tools

```bash
# From repo root - builds tools/godeps-gen/main.go
go build -o godeps-gen ./tools/godeps-gen

# Run directly
go run ./tools/godeps-gen --help
```

### Nix Packaging Considerations

For Go tools, use standard `buildGoModule` with `vendorHash`:

```nix
pkgs.buildGoModule {
  pname = "godeps-gen";
  src = ./.;  # Repo root
  subPackages = [ "cmd/godeps-gen" ];
  vendorHash = "sha256-...";  # Let build fail once to get this
}
```

For **dependency cells** (consumed by Buck2), use per-module fetching - see `docs/dependency-management.md`.

### Why Monorepo?

1. **Consistency** - All code uses same dependency versions
2. **Simplicity** - One place to manage deps, one go.sum to update
3. **Buck2 alignment** - Matches Buck2's cell-based monorepo model
4. **Dogfooding** - Tools can depend on library code in the same repo

## Importing External Software

When incorporating external tools that need modifications, **never duplicate source code locally**. Instead, use Nix to fetch from upstream and apply patches.

### Directory Structure

```
nix/
├── packages/
│   └── gobuckify.nix      # Package definition (fetches + patches)
└── patches/
    └── gobuckify/         # One directory per patched software
        └── use-go-directly.patch
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

### Example: gobuckify

gobuckify is fetched from facebook/buck2 and patched to use `go` directly:

```nix
# nix/packages/gobuckify.nix
src = pkgs.fetchFromGitHub {
  owner = "facebook";
  repo = "buck2";
  rev = "54ad016...";
  hash = "sha256-...";
  sparseCheckout = [ "prelude/go/tools/gobuckify" ];
};

patches = [ ../patches/gobuckify/use-go-directly.patch ];
```

The patch modifies one function to use `$GO_BINARY` or `go` instead of `buck2 run`.

## Development Workflows

### Git Workflow

**Commit Message Convention**:
- Use Conventional Commits style
- Prefixes: `feat:`, `docs:`, `fix:`, `refactor:`, `test:`
- Clear, descriptive messages
- Example: `feat: add support for cargo toolchain`

**Branch Naming**:
- Feature branches: `sigma/push-<randomid>` or descriptive names
- Claude branches: `claude/claude-md-<sessionid>`
- All changes go through pull requests

**Current Branch**: `claude/claude-md-mi24hguxg9tt783j-012hJXSBmAEfcSKL2eTc48Wc`

### Development Environment

**Using direnv** (recommended):
```bash
cd /home/user/turnkey
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
- **No testing infrastructure** currently exists
- No CI/CD pipelines
- No automated checks
- This is early-stage infrastructure code

### Future Testing Strategy
When implementing tests, consider:
1. **Nix evaluation tests** - Verify module system works correctly
2. **Integration tests** - Ensure toolchains are properly resolved
3. **Example project tests** - Test with real Buck2 projects
4. **Cross-platform tests** - Verify all 4 supported systems work

### Manual Testing
Currently, testing involves:
1. Making changes to the Nix code
2. Rebuilding the dev shell
3. Verifying expected toolchains are available
4. Testing with real projects that import the module

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
Follow the model of `docs/buck2_cell_resolution.md`:
- **Comprehensive**: Cover the topic thoroughly
- **Source code references**: Include exact file paths and line numbers
- **Working examples**: Provide copy-paste-able code
- **Problem/solution structure**: Clearly identify issues and solutions
- **Visual aids**: Use emojis for quick scanning (✅, ❌, ⚠️) when appropriate

### Missing Documentation
Currently needed:
- **README.md**: Project overview, quick start, usage examples
- **CONTRIBUTING.md**: Guidelines for contributors
- **LICENSE**: Legal terms (project appears to be open source)
- **API documentation**: Detailed module option reference

## Common Tasks

### Adding a New Toolchain

1. **Update the registry** (`nix/registry/default.nix`):
```nix
{
  buck2 = pkgs.buck2;
  nix = pkgs.nix;
  cargo = pkgs.cargo;  # Add new toolchain
}
```

2. **Test locally** - Add to `toolchain.toml`:
```toml
[toolchains]
buck2 = {}
nix = {}
cargo = {}
```

3. **Verify** - Rebuild dev shell and check `cargo` is available

### Modifying Module Behavior

1. **Identify the right module**:
   - User-facing API changes → `nix/flake-parts/turnkey/default.nix`
   - Shell behavior changes → `nix/devenv/turnkey/default.nix`
   - Toolchain mappings → `nix/registry/default.nix`

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
3. **Follow existing patterns**: Match the quality of `buck2_cell_resolution.md`
4. **Include examples**: Always provide working code examples

## Dependencies

### Flake Inputs
- **nixpkgs** (github:NixOS/nixpkgs/nixos-unstable) - Base package collection
- **flake-parts** (github:hercules-ci/flake-parts) - Modular flake organization
- **devenv** (github:cachix/devenv) - Development environment management

### Transitive Dependencies
devenv brings in:
- cachix (binary cache)
- git-hooks (pre-commit hooks)
- nix (custom version 2.30.6)
- flake-compat

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
5. **Dependencies live in Nix, not the repo** - See `docs/dependency-management.md` for the core principles. Never vendor in-repo, always use per-module fetching with deterministic hashes.

### When Making Changes
1. **Preserve the layered architecture** - Don't blur the lines between modules
2. **Keep the registry simple** - Resist the temptation to make it complex
3. **Test with self-usage** - If the flake can't use itself, something is wrong
4. **Document thoroughly** - Match the quality of existing docs

### Code Quality Expectations
1. **Consistent formatting** - Follow existing Nix code style
2. **Clear option descriptions** - Every user-facing option needs documentation
3. **Lazy evaluation** - Use `lazyAttrsOf` for large attribute sets
4. **Sensible defaults** - Always provide good defaults, allow overrides

### Communication Style
- **Be precise** - This is infrastructure code, accuracy matters
- **Reference source** - When discussing Buck2 or Nix behavior, cite sources
- **Explain rationale** - Don't just say what, explain why
- **Provide examples** - Working code examples are essential

## Project Maturity

**Current State**: Early stage (5 commits total)
- Core architecture established
- Key modules implemented
- Self-usage working
- Excellent Buck2 documentation
- No testing infrastructure yet
- No README or general documentation yet

**Next Steps** (likely):
1. Add README.md with quick start guide
2. Implement testing infrastructure
3. Expand registry with more common toolchains
4. Add CI/CD for automated checks
5. Document the module API comprehensively
6. Add example projects showing integration

## Related Resources

- **Dependency Management**: See `docs/dependency-management.md` for core principles on how dependencies flow from language-native declarations through Nix to Buck2 cells. **Read this before working on any dependency-related code.**
- **Native Tool Wrappers**: See `docs/native-tool-wrappers.md` for how `go`, `cargo`, `uv` are transparently wrapped with auto-sync.
- **Buck2 Cell Resolution**: See `docs/buck2_cell_resolution.md` for deep dive
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

## Git Operations

### Pushing Changes

```bash
# Develop on the designated branch
git add .
git commit -m "feat: descriptive message"

# Push to the current branch
git push -u origin claude/claude-md-mi24hguxg9tt783j-012hJXSBmAEfcSKL2eTc48Wc
```

### Network Retry Strategy
If push fails due to network errors, retry up to 4 times with exponential backoff (2s, 4s, 8s, 16s).

## Summary

Turnkey is a well-architected, early-stage toolchain management framework for Nix flakes. It exemplifies good Nix code practices with its clean modular design, separation of concerns, and declarative approach. When working on this codebase:

- Respect the simplicity
- Maintain the layered architecture
- Document thoroughly
- Test with self-usage
- Think about consumers (projects that will import these modules)

The goal is to make toolchain management in Nix flakes as simple as declaring what you need in a TOML file.

<!-- bv-agent-instructions-v1 -->

---

## Beads Workflow Integration

This project uses [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) for issue tracking. Issues are stored in `.beads/` and tracked in git.

### Essential Commands

```bash
# View issues (launches TUI - avoid in automated sessions)
bv

# CLI commands for agents (use these instead)
bd ready              # Show issues ready to work (no blockers)
bd list --status=open # All open issues
bd show <id>          # Full issue details with dependencies
bd create --title="..." --type=task --priority=2
bd update <id> --status=in_progress
bd close <id> --reason="Completed"
bd close <id1> <id2>  # Close multiple issues at once
bd sync               # Commit and push changes
```

### Workflow Pattern

1. **Start**: Run `bd ready` to find actionable work
2. **Claim**: Use `bd update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `bd close <id>`
5. **Sync**: Always run `bd sync` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `bd ready` shows only unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers, not words)
- **Types**: task, bug, feature, epic, question, docs
- **Blocking**: `bd dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
bd sync                 # Commit beads changes
git commit -m "..."     # Commit code
bd sync                 # Commit any new beads changes
git push                # Push to remote
```

### Best Practices

- Check `bd ready` at session start to find available work
- Update status as you work (in_progress → closed)
- Create new issues with `bd create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always `bd sync` before ending session

### Using bv as an AI sidecar

bv is a graph-aware triage engine for Beads projects (.beads/beads.jsonl). Instead of parsing JSONL or hallucinating graph traversal, use robot flags for deterministic, dependency-aware outputs with precomputed metrics (PageRank, betweenness, critical path, cycles, HITS, eigenvector, k-core).

**Scope boundary:** bv handles *what to work on* (triage, priority, planning). For agent-to-agent coordination (messaging, work claiming, file reservations), use [MCP Agent Mail](https://github.com/Dicklesworthstone/mcp_agent_mail).

** CRITICAL: Use ONLY `--robot-*` flags. Bare `bv` launches an interactive TUI that blocks your session.**

#### The Workflow: Start With Triage

**`bv --robot-triage` is your single entry point.** It returns everything you need in one call:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

bv --robot-triage        # THE MEGA-COMMAND: start here
bv --robot-next          # Minimal: just the single top pick + claim command

#### Other Commands

**Planning:**
| Command | Returns |
|---------|---------|
| `--robot-plan` | Parallel execution tracks with `unblocks` lists |
| `--robot-priority` | Priority misalignment detection with confidence |

**Graph Analysis:**
| Command | Returns |
|---------|---------|
| `--robot-insights` | Full metrics: PageRank, betweenness, HITS (hubs/authorities), eigenvector, critical path, cycles, k-core, articulation points, slack |
| `--robot-label-health` | Per-label health: `health_level` (healthy\|warning\|critical), `velocity_score`, `staleness`, `blocked_count` |
| `--robot-label-flow` | Cross-label dependency: `flow_matrix`, `dependencies`, `bottleneck_labels` |
| `--robot-label-attention [--attention-limit=N]` | Attention-ranked labels by: (pagerank  staleness  block_impact) / velocity |

**History & Change Tracking:**
| Command | Returns |
|---------|---------|
| `--robot-history` | Bead-to-commit correlations: `stats`, `histories` (per-bead events/commits/milestones), `commit_index` |
| `--robot-diff --diff-since <ref>` | Changes since ref: new/closed/modified issues, cycles introduced/resolved |

**Other Commands:**
| Command | Returns |
|---------|---------|
| `--robot-burndown <sprint>` | Sprint burndown, scope changes, at-risk items |
| `--robot-forecast <id\|all>` | ETA predictions with dependency-aware scheduling |
| `--robot-alerts` | Stale issues, blocking cascades, priority mismatches |
| `--robot-suggest` | Hygiene: duplicates, missing deps, label suggestions, cycle breaks |
| `--robot-graph [--graph-format=json\|dot\|mermaid]` | Dependency graph export |
| `--export-graph <file.html>` | Self-contained interactive HTML visualization |

#### Scoping & Filtering

bv --robot-plan --label backend              # Scope to label's subgraph
bv --robot-insights --as-of HEAD~30          # Historical point-in-time
bv --recipe actionable --robot-plan          # Pre-filter: ready to work (no blockers)
bv --recipe high-impact --robot-triage       # Pre-filter: top PageRank scores
bv --robot-triage --robot-triage-by-track    # Group by parallel work streams
bv --robot-triage --robot-triage-by-label    # Group by domain

#### Understanding Robot Output

**All robot JSON includes:**
- `data_hash`  Fingerprint of source beads.jsonl (verify consistency across calls)
- `status`  Per-metric state: `computed|approx|timeout|skipped` + elapsed ms
- `as_of` / `as_of_commit`  Present when using `--as-of`; contains ref and resolved SHA

**Two-phase analysis:**
- **Phase 1 (instant):** degree, topo sort, density  always available immediately
- **Phase 2 (async, 500ms timeout):** PageRank, betweenness, HITS, eigenvector, cycles  check `status` flags

**For large graphs (>500 nodes):** Some metrics may be approximated or skipped. Always check `status`.

#### jq Quick Reference

bv --robot-triage | jq '.quick_ref'                        # At-a-glance summary
bv --robot-triage | jq '.recommendations[0]'               # Top recommendation
bv --robot-plan | jq '.plan.summary.highest_impact'        # Best unblock target
bv --robot-insights | jq '.status'                         # Check metric readiness
bv --robot-insights | jq '.Cycles'                         # Circular deps (must fix!)
bv --robot-label-health | jq '.results.labels[] | select(.health_level == "critical")'

**Performance:** Phase 1 instant, Phase 2 async (500ms timeout). Prefer `--robot-plan` over `--robot-insights` when speed matters. Results cached by data hash.

Use bv instead of parsing beads.jsonlit computes PageRank, critical paths, cycles, and parallel tracks deterministically.

<!-- end-bv-agent-instructions -->
