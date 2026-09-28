# The platforms turnkey builds for (the buck2.platforms option), as Buck2
# sees them.
#
# Pure. A platform is a Nix system (e.g. "aarch64-darwin"); Buck2 names it by
# the values of its os and cpu constraints (config//os:macos,
# config//cpu:arm64). Rules sync (through .turnkey/sync.toml) and the cell
# generators get the platforms from here, and the toolchains cell defines
# one combined config_setting per platform (settingsBuckFile), which a
# select() uses when deps differ by CPU within one OS.
{ lib }:

let
  # Nix's names for the parts of a system, in Buck2's
  cpus = {
    x86_64 = "x86_64";
    aarch64 = "arm64";
  };
  oses = {
    linux = "linux";
    darwin = "macos";
  };

  # "aarch64-darwin" -> { system = "aarch64-darwin"; os = "macos"; cpu = "arm64"; }
  fromSystem =
    system:
    let
      parts = lib.splitString "-" system;
      cpu = builtins.head parts;
      os = lib.last parts;
    in
    if builtins.length parts != 2 || !(cpus ? ${cpu}) || !(oses ? ${os}) then
      throw "turnkey: buck2.platforms: ${system} is not a system turnkey knows how to name in Buck2 (${lib.concatStringsSep ", " (lib.attrNames cpus)} on ${lib.concatStringsSep ", " (lib.attrNames oses)})"
    else
      {
        inherit system;
        os = oses.${os};
        cpu = cpus.${cpu};
      };

  # The Buck2 package of the combined config_settings, in the toolchains
  # cell, each named "<os>-<cpu>"
  settingsPackage = "toolchains//conditions";
in
{
  inherit fromSystem settingsPackage;

  # Every OS and CPU turnkey names, in Buck2's names
  names = {
    os = lib.attrValues oses;
    cpu = lib.attrValues cpus;
  };

  # What the cell generators get: the platforms, in Buck2's names, and the
  # package of the combined config_settings. As JSON, it is what
  # turnkey.cfg.Platforms.from_json reads.
  conditions = systems: {
    settings = settingsPackage;
    platforms = map (system: builtins.removeAttrs (fromSystem system) [ "system" ]) systems;
  };

  # Split each platform's list of values (valuesOf platform) into the values
  # every platform has and select() branches, with the conditions core's
  # key rules (src/go/pkg/conditions): the smallest exact key, the OS's
  # (config//os:<os>), the CPU's (config//cpu:<cpu>), or the combined
  # <settings>:<os>-<cpu>, and a branch for every platform, with no
  # DEFAULT. conditions is `conditions`'s result. Returns { common;
  # branches = [ { key; values; } ] sorted by key, empty when every
  # platform has the same values }. The split-vectors flake check runs the
  # conditions core's test cases (testdata/split-vectors.json) against it.
  split =
    conditions: valuesOf:
    let
      inherit (conditions) platforms;
      per = map (p: {
        platform = p;
        values = lib.unique (valuesOf p);
      }) platforms;
      common = builtins.filter (v: lib.all (x: builtins.elem v x.values) per) (builtins.head per).values;
      extra = map (x: x // { values = lib.subtractLists common x.values; }) per;
      keyOf =
        dims: p:
        if dims == [ "os" ] then
          "config//os:${p.os}"
        else if dims == [ "cpu" ] then
          "config//cpu:${p.cpu}"
        else
          "${conditions.settings}:${p.os}-${p.cpu}";
      sameSet = a: b: lib.sort (x: y: x < y) a == lib.sort (x: y: x < y) b;
      # The branches keyed on dims, or null if dims don't tell the
      # platforms' values apart
      keyed =
        dims:
        let
          groups = lib.groupBy (x: keyOf dims x.platform) extra;
          consistent = lib.all (g: lib.all (x: sameSet x.values (builtins.head g).values) g) (
            builtins.attrValues groups
          );
        in
        if consistent then
          lib.mapAttrsToList (key: g: {
            inherit key;
            inherit (builtins.head g) values;
          }) groups
        else
          null;
      firstKeyed = lib.findFirst (b: b != null) null (
        map keyed [
          [ "os" ]
          [ "cpu" ]
          [
            "os"
            "cpu"
          ]
        ]
      );
    in
    {
      inherit common;
      branches =
        if lib.all (x: x.values == [ ]) extra then [ ] else lib.sort (a: b: a.key < b.key) firstKeyed;
    };

  # The combined config_settings' BUCK file, for platforms (fromSystem's)
  # and the allowed Go build tags: one per value combination of every set
  # of two or more dimensions (os, cpu, then each tag, set or unset), named
  # by the values' tokens joined with "-" (a tag's token is the tag when
  # set, no_<tag> when unset), as src/go/pkg/conditions names them.
  settingsBuckFile =
    platforms: tags:
    let
      sortedTags = lib.sort (a: b: a < b) tags;
      dimensions = [
        {
          name = "os";
          constraint = v: "prelude//os/constraints:os[${v}]";
          token = v: v;
        }
        {
          name = "cpu";
          constraint = v: "prelude//cpu/constraints:cpu[${v}]";
          token = v: v;
        }
      ]
      ++ map (tag: {
        name = "tag:${tag}";
        constraint = v: "prelude//go/tags/constraints:${tag}[${v}]";
        token = v: if v == "set" then tag else "no_${tag}";
      }) sortedTags;

      # Every platform with every combination of tag values
      configurations = lib.foldl' (
        configs: tag:
        lib.concatMap (config: [
          (config // { "tag:${tag}" = "set"; })
          (config // { "tag:${tag}" = "unset"; })
        ]) configs
      ) (map (p: { inherit (p) os cpu; }) platforms) sortedTags;

      # The sets of two or more dimensions, each in dimension order
      subsets =
        dims:
        if dims == [ ] then
          [ [ ] ]
        else
          let
            rest = subsets (builtins.tail dims);
          in
          map (s: [ (builtins.head dims) ] ++ s) rest ++ rest;
      sets = builtins.filter (set: builtins.length set >= 2) (subsets dimensions);

      settings = lib.unique (
        lib.concatMap (
          set:
          map (config: {
            name = lib.concatMapStringsSep "-" (d: d.token config.${d.name}) set;
            constraints = map (d: d.constraint config.${d.name}) set;
          }) configurations
        ) sets
      );
    in
    ''
      # Generated by turnkey - do not edit manually
      #
      # The config_settings combining the platform turnkey builds for
      # (buck2.platforms) and the allowed Go build tags
      # (buck2.go.allowedBuildTags): a select() uses one when deps differ by
      # more than one of them; deps that differ by one use its own
      # constraint (config//os:<os>, config//cpu:<cpu>, a tag's).
    ''
    + lib.concatMapStrings (setting: ''

      config_setting(
          name = "${setting.name}",
          constraint_values = [
      ${lib.concatMapStrings (c: "        \"${c}\",\n") setting.constraints}    ],
          visibility = ["PUBLIC"],
      )
    '') settings;
}
