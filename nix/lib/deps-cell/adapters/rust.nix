# Rust Language Adapter for Dependency Cells
#
# Provides:
#   - mkRustDepPackage: Build a single Rust crate package: its source with
#     its fixup applied, and, given its package slice, its own rules.star
#   - mkRustCrates: Every locked crate's package
#   - mkRustDepsCell: The Rust dependency cell's cell index (ADR 0004)
#
# Rust dependencies are fetched from crates.io. Each crate's rules.star is
# generated in its own package, from its package slice alone (ADR 0006);
# tk materialize lays the packages out as the cell the index describes.

{
  pkgs,
  lib,
  genericBuilder,
}:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  platforms = import ../../../buck2/platforms.nix { inherit lib; };
  inherit (genericBuilder) unversionedKeys mkCellIndex;

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

      # The user's patches of this crate (tk compose patch), in order. They
      # name files as a/vendor/<package>/..., and apply after the fixup; one
      # that doesn't apply fails the crate.
      userPatches ? [ ],
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
          nativeBuildInputs =
            buildInputs ++ lib.optional generates rustRulesGen ++ lib.optional (userPatches != [ ]) pkgs.patch;
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
        + lib.concatMapStrings (patchFile: ''

          # The user's patch ${baseNameOf patchFile}
          patch -d $out -p3 --forward --fuzz=0 < ${patchFile} || {
            echo "error: user patch ${baseNameOf patchFile} does not apply to ${name}@${version}"
            echo "  (regenerate it with 'tk compose patch', or remove it)"
            exit 1
          }
        '') userPatches
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

  # The user's patches of each locked crate, keyed "name@version", from
  # <dir>/<cellName>/vendor/<package>/*.patch as tk compose patch writes them
  # (in name order). <package> is a crate's "name@version", or its
  # unversioned name, which resolves as the cell's unversioned alias does.
  userPatchesOf =
    {
      dir,
      cellName,
      keys,
      parseKey,
    }:
    let
      cellDir = dir + "/${cellName}";
      entries = if dir != null && builtins.pathExists cellDir then builtins.readDir cellDir else { };
      flat = lib.filter (name: entries.${name} != "directory") (lib.attrNames entries);
      vendorDir = cellDir + "/vendor";
      packages =
        if entries ? vendor then
          lib.attrNames (lib.filterAttrs (_: type: type == "directory") (builtins.readDir vendorDir))
        else
          [ ];
      aliases = unversionedKeys parseKey keys;
      keyOf =
        package:
        if builtins.elem package keys then
          package
        else
          aliases.${package}
            or (throw "turnkey: user patches under ${cellName}/vendor/${package}/: ${cellName} locks no such package");
      patchesIn =
        package:
        map (name: vendorDir + "/${package}/${name}") (
          lib.sort (a: b: a < b) (
            lib.attrNames (
              lib.filterAttrs (name: type: type == "regular" && lib.hasSuffix ".patch" name) (
                builtins.readDir (vendorDir + "/${package}")
              )
            )
          )
        );
    in
    if flat != [ ] then
      throw "turnkey: user patches for ${cellName} go under ${cellName}/vendor/<package>/, one directory per package (tk compose patch writes them there); move or regenerate: ${lib.concatStringsSep ", " flat}"
    else
      lib.foldl' (
        acc: package:
        let
          key = keyOf package;
        in
        acc // { ${key} = (acc.${key} or [ ]) ++ patchesIn package; }
      ) { } packages;

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
      # The user's patches of each crate, keyed "name@version"
      # (userPatchesOf's)
      userPatches ? { },
    }:
    lib.mapAttrs (
      key: dep:
      mkRustDepPackage (
        {
          name = dep.name or (lib.head (lib.splitString "@" key));
          inherit (dep) version;
          sha256 = dep.hash;
          fixupCommands = resolvedFixups.fixups.${key}.commands or "";
          userPatches = userPatches.${key} or [ ];
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

  # Build the Rust dependency cell (ADR 0004): each locked crate's own
  # package (mkRustCrates), and the cell index that tk materialize keeps
  # .turnkey/<cellName> in line with. The result is the index, with
  # depPackages (each crate's package, keyed "name@version") and index
  # (itself) alongside.
  mkRustDepsCell =
    {
      cellName, # The cell's name (nix/buck2/languages.nix)
      depsFile, # Path to rust-deps.toml

      # The locked crates' fixups: [ { key; name; version; } ] ->
      # { fixups = { <key> = { commands; gen; }; }; unaccounted = { <key> =
      # message; }; } (nix/lib/fixups's resolve): a crate in unaccounted
      # fails its package if it has a build script
      resolveFixups ? (
        _: {
          fixups = { };
          unaccounted = { };
        }
      ),

      # The user's patches (tk compose patch): <dir>/<cellName>/vendor/<package>/
      userPatchesDir ? null,

      # The platforms to build for, and the package of their combined
      # config_settings (nix/buck2/platforms.nix's conditions): the select()s
      # of platform-specific features and deps are keyed on them
      conditions,

      # Generates each crate's rules.star from its package slice
      rustRulesGen,
    }:
    let
      deps = (builtins.fromTOML (builtins.readFile depsFile)).deps or { };
      keys = lib.attrNames deps;

      # A key's crate name (basePath) and version
      parseKey =
        key:
        let
          parts = lib.splitString "@" key;
        in
        {
          basePath = lib.head parts;
          version = if lib.length parts > 1 then lib.elemAt parts 1 else "";
        };

      depPackages = mkRustCrates {
        inherit deps conditions rustRulesGen;
        resolvedFixups = resolveFixups (
          map (key: {
            inherit key;
            name = deps.${key}.name or (parseKey key).basePath;
            inherit (deps.${key}) version;
          }) keys
        );
        userPatches = userPatchesOf {
          dir = userPatchesDir;
          inherit cellName keys parseKey;
        };
      };

      # Each crate's versioned package, and each unversioned name's alias
      # package, forwarding to its highest version
      index = mkCellIndex {
        inherit cellName depsFile;
        packages = lib.mapAttrs' (key: lib.nameValuePair "vendor/${key}") depPackages;
        aliases = lib.mapAttrs' (name: key: lib.nameValuePair "vendor/${name}" "vendor/${key}") (
          unversionedKeys parseKey keys
        );
      };
    in
    index // { inherit depPackages index; };

}
