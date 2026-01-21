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

  # Load the toolchain mappings
  mappings = import ../../buck2/mappings.nix { inherit lib; };

  # Get toolchain names from the declaration file
  toolchainNames =
    if turnkeyCfg.declarationFile != null then
      let
        decl = builtins.fromTOML (builtins.readFile turnkeyCfg.declarationFile);
      in
      if decl ? toolchains then builtins.attrNames decl.toolchains else [ ]
    else
      [ ];

  # Filter to only Buck2-relevant toolchains (those with mappings and not skipped)
  buck2Toolchains = builtins.filter (
    name: mappings ? ${name} && !(mappings.${name}.skip or false)
  ) toolchainNames;

  # Collect all implicit dependencies transitively
  collectDeps =
    toolchains:
    let
      directDeps = builtins.concatMap (
        name: mappings.${name}.implicitDependencies or [ ]
      ) toolchains;
      # Only include deps that have mappings and aren't skipped
      validDeps = builtins.filter (
        name: mappings ? ${name} && !(mappings.${name}.skip or false)
      ) directDeps;
      allDeps = lib.unique (toolchains ++ validDeps);
    in
    # Recurse until stable (handle transitive deps)
    if allDeps == toolchains then toolchains else collectDeps allDeps;

  # All toolchains including implicit dependencies
  allToolchains = collectDeps buck2Toolchains;

  # Add always-include toolchains (like genrule)
  alwaysIncluded = builtins.filter (name: mappings.${name}.alwaysInclude or false) (
    builtins.attrNames mappings
  );

  finalToolchains = lib.unique (allToolchains ++ alwaysIncluded);

  # Collect runtime dependencies from all active toolchains
  # These are packages that must be in PATH for Buck2 actions (e.g., clang for cxx)
  runtimeDeps = lib.unique (
    builtins.concatMap (
      name: mappings.${name}.runtimeDependencies or [ ]
    ) finalToolchains
  );

  # Import turnkey lib for resolution helpers
  turnkeyLib = import ../../lib { inherit lib pkgs; };

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
  resolvedRegistry = builtins.mapAttrs (name: entry:
    turnkeyLib.resolveTool turnkeyCfg.registry name {}
  ) turnkeyCfg.registry;

  # Internal generator packages (not exposed through registry, added to shell automatically)
  # These are turnkey implementation details, not user-configurable toolchains
  internalPackages = let
    godepsGen = import ../../packages/godeps-gen.nix { inherit pkgs lib; };
    rustdepsGen = import ../../packages/rustdeps-gen.nix { inherit pkgs lib; };
    pydepsGen = import ../../packages/pydeps-gen.nix { inherit pkgs lib; };
    jsdepsGen = import ../../packages/jsdeps-gen.nix { inherit pkgs lib; };
    soldepsGen = import ../../packages/soldeps-gen.nix { inherit pkgs lib; };
  in
    lib.optional cfg.go.enable godepsGen
    ++ lib.optional cfg.rust.enable rustdepsGen
    ++ lib.optional cfg.python.enable pydepsGen
    ++ lib.optional cfg.javascript.enable jsdepsGen
    ++ lib.optional cfg.solidity.enable soldepsGen;

  # Generate load statements for rules.star file
  generateLoads =
    toolchains:
    let
      # Collect all unique load statements
      loads = lib.unique (
        builtins.concatMap (
          name:
          map (t: {
            path = t.load;
            rule = t.rule;
          }) (mappings.${name}.targets or [ ])
        ) toolchains
      );
      # Group by load path for cleaner output
      byPath = lib.groupBy (l: l.path) loads;
      loadStmts = lib.mapAttrsToList (
        path: rules: ''load("${path}", ${lib.concatMapStringsSep ", " (r: ''"${r.rule}"'') rules})''
      ) byPath;
    in
    lib.concatStringsSep "\n" loadStmts;

  # Generate target instantiations for rules.star file
  generateTargets =
    toolchains:
    let
      targets = builtins.concatMap (
        name:
        map (
          t:
          let
            # Static attrs defined in the mapping
            staticAttrs = t.attrs or { };
            # Dynamic attrs resolved from registry (e.g., absolute paths to compilers)
            dynamicAttrs =
              if t ? dynamicAttrs then t.dynamicAttrs resolvedRegistry else { };
            # Merge: dynamic attrs override static attrs
            attrs = staticAttrs // dynamicAttrs;
            attrLines =
              [ "    name = \"${t.name}\"," ]
              ++ [ "    visibility = ${builtins.toJSON t.visibility}," ]
              ++ (lib.mapAttrsToList (k: v: "    ${k} = ${builtins.toJSON v},") attrs);
          in
          "${t.rule}(\n${lib.concatStringsSep "\n" attrLines}\n)"
        ) (mappings.${name}.targets or [ ])
      ) toolchains;
    in
    lib.concatStringsSep "\n\n" targets;

  # Generate the complete rules.star file content
  buckFileContent = ''
