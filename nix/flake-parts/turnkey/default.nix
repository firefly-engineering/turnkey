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

              Prefer using goDepsFile instead for declarative configuration.
            '';
          };

          goDepsFile = mkOption {
            type = types.nullOr types.path;
            default = null;
            example = lib.literalExpression "./.turnkey/go-deps.toml";
            description = ''
              Path to go-deps.toml file declaring Go dependencies.
              When set, turnkey will build the godeps cell automatically.

              Recommended: use ./.turnkey/go-deps.toml with the turnkey .envrc
              pattern, which auto-generates the file to the Nix store before
              flake evaluation. The .turnkey/ directory should be gitignored.
            '';
          };

          goModFile = mkOption {
            type = types.str;
            default = "go.mod";
            description = ''
              Relative path to go.mod file (for staleness checking and regeneration).
            '';
          };

          goSumFile = mkOption {
            type = types.str;
            default = "go.sum";
            description = ''
              Relative path to go.sum file (for staleness checking and regeneration).
            '';
          };

          autoRegenerate = mkOption {
            type = types.bool;
            default = false;
            description = ''
              Enable automatic go-deps.toml regeneration via pre-commit hook.
              When enabled, go-deps.toml will be regenerated when go.mod or go.sum
              are staged for commit.

              Requires godeps-gen in the toolchain registry.
            '';
          };

          generateOnShellEntry = mkOption {
            type = types.bool;
            default = true;
            description = ''
              Automatically generate/regenerate go-deps.toml when entering the shell
              if it's missing or stale (older than go.mod or go.sum).

              This enables a workflow where go-deps.toml doesn't need to be committed:
              1. First shell entry: go-deps.toml is generated (godeps cell skipped)
              2. Subsequent entries: Nix uses the generated file

              Requires godeps-gen to be available in PATH.
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
      defaultRegistry = import ../../registry { inherit pkgs lib; };
      registry = if cfg.registry == { } then defaultRegistry else cfg.registry;

      # Build godeps cell from goDepsFile if specified and exists
      # The file may not exist on first run (before .envrc generates it)
      godepsCell =
        if cfg.buck2.goDepsFile != null && builtins.pathExists cfg.buck2.goDepsFile then
          import ../../buck2/go-deps-cell.nix {
            inherit pkgs lib;
            depsFile = cfg.buck2.goDepsFile;
          }
        else
          cfg.buck2.godeps;

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
            godeps = godepsCell;
            # Pass through paths for staleness checking and regeneration
            # Extract filename from path for runtime staleness checking
            goDepsFile =
              if cfg.buck2.goDepsFile != null
              then builtins.baseNameOf cfg.buck2.goDepsFile
              else "go-deps.toml";
            goModFile = cfg.buck2.goModFile;
            goSumFile = cfg.buck2.goSumFile;
            autoRegenerate = cfg.buck2.autoRegenerate;
            generateOnShellEntry = cfg.buck2.generateOnShellEntry;
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
