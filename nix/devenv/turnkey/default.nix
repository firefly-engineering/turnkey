{ lib, config, pkgs, ... }:

let
  cfg = config.turnkey;

  # Generate the direnv library script
  direnvLib = import ./direnv-lib.nix { inherit lib pkgs config; };
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
      type = lib.types.lazyAttrsOf lib.types.package;
      default = { };
      description = "Registry mapping toolchain names to packages (usually inherited from flake-parts)";
    };
  };

  config = lib.mkIf (cfg.enable && cfg.declarationFile != null) {
    packages =
      let
        # Parse toolchain.toml
        toolchainDeclaration = builtins.fromTOML (builtins.readFile cfg.declarationFile);

        # Extract toolchain names from the declaration
        toolchainNames =
          if toolchainDeclaration ? toolchains then
            builtins.attrNames toolchainDeclaration.toolchains
          else
            [ ];

        # Resolve toolchains to packages from registry
        resolvedPackages = map (name: cfg.registry.${name}) toolchainNames;
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
