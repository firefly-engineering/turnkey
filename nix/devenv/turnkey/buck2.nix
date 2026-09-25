# Buck2 toolchain generation module for devenv
#
# Generates a Buck2 toolchains cell from toolchain.toml declarations.
# Creates a symlinked .buckconfig pointing to the Nix store.

{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.turnkey.buck2;
  turnkeyCfg = config.turnkey;

  # Pre-commit check tools (Rust implementations with tree-sitter parsing)
  checkSourceCoverageRs = import ../../packages/check-source-coverage-rs.nix { inherit pkgs lib; };
  checkRustEditionRs = import ../../packages/check-rust-edition-rs.nix { inherit pkgs lib; };

  # Load the toolchain mappings
  mappings = import ../../buck2/mappings.nix {
    inherit lib;
    mdbookPreprocessors = cfg.mdbook.preprocessors;
  };

  # Toolchain declarations (name -> spec, e.g. { version = "3"; }) from the declaration file
  declaredToolchains =
    if turnkeyCfg.declarationFile != null then
      (import ../../lib/toolchain-declaration.nix { inherit lib; }).toolchains turnkeyCfg.declarationFile
    else
      { };

  # What the toolchains cell holds (nix/buck2/toolchains-cell.nix)
  toolchainsCellContent = import ../../buck2/toolchains-cell.nix { inherit lib; } {
    inherit mappings declaredToolchains resolvedRegistry;
  };
  finalToolchains = toolchainsCellContent.toolchains;
  inherit (toolchainsCellContent) runtimeDeps;

  # Teller lib for registry resolution (injected via flake-parts module)
  turnkeyLib = turnkeyCfg.tellerLib;

  # Resolve runtime dependencies to actual packages from versioned registry
  runtimePackages = builtins.filter (p: p != null) (
    map (name:
      let entry = turnkeyCfg.registry.${name} or null;
      in if entry == null then null
         else turnkeyLib.resolveTool turnkeyCfg.registry name {}
    ) runtimeDeps
  );

  # Create a resolved registry for dynamicAttrs (maps toolchain names to packages)
  # This allows mappings.nix dynamicAttrs functions to use ${registry.clang}/bin/clang
  # Declared toolchains resolve at their declared version, so a path baked into
  # the toolchains cell is the same package the dev shell provides.
  resolvedRegistry = builtins.mapAttrs (name: entry:
    turnkeyLib.resolveTool turnkeyCfg.registry name (declaredToolchains.${name} or { })
  ) turnkeyCfg.registry;

  # The languages turnkey manages dependencies for (nix/buck2/languages.nix)
  languages = import ../../buck2/languages.nix { inherit pkgs lib; };
  enabledLanguages = builtins.filter (language: cfg.${language.name}.enable) languages;

  # Internal generator packages (not exposed through registry, added to shell automatically)
  # These are turnkey implementation details, not user-configurable toolchains
  internalPackages = let
    # deps-extract is the unified tree-sitter based import extractor for rules.star sync
    # Built with only the features needed for enabled languages
    depsExtract = import ../../packages/deps-extract.nix {
      inherit pkgs lib;
      enablePython = cfg.python.enable;
      enableRust = cfg.rust.enable;
      enableTypescript = cfg.javascript.enable;
      enableSolidity = cfg.solidity.enable;
    };
    # Shim that routes `pytest` through `uv run pytest`, so workspace
    # editable installs are visible to the test runner.
    pytestShim = import ../../packages/pytest-uv-shim.nix { inherit pkgs lib; };
  in
    map (language: language.generator) enabledLanguages
    ++ lib.optional cfg.python.enable pytestShim
    # Always include deps-extract (used by tk rules sync for all non-Go languages)
    ++ [ depsExtract ];

  # Toolchains cell derivation
  toolchainsCell = pkgs.runCommand "turnkey-toolchains-cell" { } ''
    mkdir -p $out

    # Create BUCK file (Buck2's buildfile name setting only applies to root cell)
    cat > $out/BUCK <<'BUCK'
    ${toolchainsCellContent.buckFile}
    BUCK

    # Create cell identity .buckconfig
    cat > $out/.buckconfig <<'BUCKCONFIG'
    [cells]
        toolchains = .
        prelude = ${preludeCellPath}
    BUCKCONFIG
  '';

  # turnkey's prelude for the pinned buck2 release, or the consumer's own
  # (prelude.path), symlinked at .turnkey/prelude
  preludeCellPath = ".turnkey/prelude";
  # Reading the removed options makes setting one an error.
  customPrelude =
    assert lib.all (value: value == null) [
      cfg.prelude.strategy
      cfg.prelude.gitOrigin
      cfg.prelude.commitHash
      cfg.go.autoRegenerate
      cfg.go.generateOnShellEntry
    ];
    cfg.prelude.path != null;
  prelude = if customPrelude then cfg.prelude.path else cfg.prelude.package;

  # Toolchains cell is accessed via a symlink at .turnkey/toolchains
  toolchainsCellPath = ".turnkey/toolchains";

  # ==========================================================================
  # Nix-backed cells registry
  # ==========================================================================
  # Define all Nix-backed cells here. Each cell needs:
  #   - name: Buck2 cell name (used in .buckconfig and targets)
  #   - path: Symlink path under .turnkey/
  #   - derivation: The Nix derivation containing the cell
  #   - description: Human-readable description for logging
  #
  # The registry automatically handles:
  #   - [cells] section in .buckconfig
  #   - Platform detector specs
  #   - Symlink creation in enterShell
  # ==========================================================================

  nixCells = lib.filterAttrs (_: cell: cell.derivation != null) (
    lib.listToAttrs (
      map (
        language:
        lib.nameValuePair language.cellName {
          name = language.cellName;
          path = ".turnkey/${language.cellName}";
          derivation = if cfg.${language.name}.enable then cfg.${language.name}.cell else null;
          inherit (language) description;
        }
      ) languages
    )
    // {
      prelude = {
        name = "prelude";
        path = preludeCellPath;
        derivation = prelude;
        description = "Prelude";
      };
    }
  );

  # The symlinks the shell keeps pointing at what it generated
  # (managed-links.nix)
  managedLinks = import ./managed-links.nix { inherit lib; };
  links =
    [
      {
        path = ".buckconfig";
        target = buckconfig;
        label = ".buckconfig";
      }
      {
        path = ".turnkey/sync.toml";
        target = syncConfig;
        label = "sync.toml";
      }
      {
        path = toolchainsCellPath;
        target = toolchainsCell;
        label = "toolchains cell";
      }
    ]
    ++ map (cell: {
      inherit (cell) path;
      target = cell.derivation;
      label = "${cell.name} cell";
    }) (lib.attrValues nixCells);

  # Generate info output for all Nix-backed cells
  nixCellsInfo = lib.concatStringsSep "\n" (
    lib.mapAttrsToList (_: cell: ''echo "  ${cell.description}: ${cell.derivation}"'') nixCells
  );

  # Generate env vars for all Nix-backed cells (for .envrc symlink sync)
  # Format: TURNKEY_CELL_<NAME> = "<path>:<derivation>"
  nixCellsEnvVars = lib.mapAttrs' (_: cell:
    lib.nameValuePair
      "TURNKEY_CELL_${lib.toUpper cell.name}"
      "${cell.path}:${cell.derivation}"
  ) nixCells;

  # The pinned buck2 release (docs/adr/0002-turnkey-owns-the-buck2-version.md)
  buck2Source = import ../../buck2/buck2-source.nix { inherit pkgs lib; };

  # turnkey-test-runner's protocol code, generated for the pinned buck2
  # release (nix/buck2/buck2-source.nix)
  testRunnerProtocol = import ../../packages/test-runner-protocol.nix { inherit pkgs lib; };

  # turnkey-test-runner replaces buck2's bundled runner when test result
  # caching is enabled. Only turnkey's prelude is known to mark its test rules
  # cache-safe, so a custom prelude turns caching off.
  testRunner =
    if cfg.testCache.enable && !customPrelude then
      import ../../packages/turnkey-test-runner.nix { inherit pkgs lib; }
    else
      null;

  # The local test result cache listens on a fixed loopback port. buck2's RE
  # client can't use Unix sockets, and the address must not be passed with -c,
  # which would change the daemon's startup config.
  # A user can move it with TURNKEY_TEST_CACHE_PORT, read when the shell is
  # evaluated (turnkey shells evaluate impurely; a pure evaluation keeps the
  # default). tk gets the same address through TURNKEY_TEST_CACHE_ADDRESS.
  testCacheLocal = cfg.testCache.endpoint == null;
  testCacheAddress =
    if testCacheLocal then "grpc://127.0.0.1:${toString testCachePort}" else cfg.testCache.endpoint;
  testCachePort =
    let
      override = builtins.getEnv "TURNKEY_TEST_CACHE_PORT";
    in
    if override == "" then 47301 else lib.toInt override;

  # PATH for cached tests: Nix store paths only, so every tool a test can run
  # is part of its result key (test_caching.bzl).
  testPath = lib.makeBinPath [
    pkgs.bash
    pkgs.coreutils
    pkgs.diffutils
  ];

  # The .buckconfig (nix/buck2/buckconfig.nix)
  buckconfigContent = import ../../buck2/buckconfig.nix { inherit lib; } {
    cells = lib.attrValues nixCells;
    inherit toolchainsCellPath testRunnerProtocol;
    testCache =
      if testRunner == null then
        null
      else
        {
          runner = testRunner;
          path = testPath;
          address = testCacheAddress;
          tls = !testCacheLocal && cfg.testCache.tls;
        };
  };

  # Buckconfig file derivation
  buckconfig = pkgs.writeText "turnkey.buckconfig" buckconfigContent;

  # ==========================================================================
  # Sync configuration generation
  # ==========================================================================
  # Generate .turnkey/sync.toml from Nix configuration.
  # This replaces the need for users to manually create sync.toml.
  # Rules are generated based on which deps files are configured.
  # ==========================================================================

  # .turnkey/sync.toml (nix/buck2/sync-config.nix)
  syncConfigContent =
    (import ../../buck2/sync-config.nix { inherit lib; } {
      inherit languages;
      buck2 = cfg;
    }).content;

  # Sync config file derivation
  syncConfig = pkgs.writeText "turnkey.sync.toml" syncConfigContent;

