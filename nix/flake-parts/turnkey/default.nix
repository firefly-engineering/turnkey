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
          type = types.lazyAttrsOf types.anything;
          default = { };
          defaultText = "Default versioned registry from nix/registry";
          description = ''
            Complete registry override. When set, replaces the default registry entirely.
            Each entry should have: { versions = { "<ver>" = <pkg>; }; default = "<ver>"; }
            Prefer using registryExtensions to add packages without duplicating defaults.
          '';
        };

        registryExtensions = mkOption {
          type = types.lazyAttrsOf types.anything;
          default = { };
          example = lib.literalExpression ''
            {
              # Single-version entry
              beads = {
                versions = { "default" = inputs.beads.packages.''${system}.default; };
                default = "default";
              };
              # Or use the helper: turnkey.lib.single inputs.beads.packages.''${system}.default
            }
          '';
          description = ''
            Extend the default registry with additional toolchains.
            Each entry should have: { versions = { "<ver>" = <pkg>; }; default = "<ver>"; }
            These are merged on top of the default registry.
          '';
        };

        wrapNativeTools = mkOption {
          type = types.bool;
          default = true;
          description = ''
            Automatically wrap native language tools (go, cargo, uv) with tw for auto-sync.
            When enabled, requesting 'go' in toolchain.toml gives you a wrapped version
            that automatically syncs dependency files when they change.

            Set to false to use unwrapped tools.
          '';
        };

        buck2 = {
          enable = mkOption {
            type = types.bool;
            default = false;
            description = "Enable Buck2 toolchain generation from toolchain.toml";
          };

          prelude = {
            strategy = mkOption {
              type = types.enum [
                "bundled"
                "git"
                "nix"
                "path"
              ];
              default = "nix";
              description = ''
                How to provide the Buck2 prelude cell:
                - nix: Use turnkey's Nix-backed prelude (default, recommended)
                - bundled: Use Buck2's built-in bundled prelude
                - git: Use a git external cell
                - path: Use an explicit filesystem path
              '';
            };

            path = mkOption {
              type = types.nullOr (types.either types.path (types.either types.str types.package));
              default = null;
              description = ''
                Path to the prelude cell.
                - For nix: defaults to turnkey-prelude derivation (can override with custom derivation)
                - For bundled: use "bundled://" to use Buck2's built-in prelude
                - For git: the local checkout path
                - For path: an absolute or relative filesystem path
              '';
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

                godeps-gen is automatically included when go.enable is true.
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

                godeps-gen is automatically included when go.enable is true.
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

          # ==========================================================================
          # JavaScript/TypeScript language support
          # ==========================================================================
          javascript = {
            enable = mkOption {
              type = types.bool;
              default = true;
              description = "Enable JavaScript/TypeScript dependency management for Buck2";
            };

            cell = mkOption {
              type = types.nullOr types.package;
              default = null;
              description = ''
                Nix derivation containing the JavaScript dependencies cell.
                When set, a 'jsdeps' cell will be added to .buckconfig
                and symlinked to .turnkey/jsdeps.

                Prefer using depsFile instead for declarative configuration.
              '';
            };

            depsFile = mkOption {
              type = types.nullOr types.path;
              default = null;
              example = lib.literalExpression "./js-deps.toml";
              description = ''
                Path to js-deps.toml file declaring JavaScript package dependencies.
                When set, turnkey will build the jsdeps cell automatically.
              '';
            };

            lockFile = mkOption {
              type = types.str;
              default = "pnpm-lock.yaml";
              description = ''
                Relative path to pnpm-lock.yaml file (for staleness checking and regeneration).
              '';
            };

            includeDevDependencies = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Include dev dependencies when generating js-deps.toml.
                Passed as --include-dev to jsdeps-gen.
              '';
            };
          };

          # ==========================================================================
          # Solidity language support
          # ==========================================================================
          solidity = {
            enable = mkOption {
              type = types.bool;
              default = true;
              description = "Enable Solidity dependency management for Buck2";
            };

            cell = mkOption {
              type = types.nullOr types.package;
              default = null;
              description = ''
                Nix derivation containing the Solidity dependencies cell.
                When set, a 'soldeps' cell will be added to .buckconfig
                and symlinked to .turnkey/soldeps.

                Prefer using depsFile instead for declarative configuration.
              '';
            };

            depsFile = mkOption {
              type = types.nullOr types.path;
              default = null;
              example = lib.literalExpression "./solidity-deps.toml";
              description = ''
                Path to solidity-deps.toml file declaring Solidity dependencies.
                When set, turnkey will build the soldeps cell automatically.
              '';
            };

            foundryTomlFile = mkOption {
              type = types.str;
              default = "foundry.toml";
              description = ''
                Relative path to foundry.toml file (for staleness checking and regeneration).
              '';
            };
          };

          # ==========================================================================
          # Pre-commit hook configuration (tk options)
          # ==========================================================================
          tk = {
            aliasBuck2 = mkOption {
              type = types.bool;
              default = true;
              description = ''
                Alias buck2 to tk in the devenv shell.
              '';
            };

            syncOnShellEntry = mkOption {
              type = types.bool;
              default = true;
              description = ''
                Run `tk sync` automatically when entering the devenv shell.
              '';
            };

            preCommitCheck = mkOption {
              type = types.bool;
              default = true;
              description = ''
                Add a pre-commit hook that runs `tk check` before commits.
              '';
            };

            rustEditionCheck = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Add a pre-commit hook that verifies Rust edition alignment.
              '';
            };

            monorepoDepCheck = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Add a pre-commit hook that verifies monorepo dependency rules.
              '';
            };

            jsTestConfigCheck = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Add a pre-commit hook that verifies Jest/Vitest/Biome configs
                properly exclude buck-out directories.
              '';
            };

            foundryConfigCheck = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Add a pre-commit hook that verifies Foundry configuration consistency.
                Checks that solc_version matches toolchain and dependencies match root.
              '';
            };

            sourceCoverageCheck = mkOption {
              type = types.bool;
              default = false;
              description = ''
                Add a pre-commit hook that validates all source files are covered by
                Buck2 targets in rules.star files.
              '';
            };

            sourceScope = mkOption {
              type = types.str;
              default = ".";
              description = ''
                Directory scope for source coverage checking.
                Default is "." (entire repository).
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

      # Import turnkey lib
      turnkeyLib = import ../../lib { inherit pkgs lib; };

      # Load default versioned registry
      defaultRegistry = import ../../registry { inherit pkgs lib; };

      # Helper for single-version entries
      single = pkg: {
        versions = {
          "default" = pkg;
        };
        default = "default";
      };

      # Normalize a registry entry to versioned format
      # Handles both flat (buck2 = pkgs.buck2) and versioned ({ versions = ...; default = ...; }) formats
      normalizeEntry =
        entry:
        if entry ? versions && entry ? default then
          entry # Already versioned
        else
          single entry; # Flat package -> convert to versioned

      # Normalize all entries in a registry to versioned format
      normalizeRegistry = reg: builtins.mapAttrs (_name: normalizeEntry) reg;

      # Merge versioned registries (toolchain level merge, version level merge)
      mergeRegistries =
        base: extensions:
        let
          mergeToolchain =
            name: ext:
            let
              existing = base.${name} or null;
            in
            if existing == null then
              ext
            else
              {
                versions = (existing.versions or { }) // (ext.versions or { });
                default = if ext ? default then ext.default else existing.default;
              };
        in
        base // (builtins.mapAttrs mergeToolchain extensions);

      # Registry merging:
      # 1. Start with default registry
      # 2. Merge registryExtensions on top (versions are additive, default overrides)
      # 3. If registry is explicitly set (non-empty), use that as complete override
      # 4. Normalize all entries to versioned format (handles flat pkgs.foo entries)
      baseRegistry = normalizeRegistry (
        if cfg.registry != { } then
          # Complete override - user specified full registry
          cfg.registry
        else
          # Default + extensions with proper merging
          mergeRegistries defaultRegistry cfg.registryExtensions
      );

      # Build the turnkey-prelude derivation (Nix-backed prelude cell)
      turnkeyPrelude = import ../../buck2/prelude.nix { inherit pkgs lib; };

      # Build tw for wrapping native tools
      tw = import ../../packages/tw.nix { inherit pkgs lib; };

      # Build wrapper packages for native tools
      # Each wrapper provides a binary with the same name as the tool (e.g., 'go')
      # but transparently invokes tw for auto-sync
      twWrappers = import ../../packages/tw-wrappers.nix { inherit pkgs lib tw; };

      # Tools that can be wrapped (must have entries in tw-wrappers.nix)
      wrappableTools = [
        "go"
        "cargo"
        "uv"
      ];

      # Augment registry with wrappers when wrapNativeTools is enabled
      # This replaces the tool entry with a wrapped version (same versioned structure)
      registry =
        if cfg.wrapNativeTools then
          baseRegistry
          // (lib.listToAttrs (
            lib.filter (x: x != null) (
              map (
                tool:
                if baseRegistry ? ${tool} then
                  {
                    name = tool;
                    value = single twWrappers."tw-${tool}";
                  }
                else
                  null
              ) wrappableTools
            )
          ))
        else
          baseRegistry;

      # Build buckgen for generating BUCK files
      buckgen = import ../../packages/buckgen.nix { inherit pkgs lib; };

      # Build godeps cell from go.depsFile if specified and exists
      # The file may not exist on first run (before .envrc generates it)
      # Only built if go.enable is true
      godepsCell =
        if cfg.buck2.go.enable then
          if cfg.buck2.go.depsFile != null && builtins.pathExists cfg.buck2.go.depsFile then
            import ../../buck2/go-deps-cell.nix {
              inherit pkgs lib buckgen;
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
                if cfg.buck2.rust.featuresFile != null && builtins.pathExists cfg.buck2.rust.featuresFile then
                  cfg.buck2.rust.featuresFile
                else
                  null;
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

      # Build jsdeps cell from javascript.depsFile if specified and exists
      # Only built if javascript.enable is true
      jsdepsCell =
        if cfg.buck2.javascript.enable then
          if cfg.buck2.javascript.depsFile != null && builtins.pathExists cfg.buck2.javascript.depsFile then
            import ../../buck2/js-deps-cell.nix {
              inherit pkgs lib;
              depsFile = cfg.buck2.javascript.depsFile;
            }
          else
            cfg.buck2.javascript.cell
        else
          null;

      # Build soldeps cell from solidity.depsFile if specified and exists
      # Only built if solidity.enable is true
      soldepsCell =
        if cfg.buck2.solidity.enable then
          if cfg.buck2.solidity.depsFile != null && builtins.pathExists cfg.buck2.solidity.depsFile then
            import ../../buck2/solidity-deps-cell.nix {
              inherit pkgs lib;
              depsFile = cfg.buck2.solidity.depsFile;
            }
          else
            cfg.buck2.solidity.cell
        else
          null;

      # Resolve the prelude path based on strategy
      # - nix: use turnkeyPrelude (or user-specified derivation)
      # - bundled: use "bundled://"
      # - path/git: use user-specified path
      resolvedPreludePath =
        if cfg.buck2.prelude.strategy == "nix" then
          if cfg.buck2.prelude.path != null then cfg.buck2.prelude.path else turnkeyPrelude
        else if cfg.buck2.prelude.strategy == "bundled" then
          "bundled://"
        else
          cfg.buck2.prelude.path;

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
              path = resolvedPreludePath;
            };
            # Shell entry options
            welcomeMessage = cfg.buck2.welcomeMessage;
            quiet = cfg.buck2.quiet;

            # Go language configuration
            go = {
              enable = cfg.buck2.go.enable;
              cell = godepsCell;
              depsFile =
                if cfg.buck2.go.depsFile != null then builtins.baseNameOf cfg.buck2.go.depsFile else "go-deps.toml";
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
                if cfg.buck2.rust.depsFile != null then builtins.baseNameOf cfg.buck2.rust.depsFile else null;
              cargoTomlFile = cfg.buck2.rust.cargoTomlFile;
              cargoLockFile = cfg.buck2.rust.cargoLockFile;
            };

            # Python language configuration
            python = {
              enable = cfg.buck2.python.enable;
              cell = pydepsCell;
              depsFile =
                if cfg.buck2.python.depsFile != null then builtins.baseNameOf cfg.buck2.python.depsFile else null;
              pyprojectFile = cfg.buck2.python.pyprojectFile;
              lockFile = cfg.buck2.python.lockFile;
            };

            # JavaScript language configuration
            javascript = {
              enable = cfg.buck2.javascript.enable;
              cell = jsdepsCell;
              depsFile =
                if cfg.buck2.javascript.depsFile != null then
                  builtins.baseNameOf cfg.buck2.javascript.depsFile
                else
                  null;
              lockFile = cfg.buck2.javascript.lockFile;
              includeDevDependencies = cfg.buck2.javascript.includeDevDependencies;
            };

            # Solidity language configuration
            solidity = {
              enable = cfg.buck2.solidity.enable;
              cell = soldepsCell;
              depsFile =
                if cfg.buck2.solidity.depsFile != null then
                  builtins.baseNameOf cfg.buck2.solidity.depsFile
                else
                  null;
              foundryTomlFile = cfg.buck2.solidity.foundryTomlFile;
            };

            # Pre-commit hook configuration
            tk = {
              aliasBuck2 = cfg.buck2.tk.aliasBuck2;
              syncOnShellEntry = cfg.buck2.tk.syncOnShellEntry;
              preCommitCheck = cfg.buck2.tk.preCommitCheck;
              rustEditionCheck = cfg.buck2.tk.rustEditionCheck;
              monorepoDepCheck = cfg.buck2.tk.monorepoDepCheck;
              jsTestConfigCheck = cfg.buck2.tk.jsTestConfigCheck;
              foundryConfigCheck = cfg.buck2.tk.foundryConfigCheck;
              sourceCoverageCheck = cfg.buck2.tk.sourceCoverageCheck;
              sourceScope = cfg.buck2.tk.sourceScope;
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
