{ lib, config, ... }:

let
  cfg = config.turnkey;
in
{
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
  };
}
