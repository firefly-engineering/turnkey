# Fixup sets: evaluating them, and resolving a dependency's fixup
# (docs/adr/0003-fixup-sets-are-modules.md).
#
# A repository brings fixup sets as modules of class turnkeyFixups, through
# `imports` in turnkey.toolchains.buck2.fixups, next to its own inline
# fixups. evalFixups evaluates them against ./schema.nix; resolve turns the
# result into what each locked dependency's build needs. Diagnostics are
# data (warnings, errors), so checks can assert on them; `apply` is the thin
# layer that warns and throws.
{ lib }:

let
  schema = import ./schema.nix { inherit lib; };

  languages = [
    "rust"
    "go"
    "python"
    "javascript"
    "solidity"
  ];

  # "1.2.3-rc.1+build" -> { major = "1"; minor = "2"; patch = "3"; pre = "rc.1"; }
  versionParts =
    version:
    let
      noBuild = builtins.head (lib.splitString "+" (lib.removePrefix "v" version));
      dash = lib.splitString "-" noBuild;
      core = lib.splitString "." (builtins.head dash);
      at = i: if builtins.length core > i then builtins.elemAt core i else "0";
    in
    {
      major = at 0;
      minor = at 1;
      patch = at 2;
      pre = if builtins.length dash > 1 then lib.concatStringsSep "-" (builtins.tail dash) else null;
    };

  # Whether a version entry's bounds hold for a locked version
  matches =
    version: when:
    let
      v = lib.removePrefix "v" version;
      cmp = bound: builtins.compareVersions v (lib.removePrefix "v" bound);
    in
    (when.atLeast == null || cmp when.atLeast >= 0) && (when.below == null || cmp when.below < 0);

  # The names a definition of a language's fixups defines, through the
  # module system's mkIf/mkMerge/mkOverride/mkOrder wrappers
  definedNames =
    value:
    if value ? _type then
      if value._type == "merge" then
        lib.concatMap definedNames value.contents
      else if value ? content then
        definedNames value.content
      else
        [ ]
    else
      builtins.attrNames value;

  # Merge a version entry's (or overlay's) fields onto a fixup's. Fields
  # are merged as the module system would: lists concatenate, env entries
  # must agree, and a crate has one build script. Returns
  # { value; errors; }.
  mergeFields =
    what: a: b:
    let
      lists = [
        "patches"
        "rustcFlags"
        "nativeLibraries"
      ];
      listPart = lib.genAttrs (builtins.filter (f: a ? ${f}) lists) (f: a.${f} ++ b.${f});
      envErrors = map (k: "${what}: env.${k} is set to both \"${a.env.${k}}\" and \"${b.env.${k}}\"") (
        envClashes a.env b.env
      );
      bs =
        if !(a ? buildScript) then
          {
            value = { };
            errors = [ ];
          }
        else if a.buildScript == null || b.buildScript == null then
          {
            value.buildScript = if a.buildScript == null then b.buildScript else a.buildScript;
            errors = [ ];
          }
        else
          {
            value.buildScript = {
              generate =
                if a.buildScript.generate == null then b.buildScript.generate else a.buildScript.generate;
              skip = a.buildScript.skip || b.buildScript.skip;
            };
            errors = lib.optional (
              a.buildScript.generate != null && b.buildScript.generate != null
            ) "${what}: two version entries both give buildScript.generate";
          };
      overlayPart =
        { dim, ... }:
        if a ? ${dim} then
          let
            merged = lib.mapAttrs (key: o: mergeFields "${what} (${dim}.${key})" o b.${dim}.${key}) (
              overlaysOf a.${dim}
            );
          in
          {
            value.${dim} = lib.mapAttrs (_: m: m.value) merged;
            errors = lib.concatMap (m: m.errors) (builtins.attrValues merged);
          }
        else
          {
            value = { };
            errors = [ ];
          };
      overlayParts = map overlayPart overlayDims;
    in
    {
      value =
        a
        // listPart
        // {
          env = a.env // b.env;
        }
        // bs.value
        // lib.mergeAttrsList (map (o: o.value) overlayParts);
      errors = envErrors ++ bs.errors ++ lib.concatMap (o: o.errors) overlayParts;
    };

  # The env entries two definitions give different values
  envClashes = a: b: builtins.filter (k: a ? ${k} && a.${k} != b.${k}) (builtins.attrNames b);

  # A Rust fixup's overlays, most general first: the order a target
  # platform ({ os; cpu; }) gets their list fields in. keyOn is the key of
  # the overlay that applies on it.
  overlayDims = [
    {
      dim = "os";
      keyOn = target: target.os;
    }
    {
      dim = "cpu";
      keyOn = target: target.cpu;
    }
    {
      dim = "platform";
      keyOn = target: "${target.os}-${target.cpu}";
    }
  ];

  # The overlays that apply on a target platform, most general first, each
  # { name; fields; } (fields null when a fixup has no such overlay)
  overlaysOn =
    eff: target:
    map (
      { dim, keyOn }:
      {
        name = "${dim}.${keyOn target}";
        fields = eff.${dim}.${keyOn target} or null;
      }
    ) overlayDims;

  # Two overlays that apply on one target platform disagreeing on an env
  # entry, for every OS and CPU pair the overlays have keys for: no
  # overlay wins over another, as no fixup set wins over another
  overlayEnvErrors =
    what: eff:
    let
      targets = lib.cartesianProduct {
        os = builtins.attrNames (overlaysOf eff.os);
        cpu = builtins.attrNames (overlaysOf eff.cpu);
      };
      clashesOn =
        target:
        let
          applying = builtins.filter (o: o.fields != null) (overlaysOn eff target);
        in
        lib.concatLists (
          lib.imap0 (
            i: a:
            lib.concatMap (
              b:
              map (
                k:
                "${what}: env.${k} is set to both \"${a.fields.env.${k}}\" (${a.name}) and \"${b.fields.env.${k}}\" (${b.name})"
              ) (envClashes a.fields.env b.fields.env)
            ) (lib.drop (i + 1) applying)
          ) applying
        );
    in
    lib.unique (lib.concatMap clashesOn targets);

  # The context a fixup's functions receive
  contextOf =
    {
      name,
      version,
      platform,
      pkgs,
    }:
    {
      inherit
        name
        version
        platform
        pkgs
        lib
        ;
      versionParts = versionParts version;
    };

  # A submodule's value without the module system's own _module
  overlaysOf = attrs: lib.filterAttrs (n: _: n != "_module") attrs;

  # functionTo values merge into functors, which builtins.isFunction misses
  callWith = ctx: v: if lib.isFunction v then v ctx else v;

  patchCommands =
    what: patches:
    lib.concatMapStrings (patch: ''
      patch -p1 --forward -d "$out" < ${patch} || {
        echo "error: fixup patch ${baseNameOf (toString patch)} does not apply to ${what}" >&2
        exit 1
      }
    '') patches;
