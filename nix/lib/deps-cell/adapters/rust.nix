# Rust Language Adapter for Dependency Cells
#
# Provides:
#   - mkRustDepPackage: Build a single Rust crate package
#   - mkRustDepsCell: Build a complete Rust dependency cell
#
# Rust dependencies are fetched from crates.io.
# Feature unification and BUCK generation happen during merge phase.

{ pkgs, lib, genericBuilder }:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  fixups = import ../fixups { inherit pkgs lib; };
  inherit (genericBuilder) genericMkDepsCell;
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
    allBuildScriptFixups = (fixups.builtinFixups.rust.buildScriptFixups or {}) // buildScriptFixups;
    allRustcFlags = (fixups.builtinFixups.rust.rustcFlags or {}) // rustcFlagsRegistry;

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
        flags = allRustcFlags.${key} or allRustcFlags.${crateName} or [];
      in
      mkRustDepPackage {
        name = crateName;
        inherit version;
        sha256 = depSpec.hash;
        buildScriptFixup = fixup;
        rustcFlags = flags;
      }
    ) deps;

    # Features file argument for compute-unified-features
    featuresFileArg = if featuresFile != null
      then "${featuresFile}"
      else "";

    # Key to path: Rust keys are already "name@version" format
    keyToPath = key: key;

    # Parse key for symlink: extract crate name (basePath) and version
    parseKeyForSymlink = key:
      let parts = lib.splitString "@" key;
      in {
        basePath = lib.head parts;
        version = if lib.length parts > 1 then lib.elemAt parts 1 else "";
      };

    # Include both versioned and unversioned crate names for gen-rust-buck
    versionedNames = lib.attrNames deps;
    unversionedNames = lib.unique (map (key:
      lib.head (lib.splitString "@" key)
    ) versionedNames);
    allCrateNames = versionedNames ++ unversionedNames;

    # Merge commands: feature unification + BUCK generation
    mergeCommands = ''
      # Compute unified features (if tool provided)
      ${if computeUnifiedFeatures != null then ''
        echo "Computing unified features..."
        UNIFIED_FEATURES=$(compute-unified-features "$out/vendor" ${featuresFileArg})
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
              '${builtins.toJSON allCrateNames}' \
              '${builtins.toJSON (lib.attrNames allBuildScriptFixups)}' \
              "$UNIFIED_FEATURES" \
              '${builtins.toJSON rustcFlagsRegistry}' \
              > "$dir/rules.star" || echo "# rules.star generation failed" > "$dir/rules.star"
          fi
        done
      '' else ''
        echo "No gen-rust-buck tool provided, skipping BUCK generation"
      ''}
    '';
  in
  genericMkDepsCell {
    cellName = "rustdeps";
    inherit depPackages keyToPath parseKeyForSymlink mergeCommands;
    createSymlinks = true;
    cellBuildInputs = cellBuildInputs ++
      (if computeUnifiedFeatures != null then [ computeUnifiedFeatures ] else []) ++
      (if genRustBuck != null then [ genRustBuck ] else []);
  };

}
