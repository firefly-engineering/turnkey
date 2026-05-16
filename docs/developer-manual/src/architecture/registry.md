# Registry Pattern

The registry maps toolchain names to versioned Nix packages. The core registry library and default registry live in [teller](https://github.com/firefly-engineering/teller), a standalone Nix flake that turnkey depends on.

## Structure

The default registry is provided by teller (`registry/default.nix` in the teller repo):

```nix
{ pkgs, lib ? pkgs.lib }:

let
  # Helper for single-version entries
  single = pkg: {
    versions = { "default" = pkg; };
    default = "default";
  };
in {
  go = single pkgs.go;
  rust = single pkgs.rustc;
  python = single pkgs.python3;
  # ...
}
```

### Versioned Format

Each registry entry has:

```nix
<toolchain-name> = {
  versions = {
    "<version-string>" = <derivation>;
    # ...
  };
  default = "<version-string>";  # Must match a key in versions
};
```

For example, a multi-version entry:

```nix
go = {
  versions = {
    "1.21" = pkgs.go_1_21;
    "1.22" = pkgs.go_1_22;
    "1.23" = pkgs.go_1_23;
  };
  default = "1.23";
};
```

## Design Principles

1. **Versioned** - Each toolchain can have multiple versions
2. **Lazy evaluation** - Only builds what's used
3. **Composable** - Multiple registries can be merged via overlays
4. **User-overridable** - Versions and defaults can be customized

## Library Functions

Teller provides helpers in `teller.lib` (system-independent):

### resolveTool

Resolves a toolchain from the registry:

```nix
# Usage
go = teller.lib.resolveTool registry "go" {};           # Use default
go122 = teller.lib.resolveTool registry "go" { version = "1.22"; };
```

### resolveToolchains

Resolves all toolchains from a parsed toolchain.toml:

```nix
declaration = builtins.fromTOML (builtins.readFile ./toolchain.toml);
packages = teller.lib.resolveToolchains registry declaration;
```

### mkRegistryOverlay

Creates overlays with two-level merging for registry composition:

```nix
overlays.default = teller.lib.mkRegistryOverlay (final: prev: {
  go = {
    versions = { "1.24" = final.go_1_24; };
    default = "1.24";
  };
});
```

When composed, versions are merged additively and `default` is overridden.

### mkMetaPackage

Bundles multiple tools into a single derivation:

```nix
rust = {
  versions = {
    "1.80" = teller.lib.mkMetaPackage {
      inherit pkgs;
      name = "rust-1.80";
      components = {
        rustc = final.rustc;
        cargo = final.cargo;
        clippy = final.clippy;
        rustfmt = final.rustfmt;
      };
    };
  };
  default = "1.80";
};
```

## Default Sourcing

The `turnkey.toolchains` flake-parts module options `tellerLib` and `tellerRegistry` default to the teller + toolbox setup that turnkey bundles, so consumers don't have to wire them up unless they need a private registry or an alternate teller revision.

The defaults are also exposed as standalone helpers on turnkey's `lib` output, reachable from any downstream flake:

```nix
# inputs.turnkey.lib.defaultTellerLib       : teller.lib (system-agnostic)
# inputs.turnkey.lib.defaultTellerRegistry  : system → registry attrset
```

So a typical consumer flake reduces to:

```nix
turnkey.toolchains = {
  enable = true;
  declarationFiles.default = ./toolchain.toml;
  buck2 = { ... };
};
```

Override scenarios:

- **Private registry overlay** — extend the toolbox-backed default with extra overlays:

  ```nix
  turnkey.toolchains.tellerRegistry =
    (import inputs.nixpkgs {
      inherit system;
      overlays = [
        inputs.teller.overlays.default
        inputs.toolbox.overlays.default
        inputs.my-org-overlay.overlays.default
      ];
    }).turnkeyRegistry;
  ```

- **Alternate teller revision** — pin a fork or a specific teller commit:

  ```nix
  turnkey.toolchains.tellerLib = inputs.my-teller-fork.lib;
  ```

Or reach the defaults directly for ad-hoc composition outside the module:

```nix
let
  registry = inputs.turnkey.lib.defaultTellerRegistry system;
  resolvedBuck2 = inputs.turnkey.lib.defaultTellerLib.resolveTool registry "buck2" {};
in ...
```

## How It's Used

The flake-parts module injects `tellerLib` into the devenv module:

```nix
# In the devenv module:
turnkeyLib = cfg.tellerLib;

# Parse toolchain.toml and resolve all toolchains
declaration = builtins.fromTOML (builtins.readFile cfg.declarationFile);
packages = turnkeyLib.resolveToolchains cfg.registry declaration;
```

## Extending the Registry

### Via registryExtensions

In your `flake.nix`:

```nix
turnkey.toolchains = {
  registryExtensions = let
    single = pkg: { versions = { "default" = pkg; }; default = "default"; };
  in {
    mytool = single myCustomPackage;
    # Add versions to existing toolchain
    go = {
      versions = { "1.24" = pkgs.go_1_24; };
      default = "1.24";  # Override default
    };
  };
};
```

### Via Custom Registry Overlay

For reusable registries, create a flake that exports an overlay:

```nix
# my-registry/flake.nix
{
  inputs.teller.url = "github:firefly-engineering/teller";

  outputs = { teller, ... }: {
    overlays.default = teller.lib.mkRegistryOverlay (final: prev: {
      zig = {
        versions = {
          "0.11" = final.zig_0_11;
          "0.12" = final.zig_0_12;
        };
        default = "0.12";
      };
    });
  };
}
```

Consumers compose overlays:

```nix
pkgs = import nixpkgs {
  overlays = [
    official-registry.overlays.default
    my-registry.overlays.default  # Versions merge!
  ];
};
```

## Internal Tools

Dependency generators (`godeps-gen`, `rustdeps-gen`, etc.) are **not** in the registry. They're internal turnkey tools that are automatically included when the corresponding language is enabled.

## Adding to Default Registry

To add a new standard toolchain, contribute to [teller](https://github.com/firefly-engineering/teller)'s `registry/default.nix`.

To add a turnkey-specific tool, add it to `registryExtensions` in turnkey's `flake.nix`.

```nix
# Single version (most common)
zig = single pkgs.zig;

# Multiple versions
nodejs = {
  versions = {
    "18" = pkgs.nodejs_18;
    "20" = pkgs.nodejs_20;
    "22" = pkgs.nodejs_22;
  };
  default = "20";
};
```
