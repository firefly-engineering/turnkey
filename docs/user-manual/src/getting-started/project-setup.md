# Project Setup

This guide covers how to create a new Turnkey project or add Turnkey to an existing project.

## New Project

Create a new Buck2 project using the Turnkey flake template:

```bash
mkdir my-project && cd my-project
nix flake init -t github:firefly-engineering/turnkey
direnv allow  # If using direnv
```

This creates:
- `flake.nix` - Nix flake configuration with Turnkey enabled
- `toolchain.toml` - Toolchain declaration (Go enabled by default)
- `.envrc` - direnv configuration with symlink sync
- `.gitignore` - Ignores Turnkey-managed files
- `rules.star` - Root build file (template)

## Existing Project

Add Turnkey to an existing Nix flake project:

### 1. Add Turnkey Input to `flake.nix`

```nix
{
  inputs = {
    # ... existing inputs ...
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
        inputs.turnkey.flakeModules.turnkey
      ];

      # ... rest of config ...

      perSystem = { pkgs, ... }: {
        turnkey = {
          enable = true;
          declarationFile = ./toolchain.toml;
        };

        devenv.shells.default = {
          turnkey.buck2.enable = true;
        };
      };
    };
}
```

### 2. Create `toolchain.toml`

```toml
[toolchains]
go = {}
# Add more as needed: rust, python, cxx
```

### 3. Update `.gitignore`

```gitignore
# Turnkey managed files
.buckconfig
.buckroot
.turnkey/
buck-out/
```

### 4. Create `.envrc` (if using direnv)

Turnkey provides a direnv library that handles all symlink management automatically:

```bash
use flake . --no-pure-eval

# Source the turnkey library and activate
source "$TURNKEY_DIRENV_LIB"
use_turnkey
```

The `use_turnkey` function handles:
- Buck2 symlink management (`.buckconfig`, cell symlinks)
- `watch_file` declarations for automatic reloads
- Optional dependency file regeneration

Then allow it:

```bash
direnv allow
```

## Directory Structure

A typical Turnkey project has this structure:

```
my-project/
├── .buckconfig           # Buck2 configuration (generated symlink)
├── .buckroot             # Empty file marking project boundary
├── .envrc                # direnv configuration
├── .turnkey/             # Generated cells (gitignored)
│   ├── prelude/          # Buck2 prelude
│   ├── toolchains/       # Language toolchains
│   ├── godeps/           # Go dependency cell (if configured)
│   └── rustdeps/         # Rust dependency cell (if configured)
├── flake.nix             # Nix flake configuration
├── flake.lock            # Locked dependencies
├── toolchain.toml        # Toolchain declarations
├── go-deps.toml          # Go dependencies (if using Go)
├── rust-deps.toml        # Rust dependencies (if using Rust)
└── rules.star            # Root build file
```

## Generated Files

When you enter the devenv shell, Turnkey generates:

| File | Description |
|------|-------------|
| `.buckconfig` | Symlink to Nix-managed Buck2 configuration |
| `.buckroot` | Empty file marking project boundary |
| `.turnkey/toolchains` | Symlink to generated toolchains cell |
| `.turnkey/godeps` | Symlink to Go dependencies cell (if configured) |
| `.turnkey/prelude` | Symlink to the prelude |

## The Prelude

The prelude is always turnkey's: the prelude built with turnkey's pinned
buck2 release, with turnkey's patches and extensions applied. It isn't
configurable, for the same reason buck2 itself isn't: they are one release
([ADR 0002](https://github.com/firefly-engineering/turnkey/blob/main/docs/adr/0002-turnkey-owns-the-buck2-version.md)).

If you really need a prelude of your own, `prelude.path` takes a derivation
or a path. That is off the supported path, and it turns test result caching
off, since turnkey can't know which of that prelude's test rules are
cache-safe:

```nix
turnkey.toolchains.buck2.prelude.path = ./my-prelude;
```

## direnv Integration

For automatic environment activation with full Turnkey support, create `.envrc`:

```bash
use flake . --no-pure-eval

source "$TURNKEY_DIRENV_LIB"
use_turnkey
```

Then allow it:

```bash
direnv allow
```

The turnkey direnv library provides additional options:
- `use_turnkey --skip-regen` - Skip dependency file regeneration
- `use_turnkey --skip-sync` - Skip symlink synchronization
- Environment variables like `TURNKEY_SKIP_ALL=1` for CI environments

## Buck2 Configuration

The `.buckconfig` is generated automatically. For project-specific settings, create `.buckconfig.local`:

```ini
[build]
# Custom build settings

[project]
# Project-specific settings
```

## Verifying Setup

After entering the shell, verify Buck2 is configured:

```bash
# Check toolchains
buck2 targets toolchains//...

# Run Go via toolchain
buck2 run toolchains//:go[go] -- version

# Build a target
buck2 build //...
```

## Common Issues

### Symlinks Not Created

If `.turnkey/` symlinks aren't created:

1. Check that you're using direnv or manually sourcing the environment
2. Verify environment variables are set:
   ```bash
   echo $TURNKEY_BUCK2_CONFIG
   echo $TURNKEY_BUCK2_TOOLCHAINS_CELL
   ```
3. Re-allow direnv:
   ```bash
   direnv allow
   ```

### Buck2 Can't Find Cells

If Buck2 reports missing cells:

1. Check `.buckconfig` is a valid symlink:
   ```bash
   ls -la .buckconfig
   ```
2. Verify cell paths in `.buckconfig` exist:
   ```bash
   cat .buckconfig
   ```
3. Ensure you've entered the Nix shell:
   ```bash
   nix develop
   ```
