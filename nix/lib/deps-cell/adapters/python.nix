# Python Language Adapter for Dependency Cells
#
# Provides:
#   - mkPythonDepPackage: Build a single Python distribution's package: its
#     source with its fixup and user patches applied, and its own rules.star
#   - mkPythonDepsCell: The Python dependency cell's cell index (ADR 0004)
#
# Python dependencies are fetched from PyPI. Each distribution's rules.star
# is generated in its own package, from its package slice alone (ADR 0010);
# tk materialize lays the packages out as the cell the index describes.

{
  pkgs,
  lib,
  genericBuilder,
}:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  inherit (genericBuilder) mkCellIndex;
in
rec {
  # Build inputs for per-dependency builds
  buildInputs = [ ];

  # ==========================================================================
  # Public API
  # ==========================================================================

  # Build a single Python distribution's package
  mkPythonDepPackage =
    {
      name, # The distribution's key in python-deps.toml (e.g., "requests")
      version, # Version string (e.g., "2.31.0")
      sha256, # SRI hash of the source
      url, # URL to fetch from

      # Its package slice (mkPythonDepsCell's sliceOf): its dependencies,
      # its requested extras and theirs, narrowed to the distributions the
      # cell holds. pydeps-cell writes its rules.star from it, the platforms
      # (platforms.nix's conditions) and the Python toolchain's version,
      # reading nothing of any other distribution. The `targets` output
      # lists the rules.star's target name.
      slice,
      conditions,
      pydepsCell,
      pythonVersion ? pkgs.python3.version,

      # Optional
      namespace ? false, # Whether this is a namespace package
      fixup ? null, # Custom fixup commands

      # The user's patches of this distribution (tk compose patch), in
      # order. They name files as a/vendor/<name>/..., and apply after the
      # fixup; one that doesn't apply fails the distribution.
      userPatches ? [ ],
    }:
    let
      fetchSpec = fetchers.mkPyPISpec {
        inherit url sha256;
      };
    in
    pkgs.runCommand "dep-python-${name}-${version}"
      {
        nativeBuildInputs = buildInputs ++ [ pydepsCell ] ++ lib.optional (userPatches != [ ]) pkgs.patch;
        src = fetchers.fetch fetchSpec;
        passthru = { inherit name version; };
        outputs = [
          "out"
          "targets"
        ];
        slice = builtins.toJSON slice;
        passAsFile = [ "slice" ];
      }
      (
        ''
          mkdir -p $out
          cp -r $src/* $out/
          chmod -R u+w $out

          # Apply fixup if provided
          cd $out
          ${if fixup != null then fixup else ""}
        ''
        + lib.concatMapStrings (patchFile: ''

          # The user's patch ${baseNameOf patchFile}
          patch -d $out -p3 --forward --fuzz=0 < ${patchFile} || {
            echo "error: user patch ${baseNameOf patchFile} does not apply to ${name}@${version}"
            echo "  (regenerate it with 'tk compose patch', or remove it)"
            exit 1
          }
        '') userPatches
        + ''

          # The distribution's rules.star, from its slice alone
          pydeps-cell \
            --name ${lib.escapeShellArg name} \
            --slice "$slicePath" \
            --platforms ${lib.escapeShellArg (builtins.toJSON conditions)} \
            --python-version ${lib.escapeShellArg pythonVersion} \
            --targets-out $targets \
            > $out/rules.star
        ''
      );

  # The user's patches of each locked distribution, keyed by its name in
  # the cell, from <dir>/<cellName>/vendor/<name>/*.patch as tk compose
  # patch writes them (in name order). A directory holding patches must be
  # a locked distribution's.
  userPatchesOf =
    {
      dir,
      cellName,
      names,
    }:
    let
      cellDir = dir + "/${cellName}";
      entries = if dir != null && builtins.pathExists cellDir then builtins.readDir cellDir else { };
      flat = lib.filter (name: entries.${name} != "directory") (lib.attrNames entries);
      vendorDir = cellDir + "/vendor";
      packages =
        if (entries.vendor or null) == "directory" then
          lib.attrNames (lib.filterAttrs (_: type: type == "directory") (builtins.readDir vendorDir))
        else
          [ ];
      unknown = lib.filter (package: !(builtins.elem package names)) packages;
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
      throw "turnkey: user patches for ${cellName} go under ${cellName}/vendor/<name>/, one directory per distribution (tk compose patch writes them there); move or regenerate: ${lib.concatStringsSep ", " flat}"
    else if unknown != [ ] then
      throw "turnkey: user patches under ${cellName}/vendor/${lib.head unknown}/: ${cellName} locks no such distribution"
    else
      lib.filterAttrs (_: patches: patches != [ ]) (lib.genAttrs packages patchesIn);

  # A distribution's package slice (ADR 0010), from its python-deps.toml
  # entry: its dependencies, the extras it is asked for and their
  # dependencies, each dependency's name and marker. Dependencies on
  # distributions the cell doesn't hold (deps, python-deps.toml's table) are
  # left out, so the slice changes only when what its rules.star can name
  # does.
  sliceOf =
    deps: dep:
    let
      edges =
        list:
        map (
          edge: { inherit (edge) name; } // lib.optionalAttrs (edge ? marker) { inherit (edge) marker; }
        ) (builtins.filter (edge: deps ? ${edge.name}) list);
      requested = dep.requested_extras or [ ];
      extras = dep.extras or { };
    in
    {
      dependencies = edges (dep.dependencies or [ ]);
      requested_extras = requested;
      extras = lib.genAttrs (builtins.filter (extra: extras ? ${extra}) requested) (
        extra: edges extras.${extra}
      );
    };

  # Build the Python dependency cell (ADR 0004, ADR 0010): each locked
  # distribution's own package, and the cell index that tk materialize keeps
  # .turnkey/<cellName> in line with: one package per distribution, at
  # vendor/<name>. The result is the index, with depPackages (each
  # distribution's package, keyed by name) and index (itself) alongside.
  mkPythonDepsCell =
    {
      cellName, # The cell's name (nix/buck2/languages.nix)
      depsFile, # Path to python-deps.toml

      # The platforms to build for and their combined settings
      # (nix/buck2/platforms.nix's conditions), and the tool that writes each
      # distribution's rules.star, each dependency's marker evaluated per
      # platform (nix/packages/pydeps-cell.nix)
      conditions,
      pydepsCell,
      # The Python toolchain's version the markers are evaluated for
      pythonVersion ? pkgs.python3.version,

      # Optional
      # The locked dependencies' fixups: [ { key; name; version; } ] ->
      # { fixups = { <key> = { commands; }; }; } (nix/lib/fixups's resolve)
      resolveFixups ? (_: { fixups = { }; }),
      # The user's patches (tk compose patch): <dir>/<cellName>/vendor/<name>/
      userPatchesDir ? null,
    }:
    let
      depsToml = builtins.fromTOML (builtins.readFile depsFile);
      deps = depsToml.deps or { };

      # The locked distributions' fixups
      resolvedFixups = resolveFixups (
        lib.mapAttrsToList (name: depSpec: {
          key = name;
          inherit name;
          inherit (depSpec) version;
        }) deps
      );

      fixups = resolvedFixups.fixups;

      userPatches = userPatchesOf {
        dir = userPatchesDir;
        inherit cellName;
        names = lib.attrNames deps;
      };

      # Build individual dep packages
      depPackages = lib.mapAttrs (
        name: depSpec:
        mkPythonDepPackage {
          inherit name;
          version = depSpec.version;
          sha256 = depSpec.hash;
          url = depSpec.url;
          fixup = fixups.${name}.commands or "";
          slice = sliceOf deps depSpec;
          inherit conditions pydepsCell pythonVersion;
          userPatches = userPatches.${name} or [ ];
        }
      ) deps;

      # Each distribution's package; one version per name (pydeps-gen
      # rejects a forked lock), so no version alias packages
      index = mkCellIndex {
        inherit cellName depsFile;
        packages = lib.mapAttrs' (name: lib.nameValuePair "vendor/${name}") depPackages;
        aliases = { };
      };
    in
    index // { inherit depPackages index; };

}
