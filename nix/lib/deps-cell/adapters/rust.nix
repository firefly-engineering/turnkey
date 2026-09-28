# Rust Language Adapter for Dependency Cells
#
# Provides:
#   - mkRustDepPackage: Build a single Rust crate package: its source with
#     its fixup applied, and, given its package slice, its own rules.star
#   - mkRustDepsCell: Build a complete Rust dependency cell, and (passthru)
#     its cell index (ADR 0004)
#
# Rust dependencies are fetched from crates.io. The merged cell still unifies
# features and generates every rules.star in its merge phase; each package's
# own rules.star, generated from its slice alone, is what the cell index
# points at.

{
  pkgs,
  lib,
  genericBuilder,
}:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  platforms = import ../../../buck2/platforms.nix { inherit lib; };
  inherit (genericBuilder) genericMkDepsCell unversionedKeys mkCellIndex;

  # The platform the cell is built on: what fixups build natively exists
  # only for it
  hostPlatform = builtins.removeAttrs (platforms.fromSystem pkgs.stdenv.hostPlatform.system) [
    "system"
  ];
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

      # Optional, together: generate the crate's rules.star from its package
      # slice (its rust-deps.toml entry's features and dependencies), with
      # rust-rules-gen. It reads nothing of any other crate: its fixup's gen
      # part, the platforms (platforms.nix's conditions) and the platform
      # building it (host, { os; cpu; }). The `targets` output lists the
      # rules.star's target names, one per line.
      slice ? null,
      fixupGen ? { },
      conditions ? null,
      host ? null,
      rustRulesGen ? null,

      # Optional, with a slice: what fails the crate if it has a build
      # script no fixup accounts for (nix/lib/fixups's resolve)
      unaccounted ? null,
    }:
    let
      fetchSpec = fetchers.mkCratesIOSpec {
        crateName = name;
        inherit version sha256;
      };
      generates = slice != null;
    in
    assert lib.assertMsg (
      !generates || (rustRulesGen != null && conditions != null && host != null)
    ) "mkRustDepPackage: a slice needs rustRulesGen, conditions and host";
    pkgs.runCommand "dep-rust-${name}-${version}"
      (
        {
          nativeBuildInputs = buildInputs ++ lib.optional generates rustRulesGen;
          src = fetchers.fetch fetchSpec;
          passthru = {
            inherit name version;
          };
        }
        // lib.optionalAttrs generates {
          outputs = [
            "out"
            "targets"
          ];
          slice = builtins.toJSON slice;
          fixup = builtins.toJSON fixupGen;
          passAsFile = [
            "slice"
            "fixup"
          ];
        }
      )
      (
        ''
          mkdir -p $out
          cp -r $src/* $out/
          chmod -R u+w $out

          # Apply the crate's fixup
          cd $out
          ${fixupCommands}
        ''
        + lib.optionalString generates ''

          # The crate's rules.star, from its slice alone. Buck2 never runs
          # build.rs: it fails if the crate has one no fixup accounts for.
          rust-rules-gen \
            --crate-dir $out \
            --slice "$slicePath" \
            --fixup "$fixupPath" \
            --platforms ${lib.escapeShellArg (builtins.toJSON conditions)} \
            --host ${lib.escapeShellArg (builtins.toJSON host)} \
            --targets-out $targets \
            ${lib.optionalString (unaccounted != null) "--unaccounted ${lib.escapeShellArg unaccounted}"} \
            > $out/rules.star
        ''
      );

  # Each locked crate's package, keyed "name@version": deps is
  # rust-deps.toml's deps table, resolvedFixups nix/lib/fixups's resolve for
  # its crates. With rustRulesGen, each package also generates its
  # rules.star from its own slice, for the platforms in conditions.
  mkRustCrates =
    {
      deps,
      resolvedFixups ? {
        fixups = { };
        unaccounted = { };
      },
      conditions ? null,
      rustRulesGen ? null,
    }:
    lib.mapAttrs (
      key: dep:
      mkRustDepPackage (
        {
          name = dep.name or (lib.head (lib.splitString "@" key));
          inherit (dep) version;
          sha256 = dep.hash;
          fixupCommands = resolvedFixups.fixups.${key}.commands or "";
        }
        // lib.optionalAttrs (rustRulesGen != null) {
          slice = {
            features = dep.features or [ ];
            dependencies = dep.dependencies or [ ];
          };
          fixupGen = resolvedFixups.fixups.${key}.gen or { };
          unaccounted = resolvedFixups.unaccounted.${key} or null;
          inherit conditions rustRulesGen;
          host = hostPlatform;
        }
      )
    ) deps;

  # Build a complete Rust dependency cell
  mkRustDepsCell =
    {
      cellName, # The cell's name (nix/buck2/languages.nix)
      depsFile, # Path to rust-deps.toml
      featuresFile ? null, # Path to rust-features.toml (optional)

      # The locked crates' fixups: [ { key; name; version; } ] ->
      # { fixups = { <key> = { commands; gen; }; }; unaccounted = { <key> =
      # message; }; } (nix/lib/fixups's resolve): a crate in unaccounted
      # fails the cell if it has a build script
      resolveFixups ? (
        _: {
          fixups = { };
          unaccounted = { };
        }
      ),

      # User patches (from FUSE edit layer)
      userPatchesDir ? null, # Path to .turnkey/patches directory

      # The platforms to build for, and the package of their combined
      # config_settings (nix/buck2/platforms.nix's conditions): the select()s
      # of target-specific deps are keyed on them
      conditions,

      # Tools (must be provided by caller)
      computeUnifiedFeatures ? null, # Tool for feature unification
      genRustBuck ? null, # Tool for BUCK generation
      # Generates each crate's own rules.star from its slice, for the cell
      # index (passthru.index), when given
      rustRulesGen ? null,
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
      resolvedFixups = resolveFixups locked;
      inherit (resolvedFixups) fixups;

      # The crates no fixup accounts for the build script of, and what the
      # cell fails with if they have one
      unaccountedFile = pkgs.writeText "${cellName}-unaccounted-build-scripts.json" (
        builtins.toJSON resolvedFixups.unaccounted
      );

      # Build individual dep packages
      depPackages = mkRustCrates {
        inherit
          deps
          resolvedFixups
          conditions
          rustRulesGen
          ;
      };

      # What gen-rust-buck reads of each crate's fixup
      fixupsFile = pkgs.writeText "${cellName}-fixups.json" (
        builtins.toJSON (lib.mapAttrs (_: fixup: fixup.gen) fixups)
      );

      # The platform the cell is built on: what fixups build natively
      # exists only for it
      hostJSON = builtins.toJSON hostPlatform;

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

      # Merge commands: every build script accounted for, feature
      # unification, BUCK generation
      mergeCommands = ''
        # Every crate with a build script needs a fixup saying what stands
        # in for it: Buck2 never runs build.rs
        python3 ${./rust-build-scripts.py} "$out/vendor" ${unaccountedFile}

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
      # The cell index: each package's own derivation, with the rules.star
      # generated from its slice, and the unversioned alias packages, as the
      # merged cell's unversioned symlinks choose them
      passthru = lib.optionalAttrs (rustRulesGen != null) {
        index = mkCellIndex {
          inherit cellName depsFile;
          packages = lib.mapAttrs' (key: lib.nameValuePair "vendor/${keyToPath key}") depPackages;
          aliases = lib.mapAttrs' (name: key: lib.nameValuePair "vendor/${name}" "vendor/${keyToPath key}") (
            unversionedKeys parseKeyForSymlink (lib.attrNames depPackages)
          );
        };
      };
    };

}
