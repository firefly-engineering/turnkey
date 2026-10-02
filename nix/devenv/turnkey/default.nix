{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.turnkey;

  # Generate the direnv library script
  direnvLib = import ./direnv-lib.nix { inherit lib pkgs config; };

  # Teller lib for registry resolution (injected via flake-parts module)
  turnkeyLib = cfg.tellerLib;
in
{
  # Import the Buck2 generation sub-module and its pre-commit hooks
  imports = [
    ./buck2.nix
    ./git-hooks.nix
  ];
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

    turnkeySources = lib.mkOption {
      type = lib.types.listOf (
        lib.types.submodule {
          options = {
            file = lib.mkOption {
              type = lib.types.str;
              description = "The source file, relative to the project root.";
            };
            hash = lib.mkOption {
              type = lib.types.str;
              description = "The SHA-256 of its content.";
            };
          };
        }
      );
      default = [ ];
      internal = true;
      description = ''
        turnkey's Nix sources the shell was built from, with their content
        hashes (deps-freshness.nix's sourceEntries). The flake-parts module
        sets them in turnkey's own repository only, for use_turnkey to
        re-evaluate the flake when one changed on disk. Empty in a consumer
        project, where turnkey's sources come from the flake input.
      '';
    };
  };

  config = lib.mkIf (cfg.enable && cfg.declarationFile != null) {
    packages = (import ../../lib/toolchain-declaration.nix { inherit lib; }).resolve {
      tellerLib = turnkeyLib;
      inherit (cfg) registry declarationFile;
    };

    # Export direnv library path
    env.TURNKEY_DIRENV_LIB = "${direnvLib}";

    # Redirect Python bytecode cache to .turnkey to keep source tree clean
    # Must be set in enterShell with $PWD since env vars are set at build time
    enterShell = ''
      export PYTHONPYCACHEPREFIX="$PWD/.turnkey/pycache"
    '';
  };
}
