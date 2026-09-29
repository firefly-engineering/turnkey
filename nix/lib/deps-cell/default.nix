# Unified Dependency Cell Library
#
# Builds dependency cells. Each dependency becomes an individual Nix package,
# enabling deduplication across cell generations; genericMkDepsCell merges
# them into the cell, and each language adapter (./adapters) builds its
# packages and calls it.
#
# Usage:
#   let depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
#   in depsCell.mkRustDepsCell {
#     cellName = "rustdeps";
#     depsFile = ./rust-deps.toml;
#     conditions = (import ../../buck2/platforms.nix { inherit lib; }).conditions [ "x86_64-linux" ];
#   }

{ pkgs, lib }:

let
  # Import sub-modules
  fetchers = import ./fetchers.nix { inherit pkgs lib; };

  # Import adapters with access to generic builder (see below)
  mkAdapters =
    genericBuilder:
    import ./adapters {
      inherit pkgs lib genericBuilder;
    };

  # Each unversioned name's package: for keys grouped by parseKey's
  # basePath, the key with the highest version (builtins.compareVersions:
  # 1.0.100 is higher than 1.0.99).
  # { <basePath> = <key>; }. The cell's unversioned symlinks and the cell
  # index's unversioned alias packages both follow it.
  unversionedKeys =
    parseKey: keys:
    let
      parsed = lib.genAttrs keys parseKey;
      byBasePath = lib.groupBy (key: parsed.${key}.basePath) keys;
      highest =
        group:
        let
          highestVersion = lib.head (
            lib.sort (a: b: builtins.compareVersions a b > 0) (map (key: parsed.${key}.version) group)
          );
        in
        lib.findFirst (key: parsed.${key}.version == highestVersion) (lib.head group) group;
    in
    lib.mapAttrs (_: highest) byBasePath;

  # A deps cell's .buckconfig
  cellBuckconfig = cellName: ''
    [cells]
        ${cellName} = .
        prelude = prelude

    [buildfile]
        name = rules.star
  '';

  # A deps cell's cell index (ADR 0004): what the materializer brings the
  # cell's directory in line with. packages maps each package's path in the
  # cell to its derivation, whose `targets` output lists its target names;
  # aliases maps each alias package's path to the package it forwards to.
  # It records the deps file's content hash, which tk compares with the
  # file on disk.
  mkCellIndex =
    {
      cellName,
      depsFile,
      packages,
      aliases,
    }:
    pkgs.runCommand "${cellName}-index.json"
      {
        nativeBuildInputs = [ pkgs.python3 ];
        spec = builtins.toJSON {
          cell = cellName;
          deps_file_sha256 = builtins.hashFile "sha256" depsFile;
          buckconfig = cellBuckconfig cellName;
          packages = lib.mapAttrs (_: package: {
            store = "${package}";
            targets = "${package.targets}";
          }) packages;
          inherit aliases;
        };
        passAsFile = [ "spec" ];
      }
      ''
        python3 ${./cell-index.py} "$specPath" > $out
      '';

  # Generic cell builder - the core reusable function
  genericMkDepsCell =
    {
      cellName, # "godeps", "rustdeps", etc.
      depPackages, # { key -> derivation } - pre-built by adapter

      # Directory structure options
      keyToPath ? (key: key), # key -> vendor subdirectory path
      createSymlinks ? false, # Create unversioned symlinks
      parseKeyForSymlink ? null, # key -> { basePath, version } for symlink grouping

      # User patches (from FUSE edit layer)
      userPatchesDir ? null, # Path to .turnkey/patches directory

      # Merge phase
      mergeCommands ? "", # Shell commands after copy
      cellBuildInputs ? [ ], # Build inputs for merge phase
      rootBuckContent ? null, # Optional content for root rules.star

      # Passthru
      passthru ? { },
    }:
    let
      # Generate symlink creation commands
      symlinkCommands =
        if createSymlinks && parseKeyForSymlink != null then
          let
            # Each basePath's symlink points at its highest version
            mkSymlink =
              basePath: highestKey:
              let
                targetPath = keyToPath highestKey;
                # Get parent directory path for mkdir
                parentDir = lib.concatStringsSep "/" (lib.init (lib.splitString "/" basePath));
                # Get relative path from basePath to targetPath
                # For simple cases like "serde" -> "serde@1.0.219", just use the target name
                baseDepth = lib.length (lib.splitString "/" basePath);
                targetName = lib.last (lib.splitString "/" targetPath);
              in
              ''
                # Create parent directories for symlink
                ${if parentDir != "" then ''mkdir -p "$out/vendor/${parentDir}"'' else ""}
                # Create symlink: ${basePath} -> ${targetPath}
                ln -sfn "${targetName}" "$out/vendor/${basePath}"
              '';
          in
          lib.concatStringsSep "\n" (
            lib.mapAttrsToList mkSymlink (unversionedKeys parseKeyForSymlink (lib.attrNames depPackages))
          )
        else
          "";
    in
    pkgs.runCommand "${cellName}-cell"
      {
        nativeBuildInputs = cellBuildInputs ++ [ pkgs.patch ];
        passthru = {
          inherit depPackages;
        }
        // passthru;
      }
      ''
        mkdir -p $out/vendor

        # Copy each dep package into vendor/
        ${lib.concatStringsSep "\n" (
          lib.mapAttrsToList (
            key: pkg:
            let
              dirPath = keyToPath key;
            in
            ''
              mkdir -p "$out/vendor/${dirPath}"
              cp -r ${pkg}/* "$out/vendor/${dirPath}/"
              chmod -R u+w "$out/vendor/${dirPath}"
            ''
          ) depPackages
        )}

        # Create symlinks (if enabled)
        ${symlinkCommands}

        # Apply user patches from FUSE edit layer
        # Patches are in .turnkey/patches/<cellName>/*.patch format
        # Patch files use a/vendor/... and b/vendor/... paths, so we use -p1
        ${
          if userPatchesDir != null then
            ''
              patchDir="${userPatchesDir}/${cellName}"
              if [ -d "$patchDir" ]; then
                echo "Applying user patches from $patchDir"
                for patchFile in "$patchDir"/*.patch; do
                  if [ -f "$patchFile" ]; then
                    echo "  Applying: $(basename "$patchFile")"
                    # Use -p1 to strip the a/ or b/ prefix from patch paths.
                    # A patch that doesn't apply fails the cell: building it
                    # without the change the user asked for would be wrong.
                    patch -d "$out" -p1 --forward < "$patchFile" || {
                      echo "error: user patch $(basename "$patchFile") does not apply to the ${cellName} cell"
                      echo "  (from $patchDir; regenerate it with 'tk compose patch' or remove it)"
                      exit 1
                    }
                  fi
                done
              fi
            ''
          else
            ""
        }

        # Run language-specific merge commands
        ${mergeCommands}

        # Generate root rules.star (if provided)
        ${
          if rootBuckContent != null then
            ''
              cat > $out/rules.star << 'ROOTRULES'
              ${rootBuckContent}
              ROOTRULES
            ''
          else
            ""
        }

        # Generate cell .buckconfig
        cp ${pkgs.writeText "${cellName}-buckconfig" (cellBuckconfig cellName)} $out/.buckconfig
      '';

  # Build generic builder for adapters
  genericBuilder = {
    inherit
      genericMkDepsCell
      unversionedKeys
      cellBuckconfig
      mkCellIndex
      ;
  };

  # Create adapters with access to generic builder
  adapters = mkAdapters genericBuilder;

in
rec {
  # Export sub-modules
  inherit fetchers adapters;

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
  inherit (adapters.solidity) mkSolDepPackage mkSolDepsCell;
}
