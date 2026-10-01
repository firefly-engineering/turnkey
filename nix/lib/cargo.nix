# Cargo workspace utilities for Nix
#
# Builds turnkey's own Rust tools from a workspace projection: the root
# workspace narrowed to one tool's members, computed at evaluation time
# (see "Workspace projection" in CONTEXT.md).
#
# A projection is `{ root, members, extraFiles } -> { src, lock }`:
#   src   the members' directories and the extra files, plus the projected
#         Cargo.toml and Cargo.lock at the root
#   lock  the projected Cargo.lock, as a string, for cargoLock.lockFileContents
#
# Nix names a derivation's output after its inputs, so neither the root
# Cargo.toml nor the root Cargo.lock may be an input to `src`: both are read
# while evaluating, and only what the members reach is written out. A change
# to the workspace that the members don't reach leaves `src` and `lock`, and
# so the tool, unchanged.
#
# Usage:
#   let
#     cargoLib = import ./cargo.nix { inherit pkgs lib; };
#     projection = cargoLib.workspaceProjection {
#       root = ./.;
#       members = [ "src/cmd/my-tool" "src/rust/my-lib" ];
#     };
#   in
#   pkgs.rustPlatform.buildRustPackage {
#     inherit (projection) src;
#     cargoLock.lockFileContents = projection.lock;
#     ...
#   }
#
{ pkgs, lib }:

