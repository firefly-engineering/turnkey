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

  # Import adapters with access to generic builder (see below)
  mkAdapters = genericBuilder: import ./adapters {
    inherit pkgs lib genericBuilder;
  };

  # Extend lib with our functions for internal use
  libWithDepsCell = lib // {
    deps-cell = {
      inherit phases hooks fetchers fixups;
    };
  };

  # Re-import with extended lib
  phasesExt = import ./phases.nix { lib = libWithDepsCell; };
  hooksExt = import ./hooks.nix { lib = libWithDepsCell; };

  # Generic cell builder - the core reusable function
  genericMkDepsCell = {
    cellName,                          # "godeps", "rustdeps", etc.
    depPackages,                       # { key -> derivation } - pre-built by adapter

    # Directory structure options
    keyToPath ? (key: key),            # key -> vendor subdirectory path
    createSymlinks ? false,            # Create unversioned symlinks
    parseKeyForSymlink ? null,         # key -> { basePath, version } for symlink grouping

    # User patches (from FUSE edit layer)
    userPatchesDir ? null,             # Path to .turnkey/patches directory

    # Merge phase
    mergeCommands ? "",                # Shell commands after copy
    cellBuildInputs ? [],              # Build inputs for merge phase
    rootBuckContent ? null,            # Optional content for root rules.star

    # Passthru
    passthru ? {},
  }:
  let
    # Generate symlink creation commands
    symlinkCommands = if createSymlinks && parseKeyForSymlink != null then
      let
        # Parse all keys to get basePath and version
        parsedKeys = lib.mapAttrs (key: _: parseKeyForSymlink key) depPackages;

        # Group keys by basePath
        byBasePath = lib.groupBy (key: (parsedKeys.${key}).basePath) (lib.attrNames depPackages);

        # For each basePath, find highest version and create symlink
        mkSymlink = basePath: keys:
          let
            versions = map (key: (parsedKeys.${key}).version) keys;
            # Sort versions descending (simple string sort works for semver)
            sortedVersions = lib.sort (a: b: a > b) versions;
            highestVersion = lib.head sortedVersions;
            # Find the key with the highest version
            highestKey = lib.findFirst (key: (parsedKeys.${key}).version == highestVersion) (lib.head keys) keys;
            targetPath = keyToPath highestKey;
            # Get parent directory path for mkdir
            parentDir = lib.concatStringsSep "/" (lib.init (lib.splitString "/" basePath));
            # Get relative path from basePath to targetPath
            # For simple cases like "serde" -> "serde@1.0.219", just use the target name
            baseDepth = lib.length (lib.splitString "/" basePath);
            targetName = lib.last (lib.splitString "/" targetPath);
          in ''
            # Create parent directories for symlink
            ${if parentDir != "" then ''mkdir -p "$out/vendor/${parentDir}"'' else ""}
            # Create symlink: ${basePath} -> ${targetPath}
            ln -sfn "${targetName}" "$out/vendor/${basePath}"
          '';
      in
      lib.concatStringsSep "\n" (lib.mapAttrsToList mkSymlink byBasePath)
    else "";
  in
  pkgs.runCommand "${cellName}-cell" {
    nativeBuildInputs = cellBuildInputs ++ [ pkgs.patch ];
    passthru = { inherit depPackages; } // passthru;
  } ''
    mkdir -p $out/vendor

    # Copy each dep package into vendor/
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (key: pkg:
      let
        dirPath = keyToPath key;
      in ''
        mkdir -p "$out/vendor/${dirPath}"
        cp -r ${pkg}/* "$out/vendor/${dirPath}/"
        chmod -R u+w "$out/vendor/${dirPath}"
      ''
    ) depPackages)}

    # Create symlinks (if enabled)
    ${symlinkCommands}

    # Apply user patches from FUSE edit layer
    # Patches are in .turnkey/patches/<cellName>/*.patch format
    # Patch files use a/vendor/... and b/vendor/... paths, so we use -p1
    ${if userPatchesDir != null then ''
      patchDir="${userPatchesDir}/${cellName}"
      if [ -d "$patchDir" ]; then
        echo "Applying user patches from $patchDir"
        for patchFile in "$patchDir"/*.patch; do
          if [ -f "$patchFile" ]; then
            echo "  Applying: $(basename "$patchFile")"
            # Use -p1 to strip the a/ or b/ prefix from patch paths
            patch -d "$out" -p1 < "$patchFile" || {
              echo "Warning: Failed to apply patch: $patchFile"
              echo "Continuing anyway..."
            }
          fi
        done
      fi
    '' else ""}

    # Run language-specific merge commands
    ${mergeCommands}

    # Generate root rules.star (if provided)
    ${if rootBuckContent != null then ''
      cat > $out/rules.star << 'ROOTRULES'
      ${rootBuckContent}
      ROOTRULES
    '' else ""}

    # Generate cell .buckconfig
    cat > $out/.buckconfig << 'BUCKCONFIG'
    [cells]
        ${cellName} = .
        prelude = prelude

    [buildfile]
        name = rules.star
    BUCKCONFIG
  '';

  # Build generic builder for adapters
  genericBuilder = {
    inherit genericMkDepsCell;
  };

  # Create adapters with access to generic builder
  adapters = mkAdapters genericBuilder;

in
rec {
  # Export sub-modules
  inherit phases hooks fetchers fixups adapters;

  # Export generic cell builder for direct use
  mkDepsCell = genericMkDepsCell;

  # ==========================================================================
  # Language-Specific Builders (Public API)
  # ==========================================================================

  # Re-export language-specific builders from adapters
  inherit (adapters.go) mkGoDepPackage mkGoDepsCell;
  inherit (adapters.rust) mkRustDepPackage mkRustDepsCell;
  inherit (adapters.python) mkPythonDepPackage mkPythonDepsCell;
  inherit (adapters.javascript) mkJsDepPackage mkJsDepsCell;
}
