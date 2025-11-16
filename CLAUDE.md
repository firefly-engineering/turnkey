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
├── docs/
│   └── buck2_cell_resolution.md    # Comprehensive Buck2 cell resolution documentation
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
- Exposes two flake modules: `turnkey` (flake-parts) and `turnkey-devenv` (devenv)
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
