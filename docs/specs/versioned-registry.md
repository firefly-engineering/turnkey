# Versioned Registry Specification

**Status:** Implemented
**Issue:** turnkey-xcpo
**Author:** Claude
**Date:** 2026-01-20

## Overview

This document specifies the interface for versioned toolchain registries in Turnkey. A registry is a Nix flake that provides multiple versions of toolchains, allowing projects to pin specific tool versions.

## Goals

1. **Version pinning**: Projects can request specific toolchain versions (e.g., Go 1.22, Python 3.12)
2. **Reproducibility**: Same toolchain.toml produces same environment across machines
3. **Composability**: Multiple registries can be combined (e.g., official + custom)
4. **Simplicity**: Simple cases remain simple (no version = sensible default)

## Non-Goals

1. **Version resolution/constraints**: No semver ranges like `>=1.22 <2.0`. Exact versions only.
2. **Automatic updates**: Registry versions are pinned via flake.lock, not auto-updated.
3. **Cross-platform version mapping**: Each system provides its own versions.

---

## Registry Flake Interface

A valid registry flake MUST expose an **overlay** that adds toolchains to the `turnkeyRegistry` attribute in pkgs. To ensure correct composition, registries SHOULD use the `mkRegistryOverlay` helper provided by Turnkey.

### Core Structure

```nix
{
  overlays.default = turnkey.lib.mkRegistryOverlay (final: prev: {
    <toolchain-name> = {
      versions = {
        "<version-string>" = <derivation>;
        "<version-string>" = <derivation>;
        # ...
      };
      default = "<version-string>";  # REQUIRED: must match a key in versions
    };
    # ...
  });
}
```

The function receives both `final` (for dependencies) and `prev` (for overrides), just like a standard Nix overlay.

### The mkRegistryOverlay Helper

Turnkey provides a helper function that handles two-level merging:

1. **Toolchain level**: New toolchains are added, existing toolchains are merged
2. **Version level**: Versions are combined additively, `default` is overridden

```nix
# Provided by turnkey
lib.mkRegistryOverlay = packagesFn: final: prev:
  let
    prevRegistry = prev.turnkeyRegistry or {};
    newPackages = packagesFn final prev;  # Both final and prev available

    # Merge a single toolchain: combine versions, override default
    mergeToolchain = name: new:
      let
        existing = prevRegistry.${name} or null;
      in
        if existing == null then new
        else {
          versions = (existing.versions or {}) // (new.versions or {});
          default = if new ? default then new.default else existing.default;
        };
  in {
    turnkeyRegistry = prevRegistry // (builtins.mapAttrs mergeToolchain newPackages);
  };
```

### Why This Design?

1. **Safe composition**: The helper ensures correct merging - registry authors can't forget the pattern
2. **Additive versions**: Multiple registries can contribute versions to the same toolchain
3. **Predictable overrides**: Later overlays override `default`, not clobber entire toolchains
4. **Full overlay power**: Both `final` and `prev` available for package references and overrides
5. **Lazy evaluation**: Only requested toolchains are evaluated

### Example Registry Flake

```nix
{
  description = "Turnkey official toolchain registry";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = { self, nixpkgs, turnkey }: {
    overlays.default = turnkey.lib.mkRegistryOverlay (final: prev: {
      go = {
        versions = {
          "1.21" = final.go_1_21;
          "1.22" = final.go_1_22;
          "1.23" = final.go_1_23;
        };
        default = "1.23";
      };

      python = {
        versions = {
          "3.11" = final.python311;
          "3.12" = final.python312;
          "3.13" = final.python313;
        };
        default = "3.12";
      };

      rust = {
        versions = {
          # Use final.rust-bin if rust-overlay composed before us
          "1.75" = final.rustc;
          "1.76" = final.rustc;
          "1.77" = final.rustc;
        };
        default = "1.77";
      };

      nodejs = {
        versions = {
          "18" = final.nodejs_18;
          "20" = final.nodejs_20;
          "22" = final.nodejs_22;
        };
        default = "20";  # LTS
      };
    });
  };
}
```

### Composing Registries

