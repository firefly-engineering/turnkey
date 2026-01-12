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
# Build script fixups:
# Some crates have build scripts that generate files needed at compile time.
# We handle these by pre-generating the output in Nix.

{ pkgs, lib, depsFile }:

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

  # Script to generate BUCK files by parsing Cargo.toml
  genBuckScript = ./gen-rust-buck.py;

  # JSON list of all available crate names for dependency resolution
  availableCratesJson = builtins.toJSON (lib.attrNames cratesByName);

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

  # Generate shell commands to set up one crate
  # key is "name@version", dep contains crateName, version, src
  setupCrate = key: dep:
    let
      # Use key (name@version) as directory to support multiple versions
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

      # Generate BUCK file by parsing Cargo.toml
      ${pkgs.python3}/bin/python3 ${genBuckScript} \
        "$out/${vendorPath}" \
        '${availableCratesJson}' \
        '${fixupCratesJson}' \
        > "$out/${vendorPath}/BUCK"
    '';

  # All setup commands
  allSetupCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList setupCrate registry
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

in
pkgs.runCommand "rust-deps-cell" {} ''
  mkdir -p $out/vendor

  # Set up all crate sources and generate BUCK files
  ${allSetupCommands}

  # Create symlinks for unversioned crate references
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

  echo "Generated rustdeps cell v2 with ${toString (lib.length (lib.attrNames registry))} crates (with build script fixups)"
''