# Generated by turnkey - do not edit manually
# Toolchains: ${lib.concatStringsSep ", " finalToolchains}

${generateLoads finalToolchains}

${generateTargets finalToolchains}
'';

  # Toolchains cell derivation
  toolchainsCell = pkgs.runCommand "turnkey-toolchains-cell" { } ''
    mkdir -p $out

    # Create BUCK file (Buck2's buildfile name setting only applies to root cell)
    cat > $out/BUCK <<'BUCK'
    ${buckFileContent}
    BUCK

    # Create cell identity .buckconfig
    cat > $out/.buckconfig <<'BUCKCONFIG'
    [cells]
        toolchains = .
        prelude = ${preludeCellPath}
    BUCKCONFIG
  '';

  # Generate external_cells section based on prelude strategy
  externalCellsSection =
    if cfg.prelude.strategy == "bundled" then ''
      [external_cells]
          prelude = bundled
    ''
    else if cfg.prelude.strategy == "git" then ''
      [external_cells]
          prelude = git

      [external_cell_prelude]
          git_origin = ${cfg.prelude.gitOrigin}
          commit_hash = ${cfg.prelude.commitHash}
    ''
    else ""; # path and nix strategies use cells section directly

  # Prelude path in cells section
  preludeCellPath =
    if cfg.prelude.strategy == "bundled" then "prelude"
    else if cfg.prelude.strategy == "git" then "prelude"  # git uses external_cells
    else if cfg.prelude.strategy == "nix" then ".turnkey/prelude"  # symlinked derivation
    else cfg.prelude.path;  # path strategy uses direct path

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

  nixCells = lib.filterAttrs (_: cell: cell.derivation != null) ({
    godeps = {
      name = "godeps";
      path = ".turnkey/godeps";
      derivation = if cfg.go.enable then cfg.go.cell else null;
      description = "Go deps";
    };
    rustdeps = {
      name = "rustdeps";
      path = ".turnkey/rustdeps";
      derivation = if cfg.rust.enable then cfg.rust.cell else null;
      description = "Rust deps";
    };
    pydeps = {
      name = "pydeps";
      path = ".turnkey/pydeps";
      derivation = if cfg.python.enable then cfg.python.cell else null;
      description = "Python deps";
    };
    jsdeps = {
      name = "jsdeps";
      path = ".turnkey/jsdeps";
      derivation = if cfg.javascript.enable then cfg.javascript.cell else null;
      description = "JavaScript deps";
    };
    soldeps = {
      name = "soldeps";
      path = ".turnkey/soldeps";
      derivation = if cfg.solidity.enable then cfg.solidity.cell else null;
      description = "Solidity deps";
    };
  } // lib.optionalAttrs (cfg.prelude.strategy == "nix") {
    # Prelude cell (only when using nix strategy)
    prelude = {
      name = "prelude";
      path = ".turnkey/prelude";
      derivation = cfg.prelude.path;
      description = "Prelude";
    };
  });

  # Generate [cells] config entries for all Nix-backed cells
  nixCellsConfig = lib.concatStringsSep "\n" (
    lib.mapAttrsToList (_: cell: "    ${cell.name} = ${cell.path}") nixCells
  );

  # Generate platform detector specs for all Nix-backed cells
  nixCellsPlatformDetectors = lib.concatMapStringsSep "" (
    cell: " target:${cell.name}//...->prelude//platforms:default"
  ) (lib.attrValues nixCells);

  # Generate symlink creation script for all Nix-backed cells
  nixCellsSymlinkScript = lib.concatStringsSep "\n" (
    lib.mapAttrsToList (_: cell: ''
      # Ensure ${cell.path} points to the ${cell.description} cell
      if [ -L ${cell.path} ]; then
        if [ "$(readlink ${cell.path})" != "${cell.derivation}" ]; then
          ln -sfn "${cell.derivation}" ${cell.path}
          echo "turnkey: Updated ${cell.name} cell symlink"
        fi
      elif [ -e ${cell.path} ]; then
        echo "turnkey: Warning: ${cell.path} exists and is not a symlink"
      else
        ln -s "${cell.derivation}" ${cell.path}
        echo "turnkey: Created ${cell.name} cell symlink"
      fi
    '') nixCells
  );

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

  # Helper for Go deps enabled
  hasGodeps = cfg.go.enable && cfg.go.cell != null;

  # Generate buckconfig content
  # Note: isolation_dir is set via BUCK_ISOLATION_DIR env var, not here
  # (Buck2 ignores the config file setting for isolation_dir)
  buckconfigContent = ''
    # Generated by turnkey - do not edit manually

    [cells]
        root = .
        toolchains = ${toolchainsCellPath}
        prelude = ${preludeCellPath}
        none = none
    ${nixCellsConfig}

    [cell_aliases]
        config = prelude
        ovr_config = prelude
        fbcode = none
        fbsource = none
        fbcode_macros = none
        buck = none

    ${externalCellsSection}
    [parser]
        target_platform_detector_spec = target:root//...->prelude//platforms:default target:toolchains//...->prelude//platforms:default${nixCellsPlatformDetectors}

    [buildfile]
        name = rules.star

    [build]
        execution_platforms = prelude//platforms:default
  '';

  # Buckconfig file derivation
  buckconfig = pkgs.writeText "turnkey.buckconfig" buckconfigContent;

  # ==========================================================================
  # Sync configuration generation
  # ==========================================================================
  # Generate .turnkey/sync.toml from Nix configuration.
  # This replaces the need for users to manually create sync.toml.
  # Rules are generated based on which deps files are configured.
  # ==========================================================================

  # Build the list of sync rules based on what's enabled
  syncRules = lib.filter (r: r != null) [
    # Go deps rule
    (if cfg.go.enable && (cfg.go.cell != null || cfg.go.depsFile != null) then {
      name = "go";
      sources = [ cfg.go.modFile cfg.go.sumFile ];
      target = cfg.go.depsFile;
      generator = [ "godeps-gen" "--go-mod" cfg.go.modFile "--go-sum" cfg.go.sumFile "--prefetch" ];
    } else null)

    # Rust deps rule
    (if cfg.rust.enable && (cfg.rust.cell != null || cfg.rust.depsFile != null) then {
      name = "rust";
      sources = [ cfg.rust.cargoTomlFile cfg.rust.cargoLockFile ];
      target = if cfg.rust.depsFile != null then cfg.rust.depsFile else "rust-deps.toml";
      generator = [ "rustdeps-gen" "--cargo-lock" cfg.rust.cargoLockFile ];
    } else null)

    # Python deps rule
    (if cfg.python.enable && (cfg.python.cell != null || cfg.python.depsFile != null) then {
      name = "python";
      sources = if cfg.python.lockFile != null
        then [ cfg.python.lockFile ]
        else [ cfg.python.pyprojectFile ];
      target = if cfg.python.depsFile != null then cfg.python.depsFile else "python-deps.toml";
      generator = if cfg.python.lockFile != null
        then [ "pydeps-gen" "--lock" cfg.python.lockFile ]
        else [ "pydeps-gen" "--pyproject" cfg.python.pyprojectFile ];
    } else null)

    # JavaScript deps rule
    (if cfg.javascript.enable && (cfg.javascript.cell != null || cfg.javascript.depsFile != null) then {
      name = "javascript";
      sources = [ cfg.javascript.lockFile ];
      target = if cfg.javascript.depsFile != null then cfg.javascript.depsFile else "js-deps.toml";
      generator = [ "jsdeps-gen" "--lock" cfg.javascript.lockFile ]
        ++ lib.optionals cfg.javascript.includeDevDependencies [ "--include-dev" ];
    } else null)

    # Solidity deps rule
    (if cfg.solidity.enable && (cfg.solidity.cell != null || cfg.solidity.depsFile != null) then {
      name = "solidity";
      sources = [ cfg.solidity.foundryTomlFile ];
      target = if cfg.solidity.depsFile != null then cfg.solidity.depsFile else "solidity-deps.toml";
      generator = [ "soldeps-gen" "--foundry" cfg.solidity.foundryTomlFile ];
    } else null)
  ];

  # Format a single rule as TOML
  formatSyncRule = rule: ''
    [[deps]]
    name = ${builtins.toJSON rule.name}
    sources = ${builtins.toJSON rule.sources}
    target = ${builtins.toJSON rule.target}
    generator = ${builtins.toJSON rule.generator}
  '';

  # Generate full sync.toml content
  syncConfigContent = ''
    # Generated by turnkey - do not edit manually
    # This file configures tk sync to keep dependency files up-to-date.
    #
    # Rules are generated from your flake.nix configuration:
    # - goDepsFile → go deps rule
    # - rustDepsFile → rust deps rule
    # - pythonDepsFile → python deps rule
    #
    # To customize, modify your flake.nix buck2 options.

    ${lib.concatMapStringsSep "\n" formatSyncRule syncRules}
  '';

  # Sync config file derivation
  syncConfig = pkgs.writeText "turnkey.sync.toml" syncConfigContent;

