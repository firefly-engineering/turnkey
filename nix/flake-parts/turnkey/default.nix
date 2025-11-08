{ self, lib, flake-parts-lib, ... }:

let
  inherit (flake-parts-lib) mkPerSystemOption;
  inherit (lib) mkOption types;
in
{
  options.perSystem = mkPerSystemOption ({ config, pkgs, system, ... }: {
    options.turnkey.toolchains = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Enable turnkey toolchain management";
      };

      declarationFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Path to toolchain.toml declaration file";
      };

      registry = mkOption {
        type = types.lazyAttrsOf types.package;
        default = {};
        defaultText = "Default registry from nix/registry";
        description = "Registry mapping toolchain names to packages";
      };
    };
  });

  config.perSystem = { config, pkgs, system, ... }:
    let
      cfg = config.turnkey.toolchains;

      # Load default registry if user didn't provide one
      defaultRegistry = import ../../registry { inherit pkgs; };
      registry = if cfg.registry == {} then defaultRegistry else cfg.registry;

      # Parse toolchain.toml if provided
      toolchainDeclaration =
        if cfg.declarationFile != null
        then builtins.fromTOML (builtins.readFile cfg.declarationFile)
        else {};

      # Extract toolchain names from the declaration
      toolchainNames =
        if toolchainDeclaration ? toolchains
        then builtins.attrNames toolchainDeclaration.toolchains
        else [];

      # Resolve toolchains to packages from registry
      resolvedPackages = map (name: registry.${name}) toolchainNames;

    in lib.mkIf cfg.enable {
      # Add resolved packages to devenv
      devenv.shells.default.packages = resolvedPackages;
    };
}
