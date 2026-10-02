# JavaScript/TypeScript Language Adapter for Dependency Cells
#
# Provides:
#   - mkJsDepPackage: Build a single npm package's contents: its tarball with
#     its fixup and user patches applied, and its own rules.star
#   - mkJsDepsCell: The JavaScript dependency cell's cell index (ADR 0004,
#     ADR 0012)
#
# The cell separates what a package is from where it sits in the graph
# (docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md):
#
#   - each locked `name@version` ([[package]]) is its own derivation, at
#     vendor/<name>@<version>, exposing its files as `files`;
#   - the cell root's rules.star, which the index carries, declares one
#     npm_instance per pnpm snapshot ([[instance]]), one npm_component per
#     dependency cycle with a forwarding npm_member per instance in it, and
#     one alias per direct dependency ([direct]), named by its npm name.
#
# The rules come from turnkey's prelude (typescript/npm.bzl): they lay each
# instance out as node_modules/<name>/, with its dependencies linked beside
# it.

{
  pkgs,
  lib,
  genericBuilder,
}:

let
  fetchers = import ../fetchers.nix { inherit pkgs lib; };
  platformsLib = import ../../../buck2/platforms.nix { inherit lib; };
  inherit (genericBuilder) mkCellIndex;

  # npm's names for Buck2's OS and CPU constraint values
  npmOS = {
    linux = "linux";
    macos = "darwin";
  };
  npmCPU = {
    x86_64 = "x64";
    arm64 = "arm64";
  };

  # Whether a package.json-style field (os, cpu, libc: names, or "!name" to
  # exclude one) allows value: when it lists none, any
  allows =
    field: value:
    let
      excluded = map (lib.removePrefix "!") (builtins.filter (lib.hasPrefix "!") field);
      included = builtins.filter (v: !(lib.hasPrefix "!" v)) field;
    in
    !(builtins.elem value excluded) && (included == [ ] || builtins.elem value included);

  # Whether npm installs pkg (a js-deps.toml [[package]]) on a platform
  # (Buck2's names): its os and cpu allow the platform's, and on Linux, its
  # libc allows glibc, the C library turnkey's Linux platforms use
  installsOn =
    pkg: platform:
    allows (pkg.os or [ ]) npmOS.${platform.os}
    && allows (pkg.cpu or [ ]) npmCPU.${platform.cpu}
    && (platform.os != "linux" || allows (pkg.libc or [ ]) "glibc");

  # A package's derivation name: "@types/node" is "types-node"
  sanitizeName = name: lib.replaceStrings [ "@" "/" ] [ "" "-" ] (lib.removePrefix "@" name);

  # The longest instance target name. buck2 puts a target's outputs in a
  # directory named after it, padded, and a path component holds at most
  # 255 bytes.
  maxTargetName = 240;

  # An instance's target name: pnpm's directory name for its key in
  # node_modules/.pnpm (@pnpm/dependency-path's depPathToFilename). "/" and
  # the characters a file name can't hold become "+", and a peer or patch
  # group "(x)" becomes "_x": "react-dom@18.2.0(react@18.2.0)" is
  # "react-dom@18.2.0_react@18.2.0". A name longer than maxTargetName is cut
  # and ends with a hash of the whole.
  instanceName =
    key:
    let
      grouped = lib.hasInfix "(" key;
      escaped =
        lib.replaceStrings
          [
            "/"
            "\\"
            ":"
            "*"
            "?"
            "\""
            "<"
            ">"
            "|"
          ]
          [
            "+"
            "+"
            "+"
            "+"
            "+"
            "+"
            "+"
            "+"
            "+"
          ]
          key;
      named =
        if grouped then
          lib.replaceStrings [ ")(" "(" ")" ] [ "_" "_" "_" ] (lib.removeSuffix ")" escaped)
        else
          escaped;
      hash = builtins.substring 0 32 (builtins.hashString "sha256" named);
    in
    if builtins.stringLength named > maxTargetName then
      "${builtins.substring 0 (maxTargetName - 33) named}_${hash}"
    else
      named;

  # The strongly connected components of a graph (Tarjan's algorithm):
  # nodes, and edges node -> the nodes it depends on. Each component is a
  # list of nodes, sorted.
  stronglyConnected =
    nodes: edges:
    let
      visit =
        v: state:
        let
          entered = state // {
            index = state.index + 1;
            indices = state.indices // {
              ${v} = state.index;
            };
            low = state.low // {
              ${v} = state.index;
            };
            stack = [ v ] ++ state.stack;
            onStack = state.onStack // {
              ${v} = true;
            };
          };
          lower =
            s: n:
            s
            // {
              low = s.low // {
                ${v} = lib.min s.low.${v} n;
              };
            };
          walked = lib.foldl' (
            s: w:
            if !(s.indices ? ${w}) then
              let
                after = visit w s;
              in
              lower after after.low.${w}
            else if s.onStack ? ${w} then
              lower s s.indices.${w}
            else
              s
          ) entered (edges v);
          # v is its component's root: the component is the stack down to v
          depth = lib.lists.findFirstIndex (n: n == v) null walked.stack;
          component = lib.take (depth + 1) walked.stack;
        in
        if walked.low.${v} != walked.indices.${v} then
          walked
        else
          walked
          // {
            stack = lib.drop (depth + 1) walked.stack;
            onStack = removeAttrs walked.onStack component;
            components = walked.components ++ [ (lib.sort (a: b: a < b) component) ];
          };
      final = lib.foldl' (s: v: if s.indices ? ${v} then s else visit v s) {
        index = 0;
        indices = { };
        low = { };
        stack = [ ];
        onStack = { };
        components = [ ];
      } nodes;
    in
    final.components;

  # A Starlark string
  str = s: builtins.toJSON s;

  # A Starlark dict of strings, one entry per line, at indent
  strDict =
    indent: attrs:
    if attrs == { } then
      "{}"
    else
      "{\n"
      + lib.concatMapStrings (name: "${indent}    ${str name}: ${attrs.${name}},\n") (lib.attrNames attrs)
      + "${indent}}";
