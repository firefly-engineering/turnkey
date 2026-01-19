# Unified Dependency Cell Library
#
# Provides a phase-based build system for dependency cells, similar to Nix's
# stdenv phases. Each dependency becomes an individual Nix package, enabling
# deduplication across cell generations.
#
# Standard phases: fetch → patch → process → buildInfra → merge
#
# Usage:
#   let depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
#   in depsCell.mkRustDepsCell { depsFile = ./rust-deps.toml; }

{ pkgs, lib }:

let
  # Import sub-modules
  phases = import ./phases.nix { inherit lib; };
  hooks = import ./hooks.nix { inherit lib; };
  fetchers = import ./fetchers.nix { inherit pkgs lib; };
  fixups = import ./fixups { inherit pkgs lib; };
  adapters = import ./adapters { inherit pkgs lib; };

  # Extend lib with our functions for internal use
  libWithDepsCell = lib // {
    deps-cell = {
      inherit phases hooks fetchers fixups;
    };
  };

  # Re-import with extended lib
  phasesExt = import ./phases.nix { lib = libWithDepsCell; };
  hooksExt = import ./hooks.nix { lib = libWithDepsCell; };

in
rec {
  # Export sub-modules
  inherit phases hooks fetchers fixups adapters;

  # ==========================================================================
  # Generic Builders (Internal)
  # ==========================================================================

  # Build a single dependency package
  # This is the internal generic builder - language adapters wrap this
  mkDepPackage = {
    name,              # Dependency name (e.g., "serde@1.0.219")
    version,           # Version string
    language,          # "go" | "rust" | "python"
    fetchSpec,         # Fetch specification for fetchers.fetch

    # Optional
    extraHooks ? {},      # Additional hooks to merge
    patchCommands ? "",   # Shell commands for patch phase
    processCommands ? "", # Shell commands for process phase
    buildInfraCommands ? "", # Shell commands for buildInfra phase
    nativeBuildInputs ? [], # Additional build inputs
  }:
  let
    adapter = adapters.${language} or (throw "Unknown language: ${language}");

    # Merge adapter hooks with user hooks
    allHooks = hooksExt.mergeHooks [
      (adapter.hooks or {})
      extraHooks
    ];

    # Fetch the source
    src = fetchers.fetch fetchSpec;

    # Build context passed through phases
    context = {
      inherit name version language src;
    };

    # Default phase implementations
    defaultPhaseImpls = {
      fetch = _: ""; # Source is fetched via Nix, nothing to do in shell
      patch = _: patchCommands;
      process = _: processCommands;
      buildInfra = _: buildInfraCommands;
    };

    # Run phases to collect commands
    result = phasesExt.runDepPhases {
      hooks = allHooks;
      phaseImpls = defaultPhaseImpls;
      inherit context;
    };
  in
  pkgs.runCommand "dep-${language}-${lib.replaceStrings ["/" "@"] ["-" "-"] name}" {
    nativeBuildInputs = (adapter.buildInputs or []) ++ nativeBuildInputs;
    inherit src;
  } ''
    mkdir -p $out
    cp -r $src/* $out/
    chmod -R u+w $out

    # Run patch phase commands
    cd $out
    ${result.patchCmds or ""}

    # Run process phase commands
    ${result.processCmds or ""}

    # Run buildInfra phase commands
    ${result.buildInfraCmds or ""}
  '';

  # Build a complete deps cell from individual packages
  # This is the internal generic builder - language adapters wrap this
  mkDepsCell = {
    language,       # "go" | "rust" | "python"
    depsFile,       # Path to *-deps.toml
    cellName,       # e.g., "godeps", "rustdeps", "pydeps"

    # Optional
    languageConfig ? {}, # Language-specific configuration
    extraHooks ? {},     # Additional hooks for merge phase
    postMerge ? "",      # Shell commands after merge
    nativeBuildInputs ? [], # Additional build inputs
  }:
  let
    adapter = adapters.${language} or (throw "Unknown language: ${language}");
    depsToml = builtins.fromTOML (builtins.readFile depsFile);
    deps = depsToml.deps or {};

    # Build individual dep packages
    depPackages = lib.mapAttrs (key: depSpec:
      adapter.mkDepPackage {
        inherit key depSpec;
        config = languageConfig;
        allDeps = deps;
      }
    ) deps;

    # Merge context for hooks
    mergeContext = {
      inherit cellName depPackages deps;
      config = languageConfig;
    };

    # Merge adapter hooks with user hooks
    allHooks = hooksExt.mergeHooks [
      (adapter.cellHooks or {})
      extraHooks
    ];

    # Pre/post merge hook commands
    preMergeCmds = if allHooks ? preMerge then allHooks.preMerge mergeContext else "";
    postMergeCmds = if allHooks ? postMerge then allHooks.postMerge mergeContext else "";
  in
  pkgs.runCommand "${cellName}-cell" {
    nativeBuildInputs = (adapter.cellBuildInputs or []) ++ nativeBuildInputs;
    passthru = { inherit depPackages; };
  } ''
    mkdir -p $out/vendor

    # Pre-merge hook
    ${preMergeCmds}

    # Copy each dep package into vendor/
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (key: pkg:
      let
        # Normalize key for directory name
        dirName = lib.replaceStrings ["@"] ["/"] key;
      in ''
        mkdir -p "$out/vendor/${dirName}"
        cp -r ${pkg}/* "$out/vendor/${dirName}/"
        chmod -R u+w "$out/vendor/${dirName}"
      ''
    ) depPackages)}

    # Language-specific merge operations
    ${(adapter.mergeCommands or (_: "")) mergeContext}

    # Post-merge hook
    ${postMergeCmds}
    ${postMerge}

    # Generate cell .buckconfig
    cat > $out/.buckconfig << 'BUCKCONFIG'
    [cells]
        ${cellName} = .
        prelude = prelude

    [buildfile]
        name = rules.star
    BUCKCONFIG
  '';

  # ==========================================================================
  # Language-Specific Builders (Public API)
  # ==========================================================================

  # Re-export language-specific builders from adapters
  inherit (adapters.go) mkGoDepPackage mkGoDepsCell;
  inherit (adapters.rust) mkRustDepPackage mkRustDepsCell;
  inherit (adapters.python) mkPythonDepPackage mkPythonDepsCell;
  inherit (adapters.javascript) mkJsDepPackage mkJsDepsCell;
}
