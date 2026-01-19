# Registry Pattern

The registry maps toolchain names to Nix packages.

## Structure

Located at `nix/registry/default.nix`:

```nix
{ pkgs, lib ? pkgs.lib }:

{
  buck2 = pkgs.buck2;
  go = pkgs.go;
  rust = pkgs.rustc;
  python = pkgs.python3;
  # ...
}
```

## Design Principles

1. **Simple attribute set** - Just names to packages
2. **Lazy evaluation** - Only builds what's used
3. **User-overridable** - Can be replaced entirely

## How It's Used

In the devenv module:

```nix
toolchainNames = builtins.attrNames toolchainDeclaration.toolchains;
resolvedPackages = map (name: cfg.registry.${name}) toolchainNames;
```

## Extending the Registry

Users can provide a custom registry in `flake.nix`:

```nix
turnkey.toolchains.registry = {
  # Override defaults
  go = pkgs.go_1_22;
  # Add custom tools
  mytool = myCustomPackage;
};
```

## Default vs Custom

The flake-parts module handles merging:

```nix
defaultRegistry = import ../../registry { inherit pkgs lib; };
registry = if cfg.registry == { } then defaultRegistry else cfg.registry;
```

**Note:** Currently this is all-or-nothing replacement. A future enhancement (turnkey-ry4a) will add extension support.

## Adding to Default Registry

To add a new toolchain to the default registry:

1. Edit `nix/registry/default.nix`
2. Add the package mapping
3. Test with `nix eval .#packages.x86_64-linux`
