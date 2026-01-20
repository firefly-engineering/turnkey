# Registry Pattern

The registry maps toolchain names to versioned Nix packages.

## Structure

Located at `nix/registry/default.nix`:

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

Turnkey provides helpers in `turnkey.lib.<system>`:

### resolveTool

Resolves a toolchain from the registry:

```nix
# Usage
go = turnkey.lib.x86_64-linux.resolveTool registry "go" {};           # Use default
go122 = turnkey.lib.x86_64-linux.resolveTool registry "go" { version = "1.22"; };
```

### resolveToolchains

Resolves all toolchains from a parsed toolchain.toml:

```nix
declaration = builtins.fromTOML (builtins.readFile ./toolchain.toml);
packages = turnkey.lib.x86_64-linux.resolveToolchains registry declaration;
```

### mkRegistryOverlay

Creates overlays with two-level merging for registry composition:

```nix
overlays.default = turnkey.lib.x86_64-linux.mkRegistryOverlay (final: prev: {
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
    "1.80" = turnkey.lib.x86_64-linux.mkMetaPackage {
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

## How It's Used

In the devenv module:

```nix
turnkeyLib = import ../../lib { inherit lib pkgs; };

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
  inputs.turnkey.url = "github:firefly-engineering/turnkey";

  outputs = { turnkey, ... }: {
    overlays.default = turnkey.lib.x86_64-linux.mkRegistryOverlay (final: prev: {
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

To add a new toolchain to the default registry:

1. Edit `nix/registry/default.nix`
2. Add the package mapping using the `single` helper (or multi-version format)
3. Test with `nix develop`

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
