# Module System

Turnkey uses NixOS-style modules for configuration.

## Module Layers

### Flake-Parts Module

Located at `nix/flake-parts/turnkey/default.nix`.

Provides perSystem options:

```nix
options.perSystem = mkPerSystemOption ({...}: {
  options.turnkey.toolchains = {
    enable = mkOption { type = types.bool; default = true; };
    declarationFiles = mkOption { type = types.attrsOf types.path; };
    registry = mkOption { type = types.lazyAttrsOf types.package; };
    buck2 = {
      enable = mkOption { ... };
      go.depsFile = mkOption { ... };
      rust.depsFile = mkOption { ... };
      # ...
    };
  };
});
```

### Devenv Module

Located at `nix/devenv/turnkey/default.nix`.

Receives configuration from flake-parts:

```nix
options.turnkey = {
  enable = mkOption { type = types.bool; };
  declarationFile = mkOption { type = types.path; };
  registry = mkOption { type = types.lazyAttrsOf types.package; };
};

config = lib.mkIf cfg.enable {
  packages = resolvedPackages;
};
```

## Configuration Flow

1. User sets `turnkey.toolchains` in their flake
2. Flake-parts module creates shell configs
3. Each shell config imports devenv module
4. Devenv module resolves packages from registry

## Extending Options

Add new options in the appropriate module:

```nix
# In flake-parts module for user-facing API
options.turnkey.toolchains.myFeature = mkOption {
  type = types.bool;
  default = false;
  description = "Enable my feature";
};

# Pass to devenv module in mkShellConfig
turnkey.myFeature = cfg.myFeature;
```