in
{
  # Outside turnkey.buck2: set under a condition that reads turnkey.buck2
  options.turnkey.managedLinks = lib.mkOption {
    type = lib.types.listOf lib.types.attrs;
    internal = true;
    default = [ ];
    description = "The symlinks the shell maintains, [ { path, target, label } ], for direnv's use_turnkey.";
  };

  options.turnkey.buck2 = lib.mkOption {
    type = lib.types.submoduleWith {
      modules = [
        (import ../../buck2/options.nix {
          inherit lib;
          inherit (buck2Source) version;
        })
        {
          # What the flake-parts module injects
          options = {
            package = lib.mkOption {
              type = lib.types.package;
              internal = true;
              description = "The pinned buck2 binary, resolved from turnkey's own registry.";
            };

            prelude.package = lib.mkOption {
              type = lib.types.package;
              internal = true;
              description = "turnkey's prelude for the pinned buck2 release.";
            };
          };
        }
      ];
    };
    default = { };
    description = "turnkey's Buck2 integration (options declared in nix/buck2/options.nix).";
  };

  config = lib.mkIf (cfg.enable && turnkeyCfg.enable) {
    # Add runtime dependencies and internal tools to shell
    # - cfg.package: the pinned buck2 binary
    # - runtimePackages: tools needed in PATH for Buck2 actions (e.g., clang for cxx)
    # - internalPackages: turnkey generators (godeps-gen, etc.) based on enabled languages
    packages = [ cfg.package ] ++ runtimePackages ++ internalPackages;

    turnkey.managedLinks = links;

    # Export paths for debugging and inspection
    env = {
      # Use .turnkey isolation dir so build outputs are ignored by Go/Cargo/pytest
      BUCK_ISOLATION_DIR = ".turnkey";
      TURNKEY_BUCK2_TOOLCHAINS_CELL = "${toolchainsCell}";
      TURNKEY_BUCK2_CONFIG = "${buckconfig}";
      TURNKEY_BUCK2_SYNC_CONFIG = "${syncConfig}";
      TURNKEY_BUCK2_TOOLCHAINS = lib.concatStringsSep "," finalToolchains;
      TURNKEY_BUCK2_RUNTIME_DEPS = lib.concatStringsSep "," runtimeDeps;
    } // nixCellsEnvVars
      # Generated protocol code for Cargo builds of turnkey-test-runner
      // {
        TURNKEY_TEST_RUNNER_PROTOCOL = "${testRunnerProtocol}";
      }
      # The local test result cache tk starts on demand (src/go/pkg/testcache)
      // lib.optionalAttrs (testRunner != null) {
        TURNKEY_TEST_CACHE_ADDRESS = testCacheAddress;
      }
      # Only a local cache has a server for tk to manage
      // lib.optionalAttrs (testRunner != null && testCacheLocal) {
        TURNKEY_TEST_CACHE_SERVER = "${pkgs.bazel-remote}/bin/bazel-remote";
      }
      # Store tk's share path for shell completion setup
      // lib.optionalAttrs (turnkeyCfg.registry ? tk) {
        TURNKEY_TK_SHARE = "${resolvedRegistry.tk}/share";
      }
      # Suppress devenv task trace output when quiet mode is enabled
      // lib.optionalAttrs cfg.quiet {
        DEVENV_TASKS_QUIET = "true";
      };

    # Create symlinks on shell entry
    enterShell = ''
      # Add tk completions to XDG_DATA_DIRS for fish/bash/zsh completion discovery
      if [ -n "''${TURNKEY_TK_SHARE:-}" ]; then
        export XDG_DATA_DIRS="''${TURNKEY_TK_SHARE}:''${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
      fi

      # Point .buckconfig, .turnkey/sync.toml and the cells at what this
      # shell generated
      ${managedLinks.ensure links}

      # Ensure .buckroot exists (marks project boundary for Buck2)
      if [ ! -e .buckroot ]; then
        touch .buckroot
        echo "turnkey: Created .buckroot file"
      fi

      # Welcome message (if configured)
      ${lib.optionalString (cfg.welcomeMessage != null) ''
        echo "${cfg.welcomeMessage} (buck2 ${cfg.version})"
      ''}

      # Verbose output (shown when quiet=false or TURNKEY_VERBOSE=1)
      if [ -n "''${TURNKEY_VERBOSE:-}" ]${lib.optionalString (!cfg.quiet) " || true"}; then
        echo "Buck2 configured by turnkey"
        echo "  Toolchains: ${lib.concatStringsSep ", " finalToolchains}"
        echo "  Runtime deps: ${lib.concatStringsSep ", " runtimeDeps}"
        ${nixCellsInfo}
        echo "  Cell: $TURNKEY_BUCK2_TOOLCHAINS_CELL"
      fi

      # tk sync on shell entry (if enabled and tk is available)
      ${lib.optionalString cfg.tk.syncOnShellEntry ''
        if command -v tk >/dev/null 2>&1; then
          # Run tk sync - use --quiet unless TURNKEY_VERBOSE is set
          # Note: flags must come before subcommand (tk --quiet sync, not tk sync --quiet)
          if [ -n "''${TURNKEY_VERBOSE:-}" ]; then
            tk sync || echo "turnkey: tk sync failed (continuing anyway)"
          else
            tk --quiet sync || echo "turnkey: tk sync failed (continuing anyway)"
          fi
        fi
      ''}

      # buck2 alias to tk (if enabled)
      # Users can set TURNKEY_NO_ALIAS=1 to bypass the alias
      ${lib.optionalString cfg.tk.aliasBuck2 ''
        if [ -z "''${TURNKEY_NO_ALIAS:-}" ]; then
          if command -v tk >/dev/null 2>&1; then
            alias buck2='tk'
            if [ -n "''${TURNKEY_VERBOSE:-}" ]${lib.optionalString (!cfg.quiet) " || true"}; then
              echo "turnkey: buck2 is aliased to tk (set TURNKEY_NO_ALIAS=1 to disable)"
            fi
          fi
        fi
      ''}
    '';

    # Pre-commit hooks
    git-hooks.hooks = {
      # tk check - verify rules.star files and deps are in sync
      turnkey-check = lib.mkIf cfg.tk.preCommitCheck {
        enable = true;
        name = "turnkey-check";
        description = "Check that rules.star files and deps are in sync";
        # Run on any file change - tk check uses its own staleness detection
        always_run = true;
        pass_filenames = false;
        entry = ''
          sh -c '
            if command -v tk >/dev/null 2>&1; then
              tk check || {
                echo ""
                echo "Files out of sync. Run: tk sync"
                exit 1
              }
            else
              echo "Warning: tk not found in PATH, skipping sync check"
            fi
          '
        '';
      };

      # Rust edition alignment check (uses tree-sitter for proper Starlark parsing)
      rust-edition-check = lib.mkIf (cfg.rust.enable && cfg.tk.rustEditionCheck) {
        enable = true;
        name = "rust-edition-check";
        description = "Check Rust edition alignment between Cargo.toml and rules.star";
        files = "(Cargo\\.toml|rules\\.star)$";
        pass_filenames = false;
        entry = ''
          ${checkRustEditionRs}/bin/check-rust-edition-rs
        '';
      };

      # Monorepo dependency rules check
      monorepo-dep-check = lib.mkIf cfg.tk.monorepoDepCheck {
        enable = true;
        name = "monorepo-dep-check";
        description = "Check monorepo dependency rules (all languages)";
        files = "(go\\.mod|Cargo\\.toml|pyproject\\.toml|package\\.json)$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-monorepo-deps/__main__.py
        '';
      };

      # JS/TS tool config check (buck-out exclusions)
      js-test-config-check = lib.mkIf (cfg.javascript.enable && cfg.tk.jsTestConfigCheck) {
        enable = true;
        name = "js-test-config-check";
        description = "Check Jest/Vitest/Biome configs exclude buck-out directories";
        files = "(jest\\.config\\.(js|ts|mjs|cjs)|vitest\\.config\\.(js|ts|mjs|mts)|biome\\.jsonc?|package\\.json)$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-js-test-config/__main__.py
        '';
      };

      # Foundry configuration consistency check
      foundry-config-check = lib.mkIf (cfg.solidity.enable && cfg.tk.foundryConfigCheck) {
        enable = true;
        name = "foundry-config-check";
        description = "Check Foundry config consistency (solc version, dependencies)";
        files = "foundry\\.toml$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-foundry-config/__main__.py
        '';
      };

      # Source coverage check - validate all source files covered by Buck2 targets
      # Uses tree-sitter for proper Starlark parsing
      source-coverage-check = lib.mkIf cfg.tk.sourceCoverageCheck {
        enable = true;
        name = "source-coverage-check";
        description = "Check all source files are covered by Buck2 targets";
        files = "(\\.go|\\.rs|\\.py|\\.ts|\\.tsx|\\.js|\\.jsx|\\.sol|rules\\.star)$";
        pass_filenames = false;
        entry = ''
          ${checkSourceCoverageRs}/bin/check-source-coverage-rs --scope "${cfg.tk.sourceScope}"
        '';
      };

      # Nix flake check
      # Note: Disabled by default because devenv's container outputs require
      # nix2container input which may not be present in all projects.
      # Enable in your flake if you want this check.
      nix-flake-check = {
        enable = false;
        name = "nix-flake-check";
        description = "Check Nix flake validity";
        files = "\\.nix$";
        pass_filenames = false;
        entry = ''
          nix flake check --no-build --impure
        '';
      };

      # Starlark syntax validation using Buck2
      starlark-lint = {
        enable = true;
        name = "starlark-lint";
        description = "Lint Starlark files (rules.star, BUCK, etc.)";
        files = "(rules\\.star|BUCK|\\.bzl)$";
        pass_filenames = true;
        entry = ''
          ${cfg.package}/bin/buck2 starlark lint
        '';
      };

      # TOML syntax validation
      toml-syntax-check = {
        enable = true;
        name = "toml-syntax-check";
        description = "Check TOML syntax validity";
        files = "\\.toml$";
        # Exclude devenv/turnkey state directories and lock files
        excludes = [ "^\\.devenv/" "^\\.turnkey/" "^buck-out/" ];
        pass_filenames = true;
        entry = ''
          ${pkgs.python3}/bin/python -c '
import tomllib
import sys
errors = 0
for path in sys.argv[1:]:
    try:
        with open(path, "rb") as f:
            tomllib.load(f)
    except Exception as e:
        print(f"TOML syntax error in {path}: {e}", file=sys.stderr)
        errors += 1
if errors:
    sys.exit(1)
'
        '';
      };

      # JSON syntax validation
      json-syntax-check = {
        enable = true;
        name = "json-syntax-check";
        description = "Check JSON syntax validity";
        files = "\\.json$";
        # Exclude devenv/turnkey state directories
        excludes = [ "^\\.devenv/" "^\\.turnkey/" "^buck-out/" ];
        pass_filenames = true;
        entry = ''
          ${pkgs.python3}/bin/python -c '
import json
import sys
errors = 0
for path in sys.argv[1:]:
    try:
        with open(path, "r") as f:
            json.load(f)
    except Exception as e:
        print(f"JSON syntax error in {path}: {e}", file=sys.stderr)
        errors += 1
if errors:
    sys.exit(1)
'
        '';
      };
    };
  };
}
