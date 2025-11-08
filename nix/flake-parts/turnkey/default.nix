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

        declarationFiles = mkOption {
          type = types.attrsOf types.path;
          default = { };
          example = lib.literalExpression ''
            {
              default = ./toolchain.toml;
              ci = ./toolchain.ci.toml;
            }
          '';
          description = ''
            Attribute set mapping shell names to toolchain declaration files.
            Each file will create a corresponding devenv shell.
            The shell name "default" maps to the default shell.
          '';
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

      # Create a shell configuration for each declaration file
      mkShellConfig = shellName: declarationFile: {
        imports = [ ../../devenv/turnkey ];

        turnkey = {
          registry = lib.mkDefault registry;
          declarationFile = declarationFile;
        };
      };

      # Generate shell configurations from declarationFiles
      shellConfigs = lib.mapAttrs mkShellConfig cfg.declarationFiles;

    in
    lib.mkIf cfg.enable {
      # Create all shells from declaration files
      devenv.shells = shellConfigs;
    };
}