Multiple registries compose via overlay stacking. The `mkRegistryOverlay` helper ensures versions are merged additively:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
    turnkey-registry.url = "github:firefly-engineering/turnkey-registry";
    rust-overlay.url = "github:oxalica/rust-overlay";
    my-registry.url = "github:myorg/my-toolchain-registry";
  };

  outputs = { nixpkgs, turnkey-registry, rust-overlay, my-registry, ... }:
    let
      # Compose overlays - versions merge, defaults override
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [
          rust-overlay.overlays.default      # Provides rust-bin
          turnkey-registry.overlays.default  # Official registry (uses rust-bin)
          my-registry.overlays.default       # Adds versions, may change defaults
        ];
      };
    in {
      # pkgs.turnkeyRegistry has merged toolchains from all registries
    };
}
```

### Composition Example

```nix
# Official registry provides:
go = {
  versions = { "1.21" = ...; "1.22" = ...; };
  default = "1.22";
};

# Custom registry adds Go 1.23, a patched 1.22, and changes default:
overlays.default = turnkey.lib.mkRegistryOverlay (final: prev: {
  go = {
    versions = {
      "1.23" = final.go_1_23;
      # Override nixpkgs package with a patch
      "1.22-patched" = prev.go_1_22.overrideAttrs (old: {
        patches = old.patches or [] ++ [ ./my-fix.patch ];
      });
    };
    default = "1.23";
  };

  # Add a new toolchain
  zig = {
    versions = { "0.11" = final.zig; };
    default = "0.11";
  };
});

# Result after composition:
go = {
  versions = { "1.21" = ...; "1.22" = ...; "1.22-patched" = ...; "1.23" = ...; };  # Merged!
  default = "1.23";  # Overridden
};
zig = {
  versions = { "0.11" = ...; };
  default = "0.11";
};
```

### Constraints

1. **`versions`**: Attribute set mapping version strings to Nix derivations
2. **`default`**: String that MUST be a key in `versions`
3. **Version strings**: Freeform, but SHOULD follow the upstream versioning scheme
   - Go: `"1.21"`, `"1.22"`, `"1.23"`
   - Python: `"3.11"`, `"3.12"`, `"3.13"`
   - Node.js: `"18"`, `"20"`, `"22"` (major only, following LTS convention)
   - Rust: `"1.75"`, `"1.76"`, `"1.77"`

---

## toolchain.toml Syntax

### Simple (use default version)

```toml
[toolchains]
go = {}
python = {}
```

### Explicit version

```toml
[toolchains]
go = { version = "1.22" }
python = { version = "3.11" }
nodejs = { version = "20" }
```

### Mixed

```toml
[toolchains]
go = { version = "1.22" }   # Pinned to 1.22
python = {}                  # Use registry default
rust = { version = "1.75" }  # Pinned to 1.75
```

---

## Resolution Algorithm

When Turnkey processes `toolchain.toml`:

```
for each toolchain in toolchain.toml:
    1. Look up toolchain name in registry.packages.${system}
    2. If not found: ERROR "Unknown toolchain: <name>"

    3. If version specified in toolchain.toml:
        a. Look up version in registry.packages.${system}.<name>.versions
        b. If not found: ERROR "Unknown version '<version>' for toolchain '<name>'. Available: <list>"
        c. Use that derivation

    4. If no version specified:
        a. Use registry.packages.${system}.<name>.default to get version string
        b. Look up that version in .versions
        c. Use that derivation
```

---

## Turnkey Integration

### flake.nix Configuration

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
    turnkey-registry.url = "github:firefly-engineering/turnkey-registry";
    rust-overlay.url = "github:oxalica/rust-overlay";
    # Optional: custom registry
    # my-registry.url = "github:myorg/my-toolchain-registry";
  };

  outputs = { nixpkgs, turnkey, turnkey-registry, rust-overlay, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ turnkey.flakeModules.turnkey ];

      perSystem = { system, ... }:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [
              rust-overlay.overlays.default           # Provides rust-bin
              turnkey-registry.overlays.default       # Official registry
              # my-registry.overlays.default          # Custom additions (versions merge!)
            ];
          };
        in {
          turnkey = {
            enable = true;
            declarationFile = ./toolchain.toml;
            # Registry comes from pkgs.turnkeyRegistry (added by overlays)
          };

          # pkgs is available with turnkeyRegistry
          _module.args.pkgs = pkgs;
        };
    };
}
```

