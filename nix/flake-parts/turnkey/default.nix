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

        buck2 = {
          enable = mkOption {
            type = types.bool;
            default = false;
            description = "Enable Buck2 toolchain generation from toolchain.toml";
          };

          prelude = {
            strategy = mkOption {
              type = types.enum [ "bundled" "git" "nix" "path" ];
              default = "bundled";
              description = ''
                How to provide the Buck2 prelude cell:
                - bundled: Use Buck2's built-in bundled prelude
                - git: Use a git external cell
                - nix: Use a Nix derivation
                - path: Use an explicit filesystem path
              '';
            };

            path = mkOption {
              type = types.either types.path types.str;
              default = "bundled://";
              description = "Path to the prelude cell (use 'bundled://' for Buck2's built-in prelude)";
            };
          };

          godeps = mkOption {
            type = types.nullOr types.package;
            default = null;
            description = ''
              Nix derivation containing the Go dependencies cell.
              When set, a 'godeps' cell will be added to .buckconfig
              and symlinked to .turnkey/godeps.
            '';
          };
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

          # Pass through Buck2 configuration
          buck2 = {
            enable = cfg.buck2.enable;
            prelude = {
              strategy = cfg.buck2.prelude.strategy;
              path = cfg.buck2.prelude.path;
            };
            godeps = cfg.buck2.godeps;
          };
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
