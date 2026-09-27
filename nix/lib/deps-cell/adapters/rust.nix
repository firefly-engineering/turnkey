# Rust Language Adapter for Dependency Cells
#
# Provides:
#   - mkRustDepPackage: Build a single Rust crate package
#   - mkRustDepsCell: Build a complete Rust dependency cell
#
# Rust dependencies are fetched from crates.io.
# Feature unification and BUCK generation happen during merge phase.

{
  pkgs,
  lib,
  genericBuilder,
}:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  platforms = import ../../../buck2/platforms.nix { inherit lib; };
  inherit (genericBuilder) genericMkDepsCell;
in
rec {
  # Build inputs for per-dependency builds
  buildInputs = with pkgs; [
    stdenv.cc
    perl
  ];

  # Build inputs for cell builds
  cellBuildInputs = with pkgs; [ python3 ];

  # ==========================================================================
  # Public API
  # ==========================================================================

  # Build a single Rust crate package
  mkRustDepPackage =
    {
      name, # Crate name (e.g., "serde")
      version, # Version string (e.g., "1.0.219")
      sha256, # SRI hash of the source

      # Optional: the crate's fixup's commands (nix/lib/fixups): its
      # patches, then what stands in for its build.rs
      fixupCommands ? "",
    }:
    let
      fetchSpec = fetchers.mkCratesIOSpec {
        crateName = name;
        inherit version sha256;
      };
    in
    pkgs.runCommand "dep-rust-${name}-${version}"
      {
        nativeBuildInputs = buildInputs;
        src = fetchers.fetch fetchSpec;
        passthru = {
          inherit name version;
        };
      }
      ''
        mkdir -p $out
        cp -r $src/* $out/
        chmod -R u+w $out

        # Apply the crate's fixup
        cd $out
        ${fixupCommands}
      '';

  # Build a complete Rust dependency cell
  mkRustDepsCell =
    {
      cellName, # The cell's name (nix/buck2/languages.nix)
      depsFile, # Path to rust-deps.toml
      featuresFile ? null, # Path to rust-features.toml (optional)

      # The locked crates' fixups: [ { key; name; version; } ] ->
      # { <key> = { commands; gen; }; } (nix/lib/fixups's resolve)
      resolveFixups ? (_: { }),

      # User patches (from FUSE edit layer)
      userPatchesDir ? null, # Path to .turnkey/patches directory

      # The platforms to build for, and the package of their combined
      # config_settings (nix/buck2/platforms.nix's conditions): the select()s
      # of target-specific deps are keyed on them
      conditions,

      # Tools (must be provided by caller)
      computeUnifiedFeatures ? null, # Tool for feature unification
      genRustBuck ? null, # Tool for BUCK generation
    }:
    let
      depsToml = builtins.fromTOML (builtins.readFile depsFile);
      deps = depsToml.deps or { };

      # The locked crates, and their fixups
      locked = lib.mapAttrsToList (key: depSpec: {
        inherit key;
        name = depSpec.name or (lib.head (lib.splitString "@" key));
        inherit (depSpec) version;
      }) deps;
      fixups = resolveFixups locked;

      # Build individual dep packages
      depPackages = lib.listToAttrs (
        map (
          crate:
          lib.nameValuePair crate.key (mkRustDepPackage {
            inherit (crate) name version;
            sha256 = deps.${crate.key}.hash;
            fixupCommands = fixups.${crate.key}.commands or "";
          })
        ) locked
      );

      # What gen-rust-buck reads of each crate's fixup
      fixupsFile = pkgs.writeText "${cellName}-fixups.json" (
        builtins.toJSON (lib.mapAttrs (_: fixup: fixup.gen) fixups)
      );

      # The platform the cell is built on: what fixups build natively
      # exists only for it
      hostJSON = builtins.toJSON (
        builtins.removeAttrs (platforms.fromSystem pkgs.stdenv.hostPlatform.system) [ "system" ]
      );

      # Features file argument for compute-unified-features
      featuresFileArg = if featuresFile != null then "${featuresFile}" else "";

      # Key to path: Rust keys are already "name@version" format
      keyToPath = key: key;

      # Parse key for symlink: extract crate name (basePath) and version
      parseKeyForSymlink =
        key:
        let
          parts = lib.splitString "@" key;
        in
        {
          basePath = lib.head parts;
          version = if lib.length parts > 1 then lib.elemAt parts 1 else "";
        };

      # Include both versioned and unversioned crate names for gen-rust-buck
      versionedNames = lib.attrNames deps;
      unversionedNames = lib.unique (map (key: lib.head (lib.splitString "@" key)) versionedNames);
      allCrateNames = versionedNames ++ unversionedNames;

      conditionsJSON = builtins.toJSON conditions;

      # Merge commands: feature unification + BUCK generation
      mergeCommands = ''
        # Compute unified features (if tool provided)
        ${
          if computeUnifiedFeatures != null then
            ''
              echo "Computing unified features..."
              UNIFIED_FEATURES=$(compute-unified-features "$out/vendor" ${featuresFileArg} --deps-file ${depsFile} --platforms '${conditionsJSON}')
              export UNIFIED_FEATURES
            ''
          else
            ''
              UNIFIED_FEATURES="{}"
              export UNIFIED_FEATURES
            ''
        }

        # Generate BUCK files (if tool provided)
        ${
          if genRustBuck != null then
            ''
              echo "Generating BUCK files..."
              for dir in "$out/vendor"/*; do
                if [ -d "$dir" ] && [ -f "$dir/Cargo.toml" ]; then
                  gen-rust-buck "$dir" \
                    '${builtins.toJSON allCrateNames}' \
                    ${fixupsFile} \
                    "$UNIFIED_FEATURES" \
                    '${conditionsJSON}' \
                    '${hostJSON}' \
                    > "$dir/rules.star" || echo "# rules.star generation failed" > "$dir/rules.star"
                fi
              done
            ''
          else
            ''
              echo "No gen-rust-buck tool provided, skipping BUCK generation"
            ''
        }
      '';
    in
    genericMkDepsCell {
      inherit
        cellName
        depPackages
        keyToPath
        parseKeyForSymlink
        mergeCommands
        userPatchesDir
        ;
      createSymlinks = true;
      cellBuildInputs =
        cellBuildInputs
        ++ (if computeUnifiedFeatures != null then [ computeUnifiedFeatures ] else [ ])
        ++ (if genRustBuck != null then [ genRustBuck ] else [ ]);
    };

}