### How Turnkey Resolves Toolchains

1. Turnkey reads `toolchain.toml`
2. For each toolchain, looks up `pkgs.turnkeyRegistry.<name>`
3. Resolves version (explicit or default)
4. Returns the derivation

Turnkey provides `lib.resolveTool` and `lib.resolveToolchains` for this.

**Note:** Library functions require `pkgs` and are exported per-system:
`turnkey.lib.${system}.resolveTool`, not `turnkey.lib.resolveTool`.

```nix
# Resolve a single tool — turnkey.lib.${system}.resolveTool
resolveTool = registry: name: spec:
  let
    entry = registry.${name} or (throw "Unknown toolchain: ${name}");
    version = spec.version or entry.default;
    availableVersions = builtins.attrNames entry.versions;
    versionEntry = entry.versions.${version}
      or (throw ''
        Version '${version}' of toolchain '${name}' is not available.

        Available versions for '${name}':
          ${builtins.concatStringsSep "\n    " (
            map (v: if v == entry.default then "- ${v} (default)" else "- ${v}") availableVersions
          )}
      '');
  in warnIfNeeded name version versionEntry;

# Resolve all toolchains from a toolchain.toml declaration — returns a list of packages
resolveToolchains = registry: declaration: ...;

# Usage:
go = turnkey.lib.${system}.resolveTool pkgs.turnkeyRegistry "go" { version = "1.22"; };
packages = turnkey.lib.${system}.resolveToolchains pkgs.turnkeyRegistry toolchainDeclaration;
```

Both functions support extended version entries with deprecation/EOL metadata
(see Open Questions §2 below) and emit warnings via `lib.warn`.

### Backward Compatibility

The current flat registry format:
```nix
{ go = pkgs.go; python = pkgs.python3; }
```

Is auto-wrapped to versioned format internally by the flake-parts module via `normalizeRegistry`. This function detects whether an entry already has `versions`/`default` attributes and converts flat entries (plain derivations) to versioned format:

```nix
# Internal to nix/flake-parts/turnkey/default.nix — not exported as a library function
normalizeEntry = entry:
  if entry ? versions && entry ? default then entry  # Already versioned
  else { versions = { "default" = entry; }; default = "default"; };  # Flat → versioned

normalizeRegistry = reg: builtins.mapAttrs (_name: normalizeEntry) reg;
```

This allows consumers to pass either flat or versioned registries — the module handles both transparently.

---

## Open Questions

### 1. Version Aliases

**Resolved:** Not needed as a separate construct.

Version strings are freeform, so aliases can simply be defined as version entries:

```nix
go = {
  versions = {
    "1.21" = final.go_1_21;
    "1.22" = final.go_1_22;
    "1.23" = final.go_1_23;
    "latest" = final.go_1_23;  # Alias is just another entry
    "lts" = final.go_1_22;
  };
  default = "1.23";
};
```

This keeps the design simple while supporting the use case.

### 2. Metadata (Deprecation, EOL)

**Implemented:** Version entries can include deprecation and EOL metadata.

Version entries support two formats:
1. **Plain derivation** (existing): `"1.23" = final.go_1_23;`
2. **Extended with metadata** (new):
   ```nix
   "1.22" = {
     package = final.go_1_22;
     deprecated = true;
     deprecationMessage = "Use 1.23 instead";
     eol = "2025-02-01";
   };
   ```

**Metadata Fields:**
- `package` (required for extended format): The actual derivation
- `deprecated` (bool, optional): Mark version as deprecated
- `deprecationMessage` (string, optional): Migration guidance shown in warning
- `eol` (string, optional): End-of-life date in ISO 8601 format (YYYY-MM-DD)

**Behavior:**
- Warnings emitted via `lib.warn` during Nix evaluation when:
  - `deprecated = true` is set
  - EOL date has passed (compared against current date)
- Warnings include toolchain name, version, and any migration guidance
- Set `TURNKEY_NO_DEPRECATION_WARNINGS=1` to suppress all deprecation warnings

