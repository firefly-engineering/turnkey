# Rust Language Adapter for Dependency Cells
#
# Provides:
#   - mkRustDepPackage: Build a single Rust crate package
#   - mkRustDepsCell: Build a complete Rust dependency cell
#
# Rust dependencies are fetched from crates.io.
# Feature unification and BUCK generation happen during merge phase.

{ pkgs, lib }:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  fixups = import ../fixups { inherit pkgs lib; };
in
rec {
  # Build inputs for per-dependency builds
  buildInputs = with pkgs; [ stdenv.cc perl ];

  # Build inputs for cell builds
  cellBuildInputs = with pkgs; [ python3 ];

  # Hooks for per-dependency phases
  hooks = {
    # Build script fixups are applied during patch phase
    # This is handled in mkRustDepPackage based on fixup lookup
  };

  # Hooks for cell merge phase
  cellHooks = {
    # Feature unification and BUCK generation happen in postMerge
    # This is handled in mkRustDepsCell
  };

  # ==========================================================================
  # Public API
  # ==========================================================================

  # Build a single Rust crate package
  mkRustDepPackage = {
    name,               # Crate name (e.g., "serde")
    version,            # Version string (e.g., "1.0.219")
    sha256,             # SRI hash of the source

    # Optional
    buildScriptFixup ? null,  # Fixup commands for build.rs emulation
    rustcFlags ? [],          # --cfg flags for rustc
  }:
  let
    fetchSpec = fetchers.mkCratesIOSpec {
      crateName = name;
      inherit version sha256;
    };
  in
  pkgs.runCommand "dep-rust-${name}-${version}" {
    nativeBuildInputs = buildInputs;
    src = fetchers.fetch fetchSpec;
    passthru = {
      inherit name version rustcFlags;
    };
  } ''
    mkdir -p $out
    cp -r $src/* $out/
    chmod -R u+w $out

    # Apply build script fixup if provided
    cd $out
    ${if buildScriptFixup != null then buildScriptFixup else ""}
  '';

  # Build a complete Rust dependency cell
  mkRustDepsCell = {
    depsFile,                   # Path to rust-deps.toml
    featuresFile ? null,        # Path to rust-features.toml (optional)
    buildScriptFixups ? {},     # Additional build script fixups
    rustcFlagsRegistry ? {},    # Additional rustc flags

    # Tools (must be provided by caller)
    computeUnifiedFeatures ? null,  # Tool for feature unification
    genRustBuck ? null,             # Tool for BUCK generation
  }:
  let
    depsToml = builtins.fromTOML (builtins.readFile depsFile);
    deps = depsToml.deps or {};

    # Merge built-in fixups with user-provided
    allBuildScriptFixups = (fixups.builtinFixups.rust or {}) // buildScriptFixups;

    # Build individual dep packages
    depPackages = lib.mapAttrs (key: depSpec:
      let
        # Parse name from key (may be "name@version" format)
        parts = lib.splitString "@" key;
        crateName = depSpec.name or (lib.head parts);
        version = depSpec.version;
        patchVersion = lib.last (lib.splitString "." version);

        # Look up fixup
        fixupFn = allBuildScriptFixups.${key} or allBuildScriptFixups.${crateName} or null;
        fixup = if fixupFn != null then
          if builtins.isFunction fixupFn then
            fixupFn { inherit crateName version patchVersion key; vendorPath = "."; }
          else
            fixupFn
        else null;

        # Look up rustc flags
        flags = rustcFlagsRegistry.${key} or rustcFlagsRegistry.${crateName} or [];
      in
      mkRustDepPackage {
        name = crateName;
        inherit version;
        sha256 = depSpec.hash;
        buildScriptFixup = fixup;
        rustcFlags = flags;
      }
    ) deps;

    # Read features file if provided
    featuresOverrides = if featuresFile != null
      then builtins.fromTOML (builtins.readFile featuresFile)
      else {};
  in
  pkgs.runCommand "rustdeps-cell" {
    nativeBuildInputs = cellBuildInputs ++
      (if computeUnifiedFeatures != null then [ computeUnifiedFeatures ] else []) ++
      (if genRustBuck != null then [ genRustBuck ] else []);
    passthru = { inherit depPackages; };
  } ''
    mkdir -p $out/vendor

    # Copy each dep package into vendor/ with versioned path
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (key: pkg:
      let
        # Use key as directory name (includes version)
        dirName = key;
      in ''
        mkdir -p "$out/vendor/${dirName}"
        cp -r ${pkg}/* "$out/vendor/${dirName}/"
        chmod -R u+w "$out/vendor/${dirName}"
      ''
    ) depPackages)}

    # Create unversioned symlinks for convenience
    # Pick highest version when multiple versions exist
    ${lib.concatStringsSep "\n" (
      let
        # Group by crate name
        byName = lib.groupBy (key:
          let parts = lib.splitString "@" key;
          in lib.head parts
        ) (lib.attrNames deps);

        # For each name, create symlink to highest version
        mkSymlink = name: keys:
          let
            # Sort by version (simple string sort works for semver)
            sorted = lib.sort (a: b: a > b) keys;
            highest = lib.head sorted;
          in
          if lib.length keys > 0 && lib.hasInfix "@" highest
          then ''ln -sf "${highest}" "$out/vendor/${name}" 2>/dev/null || true''
          else "";
      in
      lib.mapAttrsToList mkSymlink byName
    )}

    # Compute unified features (if tool provided)
    ${if computeUnifiedFeatures != null then ''
      echo "Computing unified features..."
      UNIFIED_FEATURES=$(compute-unified-features "$out/vendor" ${lib.optionalString (featuresFile != null) "'${builtins.toJSON featuresOverrides}'"})
      export UNIFIED_FEATURES
    '' else ''
      UNIFIED_FEATURES="{}"
      export UNIFIED_FEATURES
    ''}

    # Generate BUCK files (if tool provided)
    ${if genRustBuck != null then ''
      echo "Generating BUCK files..."
      for dir in "$out/vendor"/*; do
        if [ -d "$dir" ] && [ -f "$dir/Cargo.toml" ]; then
          gen-rust-buck "$dir" \
            '${builtins.toJSON (lib.attrNames deps)}' \
            '${builtins.toJSON (lib.attrNames allBuildScriptFixups)}' \
            "$UNIFIED_FEATURES" \
            '${builtins.toJSON rustcFlagsRegistry}' \
            > "$dir/BUCK" || echo "# BUCK generation failed" > "$dir/BUCK"
        fi
      done
    '' else ''
      echo "No gen-rust-buck tool provided, skipping BUCK generation"
    ''}

    # Generate cell .buckconfig
    cat > $out/.buckconfig << 'BUCKCONFIG'
    [cells]
        rustdeps = .
        prelude = prelude

    [buildfile]
        name = BUCK
    BUCKCONFIG
  '';

  # ==========================================================================
  # Internal Helpers
  # ==========================================================================

  # Internal mkDepPackage for generic builder compatibility
  mkDepPackage = { key, depSpec, config, allDeps }:
    let
      parts = lib.splitString "@" key;
      crateName = depSpec.name or (lib.head parts);
      version = depSpec.version;
      patchVersion = lib.last (lib.splitString "." version);

      allFixups = (config.buildScriptFixups or {});
      fixupFn = allFixups.${key} or allFixups.${crateName} or null;
      fixup = if fixupFn != null then
        if builtins.isFunction fixupFn then
          fixupFn { inherit crateName version patchVersion key; vendorPath = "."; }
        else fixupFn
      else null;

      flags = (config.rustcFlagsRegistry or {}).${key} or
              (config.rustcFlagsRegistry or {}).${crateName} or [];
    in
    mkRustDepPackage {
      name = crateName;
      inherit version;
      sha256 = depSpec.hash;
      buildScriptFixup = fixup;
      rustcFlags = flags;
    };

  # Merge commands for generic builder
  mergeCommands = ctx: ''
    # Rust merge commands are handled directly in mkRustDepsCell
  '';
}