in
rec {
  inherit schema versionParts;

  # Evaluate fixup modules: the sets a repository imports and its inline
  # fixups (its `fixups` definitions), with pkgs for the system being built
  evalFixups =
    {
      modules,
      pkgs ? null,
    }:
    lib.evalModules {
      class = "turnkeyFixups";
      modules = [
        schema
        { _module.args.pkgs = pkgs; }
      ]
      ++ modules;
    };

  # Resolve each locked dependency's fixup.
  #
  #   evaluated    evalFixups's result
  #   language     "rust", "go", ...
  #   deps         [ { key; name; version; } ], the locked dependencies
  #   platform     { system; os; cpu; }: the platform the cell is built on
  #   inlineFiles  the files of the repository's own fixups: a fixup
  #                defined only there warns when it matches nothing locked
  #   catalog      published fixup sets, { <name> = { module; import; }; }
  #                (import: how a repository imports it), which the
  #                message for an unaccounted build script points at when
  #                one of them accounts for it
  #
  # Returns { fixups = { <key> = { commands; gen; accounted; }; };
  # unaccounted = { <key> = message; }; warnings; errors; }. commands run
  # in the dependency's derivation (patches, then the build script); gen is
  # what gen-rust-buck reads. unaccounted holds, for every Rust crate whose
  # build.rs no fixup accounts for, the error the cell fails with if the
  # crate turns out to have one.
  resolve =
    {
      evaluated,
      language,
      deps,
      platform,
      pkgs ? null,
      inlineFiles ? [ ],
      catalog ? { },
    }:
    let
      records = evaluated.config.${language};

      # Provenance: the names defined in the repository's own files, and
      # those some imported set defines
      defs = evaluated.options.${language}.definitionsWithLocations;
      namesIn =
        inline:
        lib.unique (
          lib.concatMap (d: definedNames d.value) (
            builtins.filter (d: builtins.elem d.file inlineFiles == inline) defs
          )
        );
      inlineOnly = lib.subtractLists (namesIn false) (namesIn true);

      lockedNames = lib.unique (map (d: d.name) deps);
      unusedWarnings =
        map (
          n: "turnkey: ${language} fixup ${n} (defined in this repository) matches no locked dependency"
        ) (builtins.filter (n: !(builtins.elem n lockedNames)) inlineOnly)
        ++ lib.concatMap (
          n:
          let
            versions = map (d: d.version) (builtins.filter (d: d.name == n) deps);
          in
          lib.imap1
            (
              i: entry:
              "turnkey: ${language} fixup ${n}'s versions entry ${toString i} (defined in this repository) matches none of its locked versions (${lib.concatStringsSep ", " versions})"
            )
            (builtins.filter (entry: !(builtins.any (v: matches v entry.when) versions)) records.${n}.versions)
        ) (builtins.filter (n: builtins.elem n lockedNames) inlineOnly);

      resolveDep =
        dep:
        let
          record = records.${dep.name};
          what = "${language} fixup ${dep.name} (${dep.version})";
          base = builtins.removeAttrs record [
            "enable"
            "versions"
          ];
          entries = map (e: builtins.removeAttrs e [ "when" ]) (
            builtins.filter (e: matches dep.version e.when) record.versions
          );
          folded =
            lib.foldl'
              (
                acc: entry:
                let
                  m = mergeFields what acc.value entry;
                in
                {
                  inherit (m) value;
                  errors = acc.errors ++ m.errors;
                }
              )
              {
                value = base;
                errors = [ ];
              }
              entries;
          eff = folded.value;
          ctx = contextOf {
            inherit (dep) name version;
            inherit platform pkgs;
          };

          isRust = language == "rust";
          bs = if isRust then eff.buildScript else null;
          script = if bs != null && bs.generate != null then callWith ctx bs.generate else null;
          bsErrors = lib.optional (
            bs != null && bs.generate != null && bs.skip
          ) "${what}: buildScript has both generate and skip; it needs exactly one";

          # What applies on the platform the cell is built on
          hostOverlays = lib.optionals isRust (overlaysOn eff platform);
          # A field as the cell generator reads it: what every platform
          # gets (common), and what each overlay adds, per key
          layered =
            field:
            {
              common = eff.${field};
            }
            // lib.genAttrs (map (d: d.dim) overlayDims) (
              dim: lib.mapAttrs (_: o: o.${field}) (overlaysOf eff.${dim})
            );
          onHost =
            field:
            eff.${field} ++ lib.concatMap (o: if o.fields == null then [ ] else o.fields.${field}) hostOverlays;
          patches = if isRust then onHost "patches" else eff.patches;
          nativeLibraries = map (l: {
            lib_name = callWith ctx l.name;
            static_lib_path = callWith ctx l.staticLib;
            link_search_path = l.linkSearchPath;
          }) (onHost "nativeLibraries");

          envErrors = lib.optional (
            !isRust && eff.env != { }
          ) "${what}: env isn't supported for ${language} fixups yet";

          commands =
            patchCommands "${dep.name} ${dep.version}" patches
            + lib.optionalString (script != null) ''
              export CRATE_SRC="$out" OUT_DIR="$out/out_dir"
              mkdir -p "$OUT_DIR"
              ${script}
            '';
        in
        {
          value = {
            inherit commands;
            accounted = bs != null && (bs.generate != null || bs.skip);
            gen = lib.optionalAttrs isRust {
              outDir = script != null;
              rustcFlags = layered "rustcFlags";
              env = layered "env";
              inherit nativeLibraries;
            };
          };
          errors = folded.errors ++ bsErrors ++ envErrors ++ lib.optionals isRust (overlayEnvErrors what eff);
        };

      applicable = builtins.filter (d: records ? ${d.name} && records.${d.name}.enable) deps;
      resolved = map (d: {
        inherit (d) key;
        r = resolveDep d;
      }) applicable;
      fixups = builtins.listToAttrs (map (x: lib.nameValuePair x.key x.r.value) resolved);

      # The published sets that would account for each crate's build script
      accountedBy = lib.mapAttrs (
        _: set:
        (resolve {
          evaluated = evalFixups {
            inherit pkgs;
            modules = [ set.module ];
          };
          inherit
            language
            deps
            platform
            pkgs
            ;
        }).fixups
      ) catalog;
      unaccountedMessage =
        dep:
        let
          sets = builtins.filter (n: (accountedBy.${n}.${dep.key}.accounted or false)) (
            builtins.attrNames catalog
          );
        in
        "turnkey: ${dep.name} ${dep.version} has a build.rs, and no fixup says what stands in for it; "
        + (
          if sets != [ ] then
            "a published fixup set does: add `${
              catalog.${builtins.head sets}.import
            }` to turnkey.toolchains.buck2.fixups.imports"
          else
            "give it one in turnkey.toolchains.buck2.fixups: `rust.\"${dep.name}\".buildScript.generate` producing what its build.rs would, or `rust.\"${dep.name}\".buildScript.skip = true` if the build needs nothing from it"
        );
    in
    {
      inherit fixups;
      unaccounted = lib.optionalAttrs (language == "rust") (
        builtins.listToAttrs (
          map (dep: lib.nameValuePair dep.key (unaccountedMessage dep)) (
            builtins.filter (dep: !(fixups.${dep.key}.accounted or false)) deps
          )
        )
      );
      warnings = unusedWarnings;
      errors = lib.concatMap (x: x.r.errors) resolved;
    };

  # The thin layer: warn, throw on errors, and return what the cell
  # builders read, { fixups; unaccounted; }
  apply =
    result:
    if result.errors != [ ] then
      throw ("turnkey: fixups:\n" + lib.concatMapStringsSep "\n" (e: "  - ${e}") result.errors)
    else
      lib.foldr lib.warn { inherit (result) fixups unaccounted; } result.warnings;

  # The languages a fixup set can hold fixups for
  inherit languages;
}