**Example with warnings:**
```
warning: DEPRECATED: Toolchain 'go' version '1.21' is deprecated.
  Use 1.22 or later instead
warning: EOL: Toolchain 'python' version '3.9' reached end-of-life on 2025-10-01.
```

**Backward Compatibility:**
Plain derivation entries continue to work unchanged. Detection is based on the presence of the `package` attribute.

See implementation: `nix/lib/default.nix`

### 3. Toolchain Groups (Meta-Packages)

Some toolchains are bundles (rust = rustc + cargo + clippy + rustfmt). These should be handled as **meta-packages** - single entries that combine multiple related tools.

**Benefits:**
- Fewer entries in `toolchain.toml` (`rust = {}` instead of 5 entries)
- Enforced version consistency across components
- Simpler mental model ("rust 1.91" is one thing)

**Implementation:** Turnkey provides `mkMetaPackage` helper:

```nix
mkMetaPackage = { name, components }:
  pkgs.symlinkJoin {
    inherit name;
    paths = builtins.attrValues components;
    passthru = {
      inherit components;
    } // components;  # Allows introspection via e.g. rust.rustc
  };
```

**Usage in registry:**

```nix
rust = {
  versions = {
    "1.91" = turnkey.lib.mkMetaPackage {
      name = "rust-1.91";
      components = {
        rustc = final.rust-bin.stable."1.91.0".minimal;
        cargo = final.rust-bin.stable."1.91.0".minimal;
        clippy = final.rust-bin.stable."1.91.0".clippy;
        rustfmt = final.rust-bin.stable."1.91.0".rustfmt;
        rust-analyzer = final.rust-analyzer;
      };
    };
  };
  default = "1.91";
};

go = {
  versions = {
    "1.23" = turnkey.lib.mkMetaPackage {
      name = "go-1.23";
      components = {
        go = final.go_1_23;
        gopls = final.gopls;
        golangci-lint = final.golangci-lint;
      };
    };
  };
  default = "1.23";
};
```

**Result:**
- Single derivation with all binaries in `$out/bin`
- All tools in PATH when package is added to environment
- Components accessible via `rust.components.rustc` or `rust.rustc` for introspection
- Buck2 system toolchains work normally (tools found in PATH)

### 4. Registry Composition

**Resolved:** Using `mkRegistryOverlay` helper with two-level merging.

```nix
overlays = [
  rust-overlay.overlays.default      # Provides rust-bin
  turnkey-registry.overlays.default  # Uses rust-bin, adds turnkeyRegistry
  my-registry.overlays.default       # Versions merge, defaults override
];
```

The helper handles merging automatically - registry authors just define their packages.

### 5. Per-System Version Availability

**Resolved:** Registry providers are responsible for per-system availability.

Not all versions may be available on all systems (e.g., old Go on ARM). Registries should only include versions they can provide for each system.

When a requested version doesn't exist, Turnkey MUST provide a clear error:

```
Error: Version '1.19' of toolchain 'go' is not available for system 'aarch64-darwin'.

Available versions for 'go' on this system:
  - 1.21
  - 1.22
  - 1.23 (default)

Hint: Check if this version is supported on your platform, or use a different version.
```

---

## Migration Path

### Phase 1: Spec Finalization
- Finalize this specification
- Get feedback on interface design

### Phase 2: Registry Implementation
- Create `turnkey-registry` flake with versioned packages
- Cover Go, Python, Rust, Node.js initially

### Phase 3: Turnkey Integration
- Update Turnkey to consume versioned registries
- Add backward compatibility wrapper for flat registries
- Update toolchain.toml parsing for `version` attribute

### Phase 4: Documentation
- Update user manual with versioned examples
- Document how to create custom registries

---

## Example: Full Registry Flake

See appendix for a complete example registry flake implementation.

---

## Appendix: Complete Registry Example

