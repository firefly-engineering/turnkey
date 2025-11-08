{
  lib,
  flake-parts-lib,
  ...
}:

let
  inherit (flake-parts-lib) mkPerSystemOption;
  inherit (lib) mkOption types;
in
{
  options.perSystem = mkPerSystemOption (
    {
      config,
      pkgs,
      system,
      ...
    }:
    {
      options.turnkey.toolchains = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Enable turnkey toolchain management";
        };

        declarationFile = mkOption {
          type = types.nullOr types.path;
          default = null;
          description = "Path to toolchain.toml declaration file for the default shell (convenience option)";
        };

        registry = mkOption {
          type = types.lazyAttrsOf types.package;
          default = { };
          defaultText = "Default registry from nix/registry";
          description = "Default registry mapping toolchain names to packages (inherited by all shells)";
        };
      };
    }
  );

  config.perSystem =
    {
      config,
      pkgs,
      system,
      ...
    }:
    let
      cfg = config.turnkey.toolchains;

      # Load default registry if user didn't provide one
      defaultRegistry = import ../../registry { inherit pkgs; };
      registry = if cfg.registry == { } then defaultRegistry else cfg.registry;

    in
    lib.mkIf cfg.enable {
      # Configure the default shell with turnkey
      # Users can create additional shells and manually import the turnkey devenv module
      devenv.shells.default = {
        imports = [ ../../devenv/turnkey ];

        turnkey = {
          registry = lib.mkDefault registry;
          declarationFile = lib.mkIf (cfg.declarationFile != null) cfg.declarationFile;
        };
      };
    };
}
