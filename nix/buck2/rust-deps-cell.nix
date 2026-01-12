# Rust dependencies cell builder
#
# Reads a rust-deps.toml file and builds a Buck2 cell containing
# all crate dependencies with BUCK files for rust_library targets.
#
# The TOML file format (supports multiple versions of same crate):
#   [deps."crate-name@1.0.0"]
#   name = "crate-name"
#   version = "1.0.0"
#   hash = "sha256-..."
#   features = ["feature1", "feature2"]  # optional
#
# This allows downstream repos to declare deps in pure data files.
#
# Feature unification:
# Features are unified across the dependency graph, matching Cargo's behavior.
# If any crate requires feature X on crate Y, crate Y is built with feature X.
#
# Manual overrides can be specified in an optional featuresFile (rust-features.toml).
#
# Build script fixups:
# Some crates have build scripts that generate files needed at compile time.
# We handle these by pre-generating the output in Nix.

{ pkgs, lib, depsFile, featuresFile ? null }:

let
  # Import semver utilities
  semver = import ../lib/semver.nix { inherit lib; };

  # Parse the TOML file
  depsToml = builtins.fromTOML (builtins.readFile depsFile);

  # Convert TOML deps to registry format
  # Key is "name@version", value contains name, version, hash
  registry = lib.mapAttrs (key: dep: {
    # Use explicit name field, fallback to parsing key for backwards compat
    crateName = dep.name or (lib.head (lib.splitString "@" key));
    inherit (dep) version;
    features = dep.features or [];
    src = fetchCrate (dep.name or (lib.head (lib.splitString "@" key))) dep;
  }) (depsToml.deps or {});

  # Fetch crate from crates.io
  fetchCrate = crateName: dep:
    pkgs.fetchzip {
      url = "https://crates.io/api/v1/crates/${crateName}/${dep.version}/download";
      sha256 = dep.hash;
      extension = "tar.gz";
    };

  # Scripts for BUCK file generation
  genBuckScript = ./gen-rust-buck.py;
  computeFeaturesScript = ./compute-unified-features.py;

  # JSON list of all available crate names for dependency resolution
  # Includes both versioned keys (e.g., "getrandom@0.2.17") and unversioned names
  # This allows version-aware dependency resolution
  availableCratesJson = builtins.toJSON (
    (lib.attrNames cratesByName) ++  # unversioned names for symlinks
    (lib.attrNames registry)         # versioned keys for precise matching
  );

  # ==========================================================================
  # Build script fixups
  # ==========================================================================
  # Some crates have build scripts that generate files. We pre-generate these
  # in Nix to avoid needing to run build scripts at Buck2 build time.

  # Generate fixup commands for a specific crate
  # Returns empty string if no fixup needed
  getFixupCommands = key: dep:
    let
      crateName = dep.crateName;
      version = dep.version;
      patchVersion = lib.last (lib.splitString "." version);
      vendorPath = "vendor/${key}";

      # serde_core's private.rs - just the versioned module
      serdeCorePrivateRs = ''
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
      '';

      # serde's private.rs - versioned module PLUS the serde_core_private alias
      serdePrivateRs = ''
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
use serde_core::__private${patchVersion} as serde_core_private;
      '';
    in
    if crateName == "serde_core" then ''
      # Fixup: serde_core build script output
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'SERDE_CORE_PRIVATE'
${serdeCorePrivateRs}
SERDE_CORE_PRIVATE
    ''
    else if crateName == "serde" then ''
      # Fixup: serde build script output (includes serde_core_private alias)
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'SERDE_PRIVATE'
${serdePrivateRs}
SERDE_PRIVATE
    ''
    else "";

  # Check if a crate needs build script fixups
  needsFixup = crateName:
    crateName == "serde_core" || crateName == "serde";

  # JSON map of crates that need OUT_DIR set (for gen-rust-buck.py)
  fixupCratesJson = builtins.toJSON (lib.filter needsFixup (lib.attrNames cratesByName));

  # ==========================================================================
  # Crate setup (Phase 1: copy sources and apply fixups)
  # ==========================================================================

  # Generate shell commands to set up one crate's sources (no BUCK file yet)
  setupCrateSources = key: dep:
    let
      vendorPath = "vendor/${key}";
      fixupCmds = getFixupCommands key dep;
    in
    ''
      # Set up ${key}
      mkdir -p $out/${vendorPath}
      cp -r ${dep.src}/* $out/${vendorPath}/
      chmod -R u+w $out/${vendorPath}

      # Apply fixups (if any)
      ${fixupCmds}
    '';

  # All source setup commands
  allSourceSetupCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList setupCrateSources registry
  );

  # ==========================================================================
  # BUCK file generation (Phase 2: after feature unification)
  # ==========================================================================

  # Generate BUCK file for one crate using unified features
  generateBuckFile = key: dep:
    let
      vendorPath = "vendor/${key}";
    in
    ''
      # Generate BUCK file for ${key}
      ${pkgs.python3}/bin/python3 ${genBuckScript} \
        "$out/${vendorPath}" \
        '${availableCratesJson}' \
        '${fixupCratesJson}' \
        "$UNIFIED_FEATURES" \
        > "$out/${vendorPath}/BUCK"
    '';

  # All BUCK generation commands
  allBuckGenCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList generateBuckFile registry
  );

  # ==========================================================================
  # Symlink generation with proper version selection
  # ==========================================================================

  # Group crates by unversioned name to create symlinks
  # This allows users to reference crates without version suffix
  cratesByName = lib.foldlAttrs (acc: key: dep:
    let
      name = dep.crateName;
      existing = acc.${name} or [];
    in
    acc // { ${name} = existing ++ [{ inherit key; version = dep.version; }]; }
  ) {} registry;

  # Generate symlink commands for unversioned references
  # When multiple versions exist, sort by semver and pick the greatest
  symlinkCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList (name: versions:
      let
        # Sort versions by semver descending (greatest first)
        sorted = lib.sort semver.sortDesc versions;
        # Pick the greatest version
        target = (lib.head sorted).key;
      in
      ''
        # Symlink ${name} -> ${target}
        ln -s "${target}" "$out/vendor/${name}"
      ''
    ) cratesByName
  );

  # Optional features file handling
  featuresFileArg = if featuresFile != null && builtins.pathExists featuresFile
    then "${featuresFile}"
    else "";

in
pkgs.runCommand "rust-deps-cell" {} ''
  mkdir -p $out/vendor

  # ==========================================================================
  # Phase 1: Set up all crate sources and apply fixups
  # ==========================================================================
  ${allSourceSetupCommands}

  # ==========================================================================
  # Phase 2: Compute unified features across all crates
  # ==========================================================================
  echo "Computing unified features..."
  UNIFIED_FEATURES=$(${pkgs.python3}/bin/python3 ${computeFeaturesScript} \
    "$out/vendor" \
    ${featuresFileArg})

  # ==========================================================================
  # Phase 3: Generate BUCK files with unified features
  # ==========================================================================
  echo "Generating BUCK files with unified features..."
  ${allBuckGenCommands}

  # ==========================================================================
  # Phase 4: Create symlinks for unversioned crate references
  # ==========================================================================
  # Users can reference "rustdeps//vendor/itoa:itoa" instead of "rustdeps//vendor/itoa@1.0.17:itoa"
  ${symlinkCommands}

  # Generate cell .buckconfig
  cat > $out/.buckconfig << 'BUCKCONFIG'
  [cells]
      rustdeps = .
      prelude = prelude

  [buildfile]
      name = BUCK
  BUCKCONFIG

  echo "Generated rustdeps cell with ${toString (lib.length (lib.attrNames registry))} crates (with feature unification)"
''