```nix
# flake.nix for turnkey-registry
{
  description = "Official Turnkey toolchain registry with versioned packages";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
    # Note: rust-overlay is NOT an input here - consumers compose it themselves
  };

  outputs = { self, nixpkgs, turnkey }: {
    # The registry uses mkRegistryOverlay for safe composition
    overlays.default = turnkey.lib.mkRegistryOverlay (final: prev: {
      # =====================================================================
      # Go
      # =====================================================================
      go = {
        versions = {
          "1.21" = final.go_1_21;
          "1.22" = final.go_1_22;
          "1.23" = final.go_1_23;
        };
        default = "1.23";
      };

      # =====================================================================
      # Python
      # =====================================================================
      python = {
        versions = {
          "3.10" = final.python310;
          "3.11" = final.python311;
          "3.12" = final.python312;
          "3.13" = final.python313;
        };
        default = "3.12";
      };

      # =====================================================================
      # Rust (meta-package: rustc + cargo + clippy + rustfmt + rust-analyzer)
      # Uses final.rust-bin if rust-overlay is composed before this overlay.
      # =====================================================================
      rust =
        let
          mkRustMeta = version: turnkey.lib.mkMetaPackage {
            name = "rust-${version}";
            components = {
              rustc = final.rust-bin.stable."${version}.0".minimal;
              cargo = final.rust-bin.stable."${version}.0".minimal;
              clippy = final.rust-bin.stable."${version}.0".clippy;
              rustfmt = final.rust-bin.stable."${version}.0".rustfmt;
              rust-analyzer = final.rust-analyzer;
            };
          };
        in {
          versions =
            if final ? rust-bin then {
              "1.75" = mkRustMeta "1.75";
              "1.76" = mkRustMeta "1.76";
              "1.77" = mkRustMeta "1.77";
              "1.78" = mkRustMeta "1.78";
              "1.79" = mkRustMeta "1.79";
              "1.80" = mkRustMeta "1.80";
            } else {
              # Fallback to nixpkgs (single version, no meta-package)
              "nixpkgs" = final.rustc;
            };
          default = if final ? rust-bin then "1.80" else "nixpkgs";
        };

      # =====================================================================
      # Node.js
      # =====================================================================
      nodejs = {
        versions = {
          "18" = final.nodejs_18;
          "20" = final.nodejs_20;
          "22" = final.nodejs_22;
        };
        default = "20";  # Current LTS
      };

      # =====================================================================
      # TypeScript
      # =====================================================================
      typescript = {
        versions = {
          "5" = final.nodePackages.typescript;
        };
        default = "5";
      };

      # =====================================================================
      # Build tools (typically single "latest" version)
      # =====================================================================
      buck2 = {
        versions."latest" = final.buck2;
        default = "latest";
      };

      biome = {
        versions."latest" = final.biome;
        default = "latest";
      };

      # =====================================================================
      # Solidity
      # =====================================================================
      solc = {
        versions."latest" = final.solc;
        default = "latest";
      };

      foundry = {
        versions."latest" = final.foundry;
        default = "latest";
      };

      # =====================================================================
      # Data templating
      # =====================================================================
      jsonnet = {
        versions."latest" = final.go-jsonnet or final.jsonnet;
        default = "latest";
      };
    });
  };
}
```

### Turnkey's Library Functions

Turnkey provides these helpers in `turnkey.lib.${system}` (per-system, since they require `pkgs`).

See implementation: `nix/lib/default.nix`

