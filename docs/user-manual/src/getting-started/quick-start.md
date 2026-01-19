# Quick Start

This guide walks you through building your first project with Turnkey.

## Create a toolchain.toml

Create a `toolchain.toml` file in your project root:

```toml
[toolchains]
buck2 = {}
go = {}
```

This declares that your project needs Buck2 and Go.

## Configure Your Flake

Update your `flake.nix` to use Turnkey:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    turnkey.url = "github:firefly-engineering/turnkey";
    devenv.url = "github:cachix/devenv";
  };

  outputs = inputs@{ turnkey, devenv, ... }:
    turnkey.lib.mkFlake { inherit inputs; } {
      imports = [
        devenv.flakeModule
        turnkey.flakeModules.turnkey
      ];

      perSystem = { ... }: {
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;
          buck2.enable = true;
        };
      };
    };
}
```

## Enter the Shell

```bash
nix develop
```

## Build Something

Create a simple Go program and build it with Buck2:

```bash
tk build //path/to:target
```

## Next Steps

- [Project Setup](./project-setup.md) - Detailed project configuration
- [toolchain.toml](../configuration/toolchain-toml.md) - Full configuration reference
