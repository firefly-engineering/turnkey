# Versioned Registry Specification

**Status:** Draft
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

A valid registry flake MUST expose an **overlay** that adds a `turnkeyRegistry` attribute to pkgs. This leverages Nix's native overlay composition.

### Core Structure

```nix
{
  overlays.default = final: prev: {
    turnkeyRegistry = {
      <toolchain-name> = {
        versions = {
          "<version-string>" = <derivation>;
          "<version-string>" = <derivation>;
          # ...
        };
        default = "<version-string>";  # REQUIRED: must match a key in versions
      };
      # ...
    };
  };
}
```

### Why Overlays?

1. **Native composition**: Overlays compose naturally with `lib.composeExtensions`
2. **Access to final pkgs**: Registries can reference packages added by other overlays
3. **Familiar pattern**: Standard Nix idiom that users already understand
4. **Lazy evaluation**: Only requested toolchains are evaluated

### Example Registry Flake

```nix
{
  description = "Turnkey official toolchain registry";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }: {
    overlays.default = final: prev: {
      turnkeyRegistry = {
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
            "1.75" = final.rustc;  # or from rust-overlay via final
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
      };
    };
  };
}
```

### Composing Registries

Multiple registries compose via overlay composition:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey-registry.url = "github:firefly-engineering/turnkey-registry";
    rust-overlay.url = "github:oxalica/rust-overlay";
    my-registry.url = "github:myorg/my-toolchain-registry";
  };

  outputs = { nixpkgs, turnkey-registry, rust-overlay, my-registry, ... }:
    let
      # Compose overlays - later overlays can override/extend earlier ones
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [
          rust-overlay.overlays.default      # Provides rust-bin
          turnkey-registry.overlays.default  # Official registry (uses rust-bin)
          my-registry.overlays.default       # Custom additions/overrides
        ];
      };
    in {
      # pkgs.turnkeyRegistry now has all toolchains from all registries
    };
}
```

### Extending vs Overriding

A custom registry can **extend** the official one:

```nix
# my-registry/flake.nix
overlays.default = final: prev: {
  turnkeyRegistry = prev.turnkeyRegistry // {
    # Add a new toolchain
    zig = {
      versions = {
        "0.11" = final.zig_0_11;
        "0.12" = final.zig_0_12;
      };
      default = "0.12";
    };

    # Override Go with additional versions
    go = prev.turnkeyRegistry.go // {
      versions = prev.turnkeyRegistry.go.versions // {
        "1.24" = final.go_1_24;  # Add new version
      };
    };
  };
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
              rust-overlay.overlays.default
              turnkey-registry.overlays.default
              # my-registry.overlays.default  # Custom additions
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

```nix
# Inside Turnkey's resolution logic
resolveTool = name: spec:
  let
    entry = pkgs.turnkeyRegistry.${name}
      or (throw "Unknown toolchain: ${name}");
    version = spec.version or entry.default;
    pkg = entry.versions.${version}
      or (throw "Unknown version '${version}' for ${name}. Available: ${toString (builtins.attrNames entry.versions)}");
  in pkg;
```

### Backward Compatibility

The current flat registry format:
```nix
{ go = pkgs.go; python = pkgs.python3; }
```

Can be auto-wrapped to the versioned format via a compatibility layer:

```nix
# Turnkey can wrap legacy registries
wrapLegacyRegistry = legacy:
  builtins.mapAttrs (name: pkg: {
    versions = { "default" = pkg; };
    default = "default";
  }) legacy;
```

This allows gradual migration from flat registries to versioned ones.

---

## Open Questions

### 1. Version Aliases

Should we support aliases like `"latest"` or `"lts"`?

```nix
go = {
  versions = {
    "1.21" = pkgs.go_1_21;
    "1.22" = pkgs.go_1_22;
    "1.23" = pkgs.go_1_23;
  };
  aliases = {
    "latest" = "1.23";
    "previous" = "1.22";
  };
  default = "1.23";
};
```

