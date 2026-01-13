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

          welcomeMessage = mkOption {
            type = types.nullOr types.str;
            default = null;
            example = "Welcome to MyProject turnkey shell";
            description = ''
              Custom welcome message to display when entering the shell.
              If null (default), no welcome message is shown.
            '';
          };

          quiet = mkOption {
            type = types.bool;
            default = true;
            description = ''
              Suppress verbose shell entry messages.
              Set TURNKEY_VERBOSE=1 in your environment to see verbose output.
            '';
          };

          # ==========================================================================
          # Go language support
          # ==========================================================================
          go = {
            enable = mkOption {
              type = types.bool;
              default = true;
              description = "Enable Go dependency management for Buck2";
            };

            cell = mkOption {
              type = types.nullOr types.package;
              default = null;
              description = ''
                Nix derivation containing the Go dependencies cell.
                When set, a 'godeps' cell will be added to .buckconfig
                and symlinked to .turnkey/godeps.

                Prefer using depsFile instead for declarative configuration.
              '';
            };

            depsFile = mkOption {
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

            modFile = mkOption {
              type = types.str;
              default = "go.mod";
              description = ''
                Relative path to go.mod file (for staleness checking and regeneration).
              '';
            };

            sumFile = mkOption {
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

          # ==========================================================================
          # Rust language support
          # ==========================================================================
          rust = {
            enable = mkOption {
              type = types.bool;
              default = true;
              description = "Enable Rust dependency management for Buck2";
            };

            cell = mkOption {
              type = types.nullOr types.package;
              default = null;
              description = ''
                Nix derivation containing the Rust dependencies cell.
                When set, a 'rustdeps' cell will be added to .buckconfig
                and symlinked to .turnkey/rustdeps.

                Prefer using depsFile instead for declarative configuration.
              '';
            };

            depsFile = mkOption {
              type = types.nullOr types.path;
              default = null;
              example = lib.literalExpression "./rust-deps.toml";
              description = ''
                Path to rust-deps.toml file declaring Rust crate dependencies.
                When set, turnkey will build the rustdeps cell automatically
                using the gen-rust-buck.py approach.
              '';
            };

            cargoTomlFile = mkOption {
              type = types.str;
              default = "Cargo.toml";
              description = ''
                Relative path to Cargo.toml file (for staleness checking and regeneration).
              '';
            };

            cargoLockFile = mkOption {
              type = types.str;
              default = "Cargo.lock";
              description = ''
                Relative path to Cargo.lock file (for staleness checking and regeneration).
              '';
            };

            featuresFile = mkOption {
              type = types.nullOr types.path;
              default = null;
              example = lib.literalExpression "./rust-features.toml";
              description = ''
                Path to rust-features.toml file for manual feature overrides.
                This file is NOT generated - it's for resolving feature conflicts
                or forcing specific feature sets on crates.

                Format:
                  [overrides]
                  # Complete replacement
                  syn = ["derive", "parsing", "visit"]

                  # Additive/subtractive
                  serde = { add = ["alloc"] }
                  some-crate = { remove = ["incompatible-feature"] }
              '';
            };

            rustcFlagsRegistry = mkOption {
              type = types.attrsOf (types.listOf types.str);
              default = { };
              example = lib.literalExpression ''
                {
                  serde_json = ["--cfg" "fast_arithmetic=\"64\""];
                  rustix = ["--cfg" "libc" "--cfg" "linux_like" "--cfg" "linux_kernel"];
                  # Version-specific (takes precedence over crate name)
                  "rustix@0.39.0" = ["--cfg" "libc" "--cfg" "linux_like"];
                }
              '';
              description = ''
                Registry of rustc flags for crates whose build scripts generate cfg directives.
                Keys can be crate names (catch-all) or "crate@version" for version-specific flags.
                Version-specific entries take precedence over catch-all entries.

                Default includes serde_json and rustix fixups.
              '';
            };

            buildScriptFixups = mkOption {
              type = types.attrsOf (types.either types.str (types.functionTo types.str));
              default = { };
              example = lib.literalExpression ''
                {
                  # Simple shell string
                  my_crate = '''
                    mkdir -p "$FIXUP_OUT_DIR"
                    echo "// generated" > "$FIXUP_OUT_DIR/config.rs"
                  ''';

                  # Version-specific (takes precedence)
                  "my_crate@1.2.3" = '''
                    mkdir -p "$FIXUP_OUT_DIR"
                    echo "// special for 1.2.3" > "$FIXUP_OUT_DIR/config.rs"
                  ''';

                  # Function receiving context
                  another_crate = { crateName, version, patchVersion, key }: '''
                    mkdir -p "$FIXUP_OUT_DIR"
                    echo "pub const VERSION: &str = \"''${version}\";" > "$FIXUP_OUT_DIR/version.rs"
                  ''';
                }
              '';
              description = ''
                Registry of build script fixups for crates that need pre-generated files.
                Keys can be crate names (catch-all) or "crate@version" for version-specific fixups.

                Fixups can be:
                - A shell string with variables: $FIXUP_OUT_DIR, $FIXUP_SRC_DIR, $CRATE_NAME, $CRATE_VERSION, $PATCH_VERSION, $CRATE_KEY
                - A function taking { crateName, version, patchVersion, key } and returning shell commands

                Default includes serde_core, serde, and ring fixups.
              '';
            };
          };

          # ==========================================================================
          # Python language support
          # ==========================================================================
          python = {
            enable = mkOption {
              type = types.bool;
              default = true;
              description = "Enable Python dependency management for Buck2";
            };

            cell = mkOption {
              type = types.nullOr types.package;
              default = null;
              description = ''
                Nix derivation containing the Python dependencies cell.
                When set, a 'pydeps' cell will be added to .buckconfig
                and symlinked to .turnkey/pydeps.

                Prefer using depsFile instead for declarative configuration.
              '';
            };

            depsFile = mkOption {
              type = types.nullOr types.path;
              default = null;
              example = lib.literalExpression "./python-deps.toml";
              description = ''
                Path to python-deps.toml file declaring Python package dependencies.
                When set, turnkey will build the pydeps cell automatically.
              '';
            };

            pyprojectFile = mkOption {
              type = types.str;
              default = "pyproject.toml";
              description = ''
                Relative path to pyproject.toml file (for staleness checking and regeneration).
              '';
            };

            lockFile = mkOption {
              type = types.nullOr types.str;
              default = null;
              example = "pylock.toml";
              description = ''
                Relative path to Python lock file (pylock.toml or requirements.txt).
                If set, this is used as the source for staleness checking.
                If null, pyproject.toml is used as the source.
              '';
            };
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

      # Build gobuckify for generating BUCK files
      gobuckify = import ../../packages/gobuckify.nix { inherit pkgs lib; };

      # Build godeps cell from go.depsFile if specified and exists
      # The file may not exist on first run (before .envrc generates it)
      # Only built if go.enable is true
      godepsCell =
        if cfg.buck2.go.enable then
          if cfg.buck2.go.depsFile != null && builtins.pathExists cfg.buck2.go.depsFile then
            import ../../buck2/go-deps-cell.nix {
              inherit pkgs lib gobuckify;
              depsFile = cfg.buck2.go.depsFile;
            }
          else
            cfg.buck2.go.cell
        else
          null;

      # Build rustdeps cell from rust.depsFile if specified and exists
      # Only built if rust.enable is true
      rustdepsCell =
        if cfg.buck2.rust.enable then
          if cfg.buck2.rust.depsFile != null && builtins.pathExists cfg.buck2.rust.depsFile then
            import ../../buck2/rust-deps-cell.nix {
              inherit pkgs lib;
              depsFile = cfg.buck2.rust.depsFile;
              featuresFile =
                if cfg.buck2.rust.featuresFile != null && builtins.pathExists cfg.buck2.rust.featuresFile
                then cfg.buck2.rust.featuresFile
                else null;
              rustcFlagsRegistry = cfg.buck2.rust.rustcFlagsRegistry;
              buildScriptFixups = cfg.buck2.rust.buildScriptFixups;
            }
          else
            cfg.buck2.rust.cell
        else
          null;

      # Build pydeps cell from python.depsFile if specified and exists
      # Only built if python.enable is true
      pydepsCell =
        if cfg.buck2.python.enable then
          if cfg.buck2.python.depsFile != null && builtins.pathExists cfg.buck2.python.depsFile then
            import ../../buck2/python-deps-cell.nix {
              inherit pkgs lib;
              depsFile = cfg.buck2.python.depsFile;
            }
          else
            cfg.buck2.python.cell
        else
          null;

      # Create a shell configuration for each declaration file
      mkShellConfig = shellName: declarationFile: {
        imports = [ ../../devenv/turnkey ];

        turnkey = {
          registry = lib.mkDefault registry;
          declarationFile = declarationFile;

          # Pass through Buck2 configuration with new language-specific namespaces
          buck2 = {
            enable = cfg.buck2.enable;
            prelude = {
              strategy = cfg.buck2.prelude.strategy;
              path = cfg.buck2.prelude.path;
            };
            # Shell entry options
            welcomeMessage = cfg.buck2.welcomeMessage;
            quiet = cfg.buck2.quiet;

            # Go language configuration
            go = {
              enable = cfg.buck2.go.enable;
              cell = godepsCell;
              depsFile =
                if cfg.buck2.go.depsFile != null
                then builtins.baseNameOf cfg.buck2.go.depsFile
                else "go-deps.toml";
              modFile = cfg.buck2.go.modFile;
              sumFile = cfg.buck2.go.sumFile;
              autoRegenerate = cfg.buck2.go.autoRegenerate;
              generateOnShellEntry = cfg.buck2.go.generateOnShellEntry;
            };

            # Rust language configuration
            rust = {
              enable = cfg.buck2.rust.enable;
              cell = rustdepsCell;
              depsFile =
                if cfg.buck2.rust.depsFile != null
                then builtins.baseNameOf cfg.buck2.rust.depsFile
                else null;
              cargoTomlFile = cfg.buck2.rust.cargoTomlFile;
              cargoLockFile = cfg.buck2.rust.cargoLockFile;
            };

            # Python language configuration
            python = {
              enable = cfg.buck2.python.enable;
              cell = pydepsCell;
              depsFile =
                if cfg.buck2.python.depsFile != null
                then builtins.baseNameOf cfg.buck2.python.depsFile
                else null;
              pyprojectFile = cfg.buck2.python.pyprojectFile;
              lockFile = cfg.buck2.python.lockFile;
            };
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