```nix
# nix/lib/default.nix
{ lib, pkgs, currentTime ? null }:
let
  # Deprecation/EOL support (internal helpers)
  suppressWarnings = builtins.getEnv "TURNKEY_NO_DEPRECATION_WARNINGS" != "";
  extractPackage = versionEntry:
    if versionEntry ? package then versionEntry.package else versionEntry;
  checkDeprecation = name: version: versionEntry: /* ... warns for deprecated/EOL entries ... */;
  warnIfNeeded = name: version: versionEntry:
    let pkg = extractPackage versionEntry;
        warning = checkDeprecation name version versionEntry;
    in if warning == null then pkg else lib.warn warning pkg;
in
{
  # Create a registry overlay with two-level merging
  # packagesFn receives both final and prev for full overlay power
  mkRegistryOverlay = packagesFn: final: prev:
    let
      prevRegistry = prev.turnkeyRegistry or { };
      newPackages = packagesFn final prev;

      mergeToolchain = name: new:
        let existing = prevRegistry.${name} or null;
        in if existing == null then new
           else {
             versions = (existing.versions or { }) // (new.versions or { });
             default = if new ? default then new.default else existing.default;
           };
    in {
      turnkeyRegistry = prevRegistry // (builtins.mapAttrs mergeToolchain newPackages);
    };

  # Create a meta-package combining multiple components
  # All component binaries end up in $out/bin, available in PATH
  mkMetaPackage = { name, components }:
    pkgs.symlinkJoin {
      inherit name;
      paths = builtins.attrValues components;
      passthru = { inherit components; } // components;
    };

  # Resolve a single toolchain from registry
  # Supports extended version entries with deprecation/EOL metadata
  resolveTool = registry: name: spec:
    let
      entry = registry.${name} or (throw "Unknown toolchain: ${name}");
      version = spec.version or entry.default;
      availableVersions = builtins.attrNames entry.versions;
      versionEntry = entry.versions.${version}
        or (throw ''
          Version '${version}' of toolchain '${name}' is not available.

          Available versions for '${name}':
            ${builtins.concatStringsSep "\n    " (
              map (v: if v == entry.default then "- ${v} (default)" else "- ${v}") availableVersions
            )}
        '');
    in warnIfNeeded name version versionEntry;

  # Resolve all toolchains from a toolchain.toml declaration
  # Returns a list of packages
  resolveToolchains = registry: declaration:
    let
      toolchains = declaration.toolchains or { };
      resolveOne = name: spec:
        let
          entry = registry.${name} or (throw "Unknown toolchain '${name}' in toolchain.toml");
          version = spec.version or entry.default;
          availableVersions = builtins.attrNames entry.versions;
          versionEntry = entry.versions.${version}
            or (throw ''
              Version '${version}' of toolchain '${name}' is not available.

              Available versions for '${name}':
                ${builtins.concatStringsSep "\n    " (
                  map (v: if v == entry.default then "- ${v} (default)" else "- ${v}") availableVersions
                )}

              Requested in: toolchain.toml
            '');
        in warnIfNeeded name version versionEntry;
    in lib.mapAttrsToList resolveOne toolchains;
}
```

**Note:** Legacy registry wrapping is handled internally by the flake-parts module
(`normalizeRegistry` in `nix/flake-parts/turnkey/default.nix`), not exported as a library function.

### Usage Example

```nix
# Consumer's flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
    turnkey-registry.url = "github:firefly-engineering/turnkey-registry";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { nixpkgs, turnkey, turnkey-registry, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          rust-overlay.overlays.default      # Add rust-bin first
          turnkey-registry.overlays.default  # Registry can use rust-bin
        ];
      };

      # Library functions are per-system (they require pkgs)
      tkLib = turnkey.lib.${system};

      # Resolve Go 1.22
      go = tkLib.resolveTool pkgs.turnkeyRegistry "go" { version = "1.22"; };

      # Resolve Rust (uses default from registry)
      rust = tkLib.resolveTool pkgs.turnkeyRegistry "rust" {};

      # Resolve all toolchains from toolchain.toml at once
      allTools = tkLib.resolveToolchains pkgs.turnkeyRegistry
        (builtins.fromTOML (builtins.readFile ./toolchain.toml));
    in {
      # ...
    };
}
```

### Custom Registry Example

```nix
# my-org-registry/flake.nix
{
  description = "My organization's custom toolchain registry";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = { turnkey, ... }: {
    # Adds to/extends any previously composed registry
    overlays.default = turnkey.lib.mkRegistryOverlay (final: prev: {
      # Add Go 1.24 and make it the new default
      go = {
        versions = { "1.24" = final.go_1_24; };
        default = "1.24";
      };

      # Add a patched version of Python
      python = {
        versions = {
          "3.12-patched" = prev.python312.overrideAttrs (old: {
            patches = old.patches or [] ++ [ ./python-fix.patch ];
          });
        };
        # Don't set default - keep the one from official registry
      };

      # Add a toolchain not in the official registry
      zig = {
        versions = {
          "0.11" = final.zig_0_11;
          "0.12" = final.zig_0_12;
        };
        default = "0.12";
      };

      # Add internal tools
      my-internal-tool = {
        versions = { "1.0" = final.callPackage ./my-tool.nix {}; };
        default = "1.0";
      };
    });
  };
}
```
