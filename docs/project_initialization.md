# Project Initialization with Turnkey

This guide covers how to create a new Buck2 project using Turnkey, or add Turnkey to an existing project.

## New Project

Create a new Buck2 project using the turnkey flake template:

```bash
mkdir my-project && cd my-project
nix flake init -t github:firefly-engineering/turnkey
direnv allow  # If using direnv
```

This creates:
- `flake.nix` - Nix flake configuration with turnkey enabled
- `toolchain.toml` - Toolchain declaration (Go enabled by default)
- `.envrc` - direnv configuration with symlink sync
- `.gitignore` - Ignores turnkey-managed files
- `BUCK` - Root BUCK file (template)

## Existing Project

Add turnkey to an existing Nix flake project:

### 1. Add turnkey input to `flake.nix`

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

### 4. Update `.envrc` (if using direnv)

Add symlink sync after `use flake`:

```bash
use flake . --no-pure-eval

# Sync turnkey symlinks on direnv reload
if [ -n "$TURNKEY_BUCK2_CONFIG" ]; then
  if [ "$(readlink .buckconfig 2>/dev/null)" != "$TURNKEY_BUCK2_CONFIG" ]; then
    ln -sf "$TURNKEY_BUCK2_CONFIG" .buckconfig
  fi
fi
if [ -n "$TURNKEY_BUCK2_TOOLCHAINS_CELL" ]; then
  mkdir -p .turnkey
  if [ "$(readlink .turnkey/toolchains 2>/dev/null)" != "$TURNKEY_BUCK2_TOOLCHAINS_CELL" ]; then
    ln -sfn "$TURNKEY_BUCK2_TOOLCHAINS_CELL" .turnkey/toolchains
  fi
fi
for var in $(env | grep '^TURNKEY_CELL_' | cut -d= -f1); do
  value="${!var}"
  cell_path="${value%%:*}"
  cell_deriv="${value#*:}"
  if [ "$(readlink "$cell_path" 2>/dev/null)" != "$cell_deriv" ]; then
    mkdir -p "$(dirname "$cell_path")"
    ln -sfn "$cell_deriv" "$cell_path"
  fi
done
```

## Prelude Strategies

Turnkey supports four strategies for providing the Buck2 prelude:

### Bundled (Default)

Uses Buck2's built-in bundled prelude. Simplest option, no configuration needed.

```nix
devenv.shells.default.turnkey.buck2 = {
  enable = true;
  prelude.strategy = "bundled";
};
```

### Git

Clones prelude from a git repository. Good for pinning to a specific version.

```nix
devenv.shells.default.turnkey.buck2 = {
  enable = true;
  prelude = {
    strategy = "git";
    gitOrigin = "https://github.com/facebook/buck2-prelude.git";
    commitHash = "abc123...";  # Required
  };
};
```

### Nix

Uses a Nix derivation containing the prelude. Best for reproducibility.

```nix
devenv.shells.default.turnkey.buck2 = {
  enable = true;
  prelude = {
    strategy = "nix";
    path = pkgs.fetchFromGitHub {
      owner = "facebook";
      repo = "buck2-prelude";
      rev = "...";
      hash = "sha256-...";
    };
  };
};
```

### Path

Uses a local filesystem path. Good for development/testing.

```nix
devenv.shells.default.turnkey.buck2 = {
  enable = true;
  prelude = {
    strategy = "path";
    path = "/path/to/local/prelude";
  };
};
```

## Generated Files

When you enter the devenv shell, turnkey generates:

| File | Description |
|------|-------------|
| `.buckconfig` | Symlink to Nix-managed Buck2 configuration |
| `.buckroot` | Empty file marking project boundary |
| `.turnkey/toolchains` | Symlink to generated toolchains cell |
| `.turnkey/godeps` | Symlink to Go dependencies cell (if configured) |
| `.turnkey/prelude` | Symlink to prelude (if using `nix` strategy) |

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