in
{
  options.turnkey.buck2 = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable Buck2 toolchain generation from toolchain.toml.

        When enabled, this module:
        1. Generates a toolchains cell with Buck2 toolchain rules
        2. Creates a .buckconfig symlink pointing to Nix-managed configuration
        3. Adds environment variables for debugging/inspection
      '';
    };

    prelude = {
      strategy = lib.mkOption {
        type = lib.types.enum [
          "bundled"
          "git"
          "nix"
          "path"
        ];
        default = "bundled";
        description = ''
          How to provide the Buck2 prelude cell:
          - bundled: Use Buck2's built-in bundled prelude (simplest)
          - git: Use a git external cell (requires gitOrigin and commitHash)
          - nix: Use a Nix derivation (requires path to be a derivation)
          - path: Use an explicit filesystem path
        '';
      };

      path = lib.mkOption {
        type = lib.types.either lib.types.path (lib.types.either lib.types.str lib.types.package);
        default = "bundled://";
        description = ''
          Path to the prelude cell.
          - For bundled: use "bundled://" to use Buck2's built-in prelude
          - For git: the local checkout path
          - For nix: a Nix derivation or store path
          - For path: an absolute or relative filesystem path
        '';
      };

      gitOrigin = lib.mkOption {
        type = lib.types.str;
        default = "https://github.com/facebook/buck2-prelude.git";
        description = "Git origin URL for the prelude (when strategy = git)";
      };

      commitHash = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Git commit hash for the prelude (required when strategy = git)";
      };
    };

    # ==========================================================================
    # Go language support
    # ==========================================================================
    go = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Go dependency management for Buck2";
      };

      cell = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = ''
          Nix derivation containing the Go dependencies cell.
          When set, a 'godeps' cell will be added to .buckconfig
          and symlinked to .turnkey/godeps.
        '';
      };

      depsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = "go-deps.toml";
        description = ''
          Relative path to go-deps.toml file (for staleness checking).
          Used to warn when go-deps.toml needs regeneration.
        '';
      };

      modFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = "go.mod";
        description = ''
          Relative path to go.mod file (for staleness checking).
          Used to warn when go-deps.toml needs regeneration.
        '';
      };

      sumFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = "go.sum";
        description = ''
          Relative path to go.sum file (for staleness checking).
          Used to warn when go-deps.toml needs regeneration.
        '';
      };

      autoRegenerate = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Enable automatic go-deps.toml regeneration via pre-commit hook.
          When enabled, go-deps.toml will be regenerated when go.mod or go.sum
          are staged for commit.

          Note: godeps-gen is automatically included when go.enable is true.
          Requires nix-prefetch-github for hash fetching.
        '';
      };

      generateOnShellEntry = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Automatically generate/regenerate go-deps.toml when entering the shell
          if it's missing or stale (older than go.mod or go.sum).

          This enables a workflow where go-deps.toml is generated on-demand:
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
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Rust dependency management for Buck2";
      };

      cell = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = ''
          Nix derivation containing the Rust dependencies cell.
          When set, a 'rustdeps' cell will be added to .buckconfig
          and symlinked to .turnkey/rustdeps.
        '';
      };

      depsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Relative path to rust-deps.toml file (for staleness checking).
          Used by tk sync for Rust dependency management.
        '';
      };

      cargoTomlFile = lib.mkOption {
        type = lib.types.str;
        default = "Cargo.toml";
        description = ''
          Relative path to Cargo.toml file (for staleness checking).
        '';
      };

      cargoLockFile = lib.mkOption {
        type = lib.types.str;
        default = "Cargo.lock";
        description = ''
          Relative path to Cargo.lock file (for staleness checking).
        '';
      };
    };

    # ==========================================================================
    # Python language support
    # ==========================================================================
    python = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Python dependency management for Buck2";
      };

      cell = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = ''
          Nix derivation containing the Python dependencies cell.
          When set, a 'pydeps' cell will be added to .buckconfig
          and symlinked to .turnkey/pydeps.
        '';
      };

      depsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Relative path to python-deps.toml file (for staleness checking).
          Used by tk sync for Python dependency management.
        '';
      };

      pyprojectFile = lib.mkOption {
        type = lib.types.str;
        default = "pyproject.toml";
        description = ''
          Relative path to pyproject.toml file (for staleness checking).
        '';
      };

      lockFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Relative path to Python lock file (for staleness checking).
        '';
      };
    };

    # ==========================================================================
    # JavaScript language support
    # ==========================================================================
    javascript = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable JavaScript/TypeScript dependency management for Buck2";
      };

      cell = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = ''
          Nix derivation containing the JavaScript dependencies cell.
          When set, a 'jsdeps' cell will be added to .buckconfig
          and symlinked to .turnkey/jsdeps.
        '';
      };

      depsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Relative path to js-deps.toml file (for staleness checking).
          Used by tk sync for JavaScript dependency management.
        '';
      };

      lockFile = lib.mkOption {
        type = lib.types.str;
        default = "pnpm-lock.yaml";
        description = ''
          Relative path to pnpm-lock.yaml file (for staleness checking).
        '';
      };

      includeDevDependencies = lib.mkOption {
        type = lib.types.bool;
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
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Solidity dependency management for Buck2";
      };

      cell = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = ''
          Nix derivation containing the Solidity dependencies cell.
          When set, a 'soldeps' cell will be added to .buckconfig
          and symlinked to .turnkey/soldeps.
        '';
      };

      depsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Relative path to solidity-deps.toml file (for staleness checking).
          Used by tk sync for Solidity dependency management.
        '';
      };

      foundryTomlFile = lib.mkOption {
        type = lib.types.str;
        default = "foundry.toml";
        description = ''
          Relative path to foundry.toml file (for staleness checking).
        '';
      };
    };

    tk = {
      aliasBuck2 = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Alias buck2 to tk in the devenv shell.
          This allows users to continue using `buck2 build` etc. while
          getting automatic sync before build-graph-reading commands.

          Set TURNKEY_NO_ALIAS=1 in your environment to bypass the alias
          and use raw buck2 directly (useful for debugging).
        '';
      };

      syncOnShellEntry = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Run `tk sync` automatically when entering the devenv shell.
          This ensures the workspace is always in sync when starting development.

          The sync is fast when nothing is stale (just timestamp checks).
          Output is only shown if something needs to be regenerated.

          Requires tk to be available in PATH (add 'tk' to your toolchain.toml).
        '';
      };

      preCommitCheck = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Add a pre-commit hook that runs `tk check` before commits.
          Prevents committing when rules.star files or deps are out of sync.

          On failure, suggests running `tk sync` to fix.

          Requires tk to be available in PATH (add 'tk' to your toolchain.toml).
        '';
      };

      rustEditionCheck = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Add a pre-commit hook that verifies Rust edition alignment.
          Checks that:
          1. All workspace members use edition.workspace = true
          2. rules.star files have edition matching workspace.package.edition

          Requires Python 3.11+ with tomllib support.
        '';
      };

      monorepoDepCheck = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Add a pre-commit hook that verifies monorepo dependency rules.
          Checks that all languages follow the pattern of declaring deps
          at the root level:
          - Go: single go.mod at root
          - Rust: workspace.dependencies with workspace = true refs
          - Python: deps in root pyproject.toml
          - JavaScript: workspace: protocol for nested packages

          Requires Python 3.11+ with tomllib support.
        '';
      };
    };

    quiet = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Suppress verbose shell entry messages.

        When true (default), shell entry shows minimal output.
        When false, shows detailed toolchain and cell information.

        Set TURNKEY_VERBOSE=1 in your environment to see verbose
        output even when quiet mode is enabled.
      '';
    };

    welcomeMessage = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "Welcome to MyProject turnkey shell";
      description = ''
        Custom welcome message to display when entering the shell.

        If null (default), no welcome message is shown.
        Set to a string to display a custom message on shell entry.

        The message can include shell variables like $PWD.
      '';
    };
  };

  config = lib.mkIf (cfg.enable && turnkeyCfg.enable) {
    # Add runtime dependencies and internal tools to shell
    # - runtimePackages: tools needed in PATH for Buck2 actions (e.g., clang for cxx)
    # - internalPackages: turnkey generators (godeps-gen, etc.) based on enabled languages
    packages = runtimePackages ++ internalPackages;

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

      # Create .turnkey directory for turnkey-managed symlinks
      mkdir -p .turnkey

      # Ensure .buckconfig points to the turnkey-generated config
      if [ -L .buckconfig ]; then
        # Already a symlink - update it if needed
        if [ "$(readlink .buckconfig)" != "${buckconfig}" ]; then
          ln -sf "${buckconfig}" .buckconfig
          echo "turnkey: Updated .buckconfig symlink"
        fi
      elif [ -e .buckconfig ]; then
        echo "turnkey: Warning: .buckconfig exists and is not a symlink"
        echo "         Remove it to let turnkey manage Buck2 configuration"
      else
        ln -s "${buckconfig}" .buckconfig
        echo "turnkey: Created .buckconfig symlink"
      fi

      # Ensure .buckroot exists (marks project boundary for Buck2)
      if [ ! -e .buckroot ]; then
        touch .buckroot
        echo "turnkey: Created .buckroot file"
      fi

      # Ensure .turnkey/sync.toml points to the generated sync config
      if [ -L .turnkey/sync.toml ]; then
        if [ "$(readlink .turnkey/sync.toml)" != "${syncConfig}" ]; then
          ln -sfn "${syncConfig}" .turnkey/sync.toml
          echo "turnkey: Updated sync.toml symlink"
        fi
      elif [ -e .turnkey/sync.toml ]; then
        echo "turnkey: Warning: .turnkey/sync.toml exists and is not a symlink"
        echo "         Remove it to let turnkey manage sync configuration"
      else
        ln -s "${syncConfig}" .turnkey/sync.toml
        echo "turnkey: Created sync.toml symlink"
      fi

      # Ensure .turnkey/toolchains points to the generated toolchains cell
      if [ -L .turnkey/toolchains ]; then
        if [ "$(readlink .turnkey/toolchains)" != "${toolchainsCell}" ]; then
          ln -sfn "${toolchainsCell}" .turnkey/toolchains
          echo "turnkey: Updated toolchains cell symlink"
        fi
      elif [ -e .turnkey/toolchains ]; then
        echo "turnkey: Warning: .turnkey/toolchains exists and is not a symlink"
      else
        ln -s "${toolchainsCell}" .turnkey/toolchains
        echo "turnkey: Created toolchains cell symlink"
      fi

      # Create symlinks for all Nix-backed cells
      ${nixCellsSymlinkScript}

      # Auto-generate or check staleness of go-deps.toml
      _go_deps_file="${cfg.go.depsFile}"
      _go_mod_file="${cfg.go.modFile}"
      _go_sum_file="${cfg.go.sumFile}"
      _should_generate=0
      _generate_enabled=${if cfg.go.enable && cfg.go.generateOnShellEntry then "1" else "0"}

      if [ -f "$_go_mod_file" ]; then
        if [ ! -f "$_go_deps_file" ]; then
          # go-deps.toml doesn't exist
          if [ "$_generate_enabled" = "1" ]; then
            _should_generate=1
            echo "turnkey: go-deps.toml not found, will generate..."
          else
            echo ""
            echo "⚠️  turnkey: go-deps.toml not found!"
            echo "   Generate with: godeps-gen --go-mod $_go_mod_file --go-sum $_go_sum_file --prefetch > $_go_deps_file"
            echo ""
          fi
        else
          # Check if stale
          _stale=0
          if [ "$_go_mod_file" -nt "$_go_deps_file" ]; then
            _stale=1
          fi
          if [ -f "$_go_sum_file" ] && [ "$_go_sum_file" -nt "$_go_deps_file" ]; then
            _stale=1
          fi
          if [ "$_stale" = "1" ]; then
            if [ "$_generate_enabled" = "1" ]; then
              _should_generate=1
              echo "turnkey: go-deps.toml is stale, will regenerate..."
            else
              echo ""
              echo "⚠️  turnkey: go-deps.toml may be stale!"
              echo "   go.mod or go.sum is newer than go-deps.toml"
              echo "   Regenerate with: godeps-gen --go-mod $_go_mod_file --go-sum $_go_sum_file --prefetch > $_go_deps_file"
              echo ""
            fi
          fi
        fi

        # Generate if needed
        if [ "$_should_generate" = "1" ]; then
          if command -v godeps-gen >/dev/null 2>&1; then
            echo "turnkey: Running godeps-gen --prefetch..."
            if godeps-gen --go-mod "$_go_mod_file" --go-sum "$_go_sum_file" --prefetch > "$_go_deps_file.tmp"; then
              mv "$_go_deps_file.tmp" "$_go_deps_file"
              echo "turnkey: Generated $_go_deps_file"
              echo ""
              echo "ℹ️  Note: The godeps cell will be available on next shell entry."
              echo "   Run 'exit' and 'nix develop' again to use the new dependencies."
              echo ""
            else
              rm -f "$_go_deps_file.tmp"
              echo ""
              echo "❌ turnkey: Failed to generate go-deps.toml"
              echo "   Check the error above and try manually:"
              echo "   godeps-gen --go-mod $_go_mod_file --go-sum $_go_sum_file --prefetch > $_go_deps_file"
              echo ""
            fi
          else
            echo ""
            echo "⚠️  turnkey: godeps-gen not found in PATH"
            echo "   Add 'godeps-gen' to your toolchain.toml or packages to enable auto-generation"
            echo "   Or generate manually: nix run .#godeps-gen -- --go-mod $_go_mod_file --go-sum $_go_sum_file --prefetch > $_go_deps_file"
            echo ""
          fi
        fi
      fi

      # Welcome message (if configured)
      ${lib.optionalString (cfg.welcomeMessage != null) ''
        echo "${cfg.welcomeMessage}"
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
      # Automatic go-deps.toml regeneration
      godeps-gen = lib.mkIf (hasGodeps && cfg.go.autoRegenerate) {
        enable = true;
        name = "godeps-gen";
        description = "Regenerate go-deps.toml when go.mod or go.sum changes";
        files = "(go\\.mod|go\\.sum)$";
        pass_filenames = false;
        entry = ''
          sh -c '
            if command -v godeps-gen >/dev/null 2>&1; then
              echo "Regenerating ${cfg.go.depsFile}..."
              godeps-gen --go-mod "${cfg.go.modFile}" --go-sum "${cfg.go.sumFile}" --prefetch > "${cfg.go.depsFile}"
              git add "${cfg.go.depsFile}"
            else
              echo "Warning: godeps-gen not found in PATH, skipping regeneration"
            fi
          '
        '';
      };

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

      # Rust edition alignment check
      rust-edition-check = lib.mkIf (cfg.rust.enable && cfg.tk.rustEditionCheck) {
        enable = true;
        name = "rust-edition-check";
        description = "Check Rust edition alignment between Cargo.toml and rules.star";
        files = "(Cargo\\.toml|rules\\.star)$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-rust-edition/__main__.py
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

      # Nix flake check
      nix-flake-check = {
        enable = true;
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
          ${pkgs.buck2}/bin/buck2 starlark lint
        '';
      };

      # TOML syntax validation
      toml-syntax-check = {
        enable = true;
        name = "toml-syntax-check";
        description = "Check TOML syntax validity";
        files = "\\.toml$";
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