**Recommendation:** Start without aliases. Add if needed later.

### 2. Metadata

Should versions carry metadata beyond the derivation?

```nix
versions = {
  "1.22" = {
    package = pkgs.go_1_22;
    deprecated = false;
    eol = "2025-02-01";  # End of life date
  };
};
```

**Recommendation:** Start with just derivations. Metadata can be added later without breaking changes.

### 3. Toolchain Groups

Some toolchains are bundles (rust = rustc + cargo + clippy). How to handle?

**Option A:** Single entry with meta-package
```nix
rust = {
  versions = {
    "1.77" = pkgs.rust-bin.stable."1.77.0".default;  # From rust-overlay
  };
};
```

**Option B:** Separate entries that must match
```nix
rustc = { versions = { "1.77" = ...; }; };
cargo = { versions = { "1.77" = ...; }; };
```

**Recommendation:** Option A - treat toolchain groups as single versioned units.

### 4. Registry Composition

**Resolved:** Using overlays gives composition for free.

```nix
overlays = [
  rust-overlay.overlays.default      # Provides rust-bin
  turnkey-registry.overlays.default  # Uses rust-bin, adds turnkeyRegistry
  my-registry.overlays.default       # Extends/overrides turnkeyRegistry
];
```

Custom registries use `prev.turnkeyRegistry // { ... }` to extend rather than replace.

### 5. Per-System Version Availability

Not all versions may be available on all systems (e.g., old Go on ARM).

**Recommendation:** Registries should only include versions they can provide. Turnkey should error clearly if a requested version isn't available for the current system.

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
    # Note: rust-overlay is NOT an input here - consumers compose it themselves
  };

  outputs = { self, nixpkgs }: {
    # The registry is an overlay that adds turnkeyRegistry to pkgs
    overlays.default = final: prev: {
      turnkeyRegistry = {
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
        # Rust
        # Uses final.rust-bin if rust-overlay is composed before this overlay.
        # Falls back to nixpkgs rustc if not.
        # =====================================================================
        rust = {
          versions =
            if final ? rust-bin then {
              # rust-overlay provides precise versions
              "1.75" = final.rust-bin.stable."1.75.0".default;
              "1.76" = final.rust-bin.stable."1.76.0".default;
              "1.77" = final.rust-bin.stable."1.77.0".default;
              "1.78" = final.rust-bin.stable."1.78.0".default;
              "1.79" = final.rust-bin.stable."1.79.0".default;
              "1.80" = final.rust-bin.stable."1.80.0".default;
            } else {
              # Fallback to nixpkgs (single version)
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
      };
    };

    # Also provide a lib for consumers to use
    lib = {
      # Helper to resolve a toolchain from registry
      resolveTool = registry: name: spec:
        let
          entry = registry.${name}
            or (throw "Unknown toolchain: ${name}");
          version = spec.version or entry.default;
          pkg = entry.versions.${version}
            or (throw "Unknown version '${version}' for ${name}. Available: ${toString (builtins.attrNames entry.versions)}");
        in pkg;

      # Wrap a legacy flat registry to versioned format
      wrapLegacyRegistry = legacy:
        builtins.mapAttrs (name: pkg: {
          versions = { "default" = pkg; };
          default = "default";
        }) legacy;
    };
  };
}
```

### Usage Example

```nix
# Consumer's flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey-registry.url = "github:firefly-engineering/turnkey-registry";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { nixpkgs, turnkey-registry, rust-overlay, ... }:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [
          rust-overlay.overlays.default      # Add rust-bin first
          turnkey-registry.overlays.default  # Registry can use rust-bin
        ];
      };

      # Resolve Go 1.22
      go = turnkey-registry.lib.resolveTool pkgs.turnkeyRegistry "go" { version = "1.22"; };

      # Resolve Rust (uses default from registry)
      rust = turnkey-registry.lib.resolveTool pkgs.turnkeyRegistry "rust" {};
    in {
      # ...
    };
}
```
