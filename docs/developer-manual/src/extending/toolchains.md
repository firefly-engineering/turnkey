# Adding Toolchains

This guide covers adding new toolchains to Turnkey.

## Steps

1. Add package to registry (versioned format)
2. Add mapping to mappings.nix
3. (Optional) Create prelude extension

## 1. Add to Registry

Edit `nix/registry/default.nix`:

```nix
let
  single = pkg: { versions = { "default" = pkg; }; default = "default"; };
in {
  # Existing entries...

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
}
```

### Versioned Format

Each registry entry must have:

```nix
<name> = {
  versions = { "<version>" = <derivation>; ... };
  default = "<version>";  # Must match a key in versions
};
```

The `single` helper is convenient for tools with only one version.

## 2. Add Toolchain Mapping

Edit `nix/buck2/mappings.nix`:

### For Standard Toolchains

```nix
zig = {
  skip = false;
  targets = [{
    name = "zig";
    rule = "system_zig_toolchain";
    load = "@prelude//zig:toolchain.bzl";
    visibility = [ "PUBLIC" ];
  }];
  implicitDependencies = [ ];
};
```

### For Non-Toolchain Tools

Some tools don't need Buck2 rules:

```nix
mydevtool = {
  skip = true;
  reason = "Development utility, not a Buck2 toolchain";
};
```

## 3. Dynamic Attributes

For toolchains needing Nix store paths, use `dynamicAttrs`. The function receives a **resolved** registry where entries are already derivations:

```nix
mylang = {
  targets = [{
    name = "mylang";
    rule = "system_mylang_toolchain";
    load = "@prelude//mylang:toolchain.bzl";
    # registry entries are already resolved to derivations
    dynamicAttrs = registry: {
      compiler = "${registry.mylang}/bin/mycompiler";
    };
  }];
};
```

## 4. Prelude Extension (if needed)

If the upstream prelude doesn't have rules for your language, create a prelude extension. See [Prelude Extensions](./prelude-extensions.md).

## Testing

1. Add toolchain to `toolchain.toml`
2. Stage files: `git add nix/`
3. Enter shell: `nix develop`
4. Verify: `tk targets toolchains//...`

## Creating Custom Registries

For reusable toolchain collections, create a registry flake:

```nix
# my-registry/flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = { nixpkgs, turnkey, ... }:
    let
      forAllSystems = f: nixpkgs.lib.genAttrs
        [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ]
        (system: f system);
    in {
      overlays.default = forAllSystems (system:
        turnkey.lib.${system}.mkRegistryOverlay (final: prev: {
          # Add toolchains
          zig = {
            versions = {
              "0.11" = final.zig_0_11;
              "0.12" = final.zig_0_12;
            };
            default = "0.12";
          };

          # Extend existing toolchain with more versions
          go = {
            versions = { "1.24" = final.go_1_24; };
            default = "1.24";
          };
        })
      );
    };
}
```

Consumers compose the overlay:

```nix
pkgs = import nixpkgs {
  overlays = [
    my-registry.overlays.default.${system}
  ];
};
```

When multiple registries are composed:
- **Versions merge additively** - all versions from all registries are available
- **Default is overridden** - later overlays set the default version