let
  fs = lib.fileset;

  readTOML = path: builtins.fromTOML (builtins.readFile path);

  # The tables a member manifest declares dependencies in, target-specific
  # ones included
  dependencyTables =
    manifest:
    let
      tablesOf =
        attrs:
        map (kind: attrs.${kind} or { }) [
          "dependencies"
          "dev-dependencies"
          "build-dependencies"
        ];
    in
    tablesOf manifest ++ lib.concatMap tablesOf (lib.attrValues (manifest.target or { }));

  inheritsFromWorkspace = value: builtins.isAttrs value && (value.workspace or false);

  # Cargo.toml narrowed to the members: [workspace] members is exactly them,
  # and [workspace.dependencies] and [workspace.package] keep only what they
  # inherit. Everything else cargo reads to build them (resolver, profiles,
  # patches) is kept as it is.
  projectManifest =
    { manifest, memberManifests }:
    let
      workspace = manifest.workspace or { };
      inherited = attrs: lib.attrNames (lib.filterAttrs (_: inheritsFromWorkspace) attrs);
      dependencies = lib.unique (
        lib.concatMap (m: lib.concatMap inherited (dependencyTables m)) (lib.attrValues memberManifests)
      );
      packageFields = lib.unique (
        lib.concatMap (m: inherited (m.package or { })) (lib.attrValues memberManifests)
      );
      inheritsLints = lib.any (m: m.lints.workspace or false) (lib.attrValues memberManifests);
      missing = lib.filter (name: !(workspace.dependencies or { } ? ${name})) dependencies;
    in
    assert lib.assertMsg (missing == [ ])
      "workspace projection: members inherit ${lib.concatStringsSep ", " missing}, which [workspace.dependencies] doesn't declare";
    manifest
    // {
      workspace =
        removeAttrs workspace [
          "members"
          "default-members"
          "exclude"
          "metadata"
          "dependencies"
          "package"
          "lints"
        ]
        // {
          members = lib.attrNames memberManifests;
        }
        // lib.optionalAttrs (dependencies != [ ]) {
          dependencies = lib.getAttrs dependencies workspace.dependencies;
        }
        // lib.optionalAttrs (packageFields != [ ]) {
          package = lib.getAttrs packageFields workspace.package;
        }
        // lib.optionalAttrs inheritsLints { inherit (workspace) lints; };
    };

  # A package's identity in the lock: name, version and source together
  packageKey = p: "${p.name} ${p.version} ${p.source or ""}";

  # Cargo.lock narrowed to the packages the members reach. Every dependency
  # edge is followed: the lock doesn't depend on features, optional
  # dependencies are always listed, so this is the lock cargo writes for the
  # projected workspace.
  projectLock =
    { lock, memberNames }:
    let
      packages = lock.package or [ ];
      byName = lib.groupBy (p: p.name) packages;

      # A lock dependency is `name`, `name version` or
      # `name version (source)`, the fewest fields that name one package
      resolve =
        ref:
        let
          fields = lib.filter (f: f != "") (lib.splitString " " ref);
          name = lib.elemAt fields 0;
          version = lib.elemAt fields 1;
          source = lib.removeSuffix ")" (lib.removePrefix "(" (lib.elemAt fields 2));
          matches = lib.filter (
            p:
            (lib.length fields < 2 || p.version == version)
            && (lib.length fields < 3 || (p.source or "") == source)
          ) (byName.${name} or [ ]);
        in
        if lib.length fields > 3 || lib.length matches != 1 then
          throw "workspace projection: Cargo.lock dependency \"${ref}\" names ${toString (lib.length matches)} packages, not one"
        else
          lib.head matches;

      memberPackage =
        name:
        let
          matches = lib.filter (p: !(p ? source)) (byName.${name} or [ ]);
        in
        if lib.length matches != 1 then
          throw "workspace projection: Cargo.lock has no workspace package ${name}"
        else
          lib.head matches;

      node = p: {
        key = packageKey p;
        package = p;
      };

      reached = builtins.genericClosure {
        startSet = map (name: node (memberPackage name)) memberNames;
        operator = item: map (ref: node (resolve ref)) (item.package.dependencies or [ ]);
      };
      reachedKeys = lib.genAttrs (map (item: item.key) reached) (_: true);
    in
    lock
    // {
      # In the lock's own order, so the text is what cargo writes
      package = lib.filter (p: reachedKeys ? ${packageKey p}) packages;
    };

  # Cargo.lock's text. The lock is a `version` and a list of packages whose
  # fields are strings and arrays of strings; anything else fails evaluation
  # rather than being written wrong.
  lockText =
    lock:
    let
      str = builtins.toJSON;
      field =
        p: key:
        if !(p ? ${key}) then
          [ ]
        else if key == "dependencies" then
          [ "dependencies = [" ] ++ map (d: " ${str d},") p.dependencies ++ [ "]" ]
        else
          [ "${key} = ${str p.${key}}" ];
      fields = [
        "name"
        "version"
        "source"
        "checksum"
        "dependencies"
      ];
      package =
        p:
        let
          unknown = lib.subtractLists fields (lib.attrNames p);
        in
        assert lib.assertMsg (unknown == [ ])
          "workspace projection: Cargo.lock package ${p.name} has fields this projection doesn't write: ${lib.concatStringsSep ", " unknown}";
        lib.concatStringsSep "\n" ([ "[[package]]" ] ++ lib.concatMap (field p) fields) + "\n";
      unknownTop = lib.subtractLists [
        "version"
        "package"
      ] (lib.attrNames lock);
    in
    assert lib.assertMsg (unknownTop == [ ])
      "workspace projection: Cargo.lock has sections this projection doesn't write: ${lib.concatStringsSep ", " unknownTop}";
    lib.concatStringsSep "\n" ([ "version = ${toString lock.version}\n" ] ++ map package lock.package);

  # Arguments:
  #   root:     Path to the workspace root
  #   members:  Workspace members the tool is built from (e.g. ["src/cmd/foo",
  #             "src/rust/bar"]), path dependencies among them included
  #   extraFiles: Other files the members read, at their path from root (e.g.
  #             test cases a member's testdata/ links to)
  #   manifest: The parsed root Cargo.toml (defaults to root's)
  #   lock:     The parsed root Cargo.lock (defaults to root's)
  #
  # Returns: { src, lock } (see the top of this file)
  workspaceProjection =
    {
      root,
      members,
      extraFiles ? [ ],
      manifest ? readTOML (root + "/Cargo.toml"),
      lock ? readTOML (root + "/Cargo.lock"),
    }:
    let
      memberManifests = lib.genAttrs members (m: readTOML (root + "/${m}/Cargo.toml"));
      projectedLock = lockText (projectLock {
        inherit lock;
        memberNames = map (m: m.package.name) (lib.attrValues memberManifests);
      });
      projectedManifest = (pkgs.formats.toml { }).generate "Cargo.toml" (projectManifest {
        inherit manifest memberManifests;
      });
      memberSources = fs.toSource {
        inherit root;
        fileset = fs.unions (map (path: root + "/${path}") (members ++ extraFiles));
      };
    in
    {
      lock = projectedLock;
      src = pkgs.runCommand "cargo-workspace-projection" { } ''
        cp -r ${memberSources} $out
        chmod -R u+w $out
        cp ${projectedManifest} $out/Cargo.toml
        cp ${builtins.toFile "Cargo.lock" projectedLock} $out/Cargo.lock
      '';
    };

in
{
  inherit workspaceProjection;
}
