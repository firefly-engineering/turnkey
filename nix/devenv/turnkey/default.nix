{ lib, config, pkgs, ... }:

let
  cfg = config.turnkey;

  # Generate the direnv library script
  direnvLib = import ./direnv-lib.nix { inherit lib pkgs config; };

  # Teller lib for registry resolution (injected via flake-parts module)
  turnkeyLib = cfg.tellerLib;
in
{
  # Import the Buck2 generation sub-module
  imports = [ ./buck2.nix ];
  options.turnkey = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable turnkey toolchain management for this shell";
    };

    declarationFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to toolchain.toml declaration file for this shell";
    };

    registry = lib.mkOption {
      type = lib.types.lazyAttrsOf lib.types.anything;
      default = { };
      description = ''
        Versioned registry mapping toolchain names to version sets.
        Each entry has the structure: { versions = { "<ver>" = <pkg>; }; default = "<ver>"; }
      '';
    };

    tellerLib = lib.mkOption {
      type = lib.types.anything;
      internal = true;
      description = "Teller library (injected by flake-parts module).";
    };
  };

  config = lib.mkIf (cfg.enable && cfg.declarationFile != null) {
    packages =
      let
        # Parse toolchain.toml
        toolchainDeclaration = builtins.fromTOML (builtins.readFile cfg.declarationFile);

        # Resolve all toolchains using the versioned registry
        resolvedPackages = turnkeyLib.resolveToolchains cfg.registry toolchainDeclaration;
      in
      resolvedPackages;

    # Export direnv library path
    env.TURNKEY_DIRENV_LIB = "${direnvLib}";

    # Redirect Python bytecode cache to .turnkey to keep source tree clean
    # Must be set in enterShell with $PWD since env vars are set at build time
    enterShell = ''
      export PYTHONPYCACHEPREFIX="$PWD/.turnkey/pycache"
    '';
  };
}