in
rec {
  # Build inputs for per-dependency builds
  buildInputs = [ ];

  inherit instanceName stronglyConnected;

  # ==========================================================================
  # Public API
  # ==========================================================================

  # Build a single npm package's contents: the files npm would unpack, with
  # a rules.star exposing them as `files`, the content-keyed input of every
  # instance of the package (ADR 0012)
  mkJsDepPackage =
    {
      name, # Package name (e.g., "lodash" or "@types/node")
      version, # Version string (e.g., "4.17.21")
      integrity, # SRI hash (sha512-...)
      url, # URL to fetch from (npm tarball)

      # Optional
      # The tarball as a file in the project, read instead of fetching url
      # (turnkey.buck2.javascript.tarballs); it must match integrity
      tarball ? null,
      fixup ? null, # Custom fixup commands

      # The user's patches of this package (tk compose patch), in order. They
      # name files as a/vendor/<name>@<version>/... (or a/vendor/<name>/...),
      # and apply after the fixup; one that doesn't apply fails the package.
      userPatches ? [ ],
    }:
    let
      fetchSpec = {
        type = "url";
        inherit url;
        hash = integrity;
      };

      # The project's own tarball, checked against the lock's integrity as
      # a fetch would be
      local =
        let
          algo = lib.head (lib.splitString "-" integrity);
          want = builtins.convertHash {
            hash = integrity;
            toHashFormat = "base16";
          };
        in
        if builtins.hashFile algo tarball == want then
          tarball
        else
          throw "turnkey: ${toString tarball} doesn't match the integrity js-deps.toml records for ${name}@${version}";

      # a/vendor/<name>@<version>/...: a/, vendor/ and the name's segments
      # (a scoped package's name has two)
      strip = 2 + lib.length (lib.splitString "/" name);

      buckContent = ''
        # Auto-generated by turnkey deps-cell
        # Package: ${name}@${version}

        filegroup(
            name = "files",
            srcs = glob(["**"], exclude = ["rules.star"]),
            visibility = ["PUBLIC"],
        )
      '';
    in
    pkgs.runCommand "dep-js-${sanitizeName name}-${version}"
      {
        nativeBuildInputs = buildInputs ++ lib.optional (userPatches != [ ]) pkgs.patch;
        src = if tarball != null then local else fetchers.fetch fetchSpec;
        # The `targets` output lists the rules.star's target names, one per
        # line, for the cell index
        outputs = [
          "out"
          "targets"
        ];
        passthru = { inherit name version; };
      }
      (
        ''
          mkdir -p $out

          # npm tarballs have a 'package/' prefix, extract contents directly
          if [[ -d $src ]]; then
            cp -r $src/* $out/
          else
            tar -xzf $src --strip-components=1 -C $out
          fi
          chmod -R u+w $out

          # Apply fixup if provided
          cd $out
          ${if fixup != null then fixup else ""}
        ''
        + lib.concatMapStrings (patchFile: ''

          # The user's patch ${baseNameOf patchFile}
          patch -d $out -p${toString strip} --forward --fuzz=0 < ${patchFile} || {
            echo "error: user patch ${baseNameOf patchFile} does not apply to ${name}@${version}"
            echo "  (regenerate it with 'tk compose patch', or remove it)"
            exit 1
          }
        '') userPatches
        + ''

          # Generate rules.star file
          cat > $out/rules.star << 'RULES'
          ${buckContent}
          RULES
          echo files > $targets
        ''
      );

  # The user's patches of each locked package, keyed "name@version", from
  # <dir>/<cellName>/vendor/<package>/*.patch as tk compose patch writes them
  # (in name order). <package> is a locked "name@version", or the bare name
  # of a direct dependency, for the version it resolves to (direct: bare
  # name -> "name@version"). A scoped package's directory is under
  # vendor/@<scope>/.
  userPatchesOf =
    {
      dir,
      cellName,
      keys,
      direct ? { },
    }:
    let
      cellDir = dir + "/${cellName}";
      entries = if dir != null && builtins.pathExists cellDir then builtins.readDir cellDir else { };
      flat = lib.filter (name: entries.${name} != "directory") (lib.attrNames entries);
      # Each directory under vendor/ holding patches, by its path there
      patchDirs =
        rel: path:
        let
          dirEntries = builtins.readDir path;
          patches = lib.sort (a: b: a < b) (
            lib.attrNames (
              lib.filterAttrs (name: type: type == "regular" && lib.hasSuffix ".patch" name) dirEntries
            )
          );
          subdirs = lib.attrNames (lib.filterAttrs (_: type: type == "directory") dirEntries);
        in
        lib.optionalAttrs (patches != [ ]) { ${rel} = map (name: path + "/${name}") patches; }
        // lib.foldl' (
          acc: sub: acc // patchDirs (if rel == "" then sub else "${rel}/${sub}") (path + "/${sub}")
        ) { } subdirs;
      found = if (entries.vendor or null) == "directory" then patchDirs "" (cellDir + "/vendor") else { };
      keyOf =
        package:
        if builtins.elem package keys then
          package
        else
          direct.${package}
            or (throw "turnkey: user patches under ${cellName}/vendor/${package}/: ${cellName} locks no such package (patches go in vendor/<name>@<version>/, or vendor/<name>/ for a direct dependency)");
    in
    if flat != [ ] then
      throw "turnkey: user patches for ${cellName} go under ${cellName}/vendor/<name>@<version>/, one directory per package (tk compose patch writes them there); move or regenerate: ${lib.concatStringsSep ", " flat}"
    else
      lib.foldl' (
        acc: package:
        let
          key = keyOf package;
        in
        acc // { ${key} = (acc.${key} or [ ]) ++ found.${package}; }
      ) { } (lib.attrNames found);

  # The cell root's rules.star (ADR 0012), from js-deps.toml's [[package]],
  # [[instance]] and [direct] tables, for the platforms in conditions
  # (nix/buck2/platforms.nix's conditions)
  rootRules =
    {
      packages,
      instances,
      direct,
      conditions,
    }:
    let
      byKey = builtins.listToAttrs (map (i: lib.nameValuePair i.key i) instances);
      packagesByKey = builtins.listToAttrs (
        map (pkg: lib.nameValuePair "${pkg.name}@${pkg.version}" pkg) packages
      );
      packageOf =
        instance:
        let
          key = "${instance.name}@${instance.version}";
        in
        packagesByKey.${key}
          or (throw "turnkey: js-deps.toml's instance ${instance.key} installs ${key}, which it has no [[package]] of; regenerate it with tk sync");
      filesLabel = instance: str "//vendor/${instance.name}@${instance.version}:files";
      label = key: str ":${instanceName key}";

      # The instance keys each instance depends on, optional ones included
      edges =
        key: lib.attrValues (byKey.${key}.dependencies or { } // byKey.${key}.optional_dependencies or { });
      components = stronglyConnected (lib.attrNames byKey) edges;
      # A component is a cycle when it has several instances, or one that
      # depends on itself
      cyclic = c: lib.length c > 1 || builtins.elem (lib.head c) (edges (lib.head c));
      cycles = builtins.filter cyclic components;
      componentOf = builtins.listToAttrs (lib.concatMap (c: map (key: lib.nameValuePair key c) c) cycles);

      # An instance's optional dependencies, split by the platforms npm
      # installs each on: { common = { <import name> = <key>; }; branches =
      # [ { key; values = { ... }; } ]; } (platforms.nix's split, on the
      # import names)
      optionalSplit =
        instance:
        let
          optional = instance.optional_dependencies or { };
          split = platformsLib.split conditions (
            platform:
            builtins.filter (imp: installsOn (packageOf byKey.${optional.${imp}}) platform) (
              lib.attrNames optional
            )
          );
          pick = names: lib.genAttrs names (imp: optional.${imp});
        in
        {
          common = pick split.common;
          branches = map (b: {
            inherit (b) key;
            values = pick b.values;
          }) split.branches;
        };

      # A target's call: rule(name = ..., <attrs>), attrs a list of
      # "<name> = <value>" strings
      call = rule: attrs: "${rule}(\n" + lib.concatMapStrings (attr: "    ${attr},\n") attrs + ")\n";

      # The `optional_deps` attribute of a target, given each branch's
      # value as text: none without platform-only optional deps
      optionalDepsAttr =
        branches: render:
        lib.optional (branches != [ ]) (
          "optional_deps = select({\n"
          + lib.concatMapStrings (b: "        ${str b.key}: ${render b},\n") branches
          + "    })"
        );

      # A plain instance: deps by import name, platform-only optional deps
      # as a select()
      instanceTarget =
        key:
        let
          instance = byKey.${key};
          split = optionalSplit instance;
          deps = lib.mapAttrs (_: label) ((instance.dependencies or { }) // split.common);
        in
        call "npm_instance" (
          [
            "name = ${str (instanceName key)}"
            "package = ${str instance.name}"
            "files = ${filesLabel instance}"
            "deps = ${strDict "    " deps}"
          ]
          ++ optionalDepsAttr split.branches (b: strDict "        " (lib.mapAttrs (_: label) b.values))
        );

      # A cycle: one npm_component holding every member, each member's
      # dependencies inside the cycle by member name and those outside by
      # label, then a forwarding npm_member per member
      componentTarget =
        members:
        let
          name = "_component_${instanceName (lib.head members)}";
          inside = dep: builtins.elem dep members;
          splits = lib.genAttrs members (key: optionalSplit byKey.${key});
          depsOf = key: (byKey.${key}.dependencies or { }) // splits.${key}.common;
          outside = deps: lib.mapAttrs (_: label) (lib.filterAttrs (_: dep: !inside dep) deps);
          perMember =
            render:
            strDict "    " (
              lib.listToAttrs (map (key: lib.nameValuePair (instanceName key) (render key)) members)
            );
          internal =
            key:
            strDict "        " (
              lib.mapAttrs (_: dep: str (instanceName dep)) (lib.filterAttrs (_: inside) (depsOf key))
            );
          # Every member's split has the same branch keys: the conditions'
          branchKeys = lib.unique (lib.concatMap (key: map (b: b.key) splits.${key}.branches) members);
          branches = map (bkey: {
            key = bkey;
            values = lib.listToAttrs (
              map (
                key:
                lib.nameValuePair (instanceName key)
                  (lib.findFirst (b: b.key == bkey) { values = { }; } splits.${key}.branches).values
              ) members
            );
          }) branchKeys;
        in
        call "npm_component" (
          [
            "name = ${str name}"
            "members = ${perMember (key: str byKey.${key}.name)}"
            "files = ${perMember (key: filesLabel byKey.${key})}"
            "internal = ${perMember internal}"
            "deps = ${perMember (key: strDict "        " (outside (depsOf key)))}"
          ]
          ++ optionalDepsAttr branches (
            b: strDict "        " (lib.mapAttrs (_: values: strDict "            " (outside values)) b.values)
          )
        )
        + lib.concatMapStrings (
          key:
          "\n"
          + call "npm_member" [
            "name = ${str (instanceName key)}"
            "component = ${str ":${name}"}"
            "member = ${str (instanceName key)}"
          ]
        ) members;

      plain = builtins.filter (key: !(componentOf ? ${key})) (lib.attrNames byKey);
    in
    ''
      # Auto-generated by turnkey deps-cell
      # JavaScript dependencies cell: one npm_instance per package instance
      # (docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md)

      load("@prelude//typescript:npm.bzl", "npm_component", "npm_instance", "npm_member")

    ''
    + lib.concatMapStringsSep "\n" instanceTarget plain
    + lib.concatMapStrings (c: "\n" + componentTarget c) cycles
    + lib.concatMapStrings (
      name:
      "\n"
      + call "alias" [
        "name = ${str name}"
        "actual = ${label direct.${name}}"
        "visibility = [\"PUBLIC\"]"
      ]
    ) (lib.attrNames direct);

  # Build the JavaScript dependency cell (ADR 0004, ADR 0012): each locked
  # package's contents (mkJsDepPackage), and the cell index that tk
  # materialize keeps .turnkey/<cellName> in line with. The result is the
  # index, with depPackages (each package's derivation, keyed
  # "name@version") and index (itself) alongside.
  mkJsDepsCell =
    {
      cellName, # The cell's name (nix/buck2/languages.nix)
      depsFile, # Path to js-deps.toml

      # The platforms to build for and their combined settings
      # (nix/buck2/platforms.nix's conditions): an instance's optional
      # dependencies restricted to some platforms (os/cpu/libc) are a select()
      conditions,

      # Optional
      # The locked dependencies' fixups: [ { key; name; version; } ] ->
      # { fixups = { <key> = { commands; }; }; } (nix/lib/fixups's resolve)
      resolveFixups ? (_: { fixups = { }; }),
      # The user's patches (tk compose patch):
      # <dir>/<cellName>/vendor/<name>@<version>/, or vendor/<name>/ for a
      # direct dependency
      userPatchesDir ? null,
      # npm tarballs kept in the project, by the URL js-deps.toml records
      # for them (turnkey.buck2.javascript.tarballs)
      tarballs ? { },
    }:
    let
      depsToml = builtins.fromTOML (builtins.readFile depsFile);
      packages = depsToml.package or [ ];
      instances = depsToml.instance or [ ];
      direct = depsToml.direct or { };

      keyOf = pkg: "${pkg.name}@${pkg.version}";
      instanceVersions = builtins.listToAttrs (
        map (i: lib.nameValuePair i.key "${i.name}@${i.version}") instances
      );

      # The locked packages' fixups
      resolvedFixups = resolveFixups (
        map (pkg: {
          key = keyOf pkg;
          inherit (pkg) name version;
        }) packages
      );

      fixups = resolvedFixups.fixups;

      userPatches = userPatchesOf {
        dir = userPatchesDir;
        inherit cellName;
        keys = map keyOf packages;
        direct = lib.mapAttrs (_: key: instanceVersions.${key}) direct;
      };

      # Build individual dep packages
      depPackages = builtins.listToAttrs (
        map (pkg: {
          name = keyOf pkg;
          value = mkJsDepPackage {
            inherit (pkg)
              name
              version
              url
              integrity
              ;
            tarball = tarballs.${pkg.url} or null;
            fixup = fixups.${keyOf pkg}.commands or "";
            userPatches = userPatches.${keyOf pkg} or [ ];
          };
        }) packages
      );

      index = mkCellIndex {
        inherit cellName depsFile;
        packages = lib.mapAttrs' (key: lib.nameValuePair "vendor/${key}") depPackages;
        aliases = { };
        root = rootRules {
          inherit
            packages
            instances
            direct
            conditions
            ;
        };
      };
    in
    assert lib.assertMsg (instances != [ ] || packages == [ ])
      "turnkey: ${toString depsFile} has no [[instance]]: regenerate it with tk sync (jsdeps-gen needs a pnpm v9 lockfile)";
    index // { inherit depPackages index; };
}
