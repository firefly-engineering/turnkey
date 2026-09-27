# The Rust fixup registries of before fixup sets, as one turnkeyFixups
# module: buck2.rust.buildScriptFixups and buck2.rust.rustcFlagsRegistry,
# merged over nix/lib/deps-cell/fixups/rust as they always were. Kept only
# until turnkey's fixups are ported to a published set and the old options
# are retired.
#
# Old keys are a crate's name or "name@version"; an exact version becomes a
# versions entry whose bounds hold for that version only.
{ lib }:

{
  buildScriptFixups,
  rustcFlags,
  nativeLibraries,
}:

let
  parseKey =
    key:
    let
      parts = lib.splitString "@" key;
    in
    {
      name = builtins.head parts;
      version = if builtins.length parts > 1 then builtins.elemAt parts 1 else null;
    };

  # What the old registries' functions received
  oldContext = ctx: {
    crateName = ctx.name;
    inherit (ctx) version;
    patchVersion = lib.last (lib.splitString "." ctx.version);
    key = "${ctx.name}@${ctx.version}";
    vendorPath = ".";
  };

  buildScriptOf = fixup: {
    buildScript.generate = if builtins.isFunction fixup then ctx: fixup (oldContext ctx) else fixup;
  };

  # A list of flags applies everywhere; a set keys them by OS, its other
  # keys' flags applying everywhere
  flagsOf =
    flags:
    if builtins.isList flags then
      { rustcFlags = flags; }
    else
      {
        rustcFlags = lib.concatLists (
          lib.attrValues (
            lib.filterAttrs (
              os: _:
              !(builtins.elem os [
                "linux"
                "macos"
              ])
            ) flags
          )
        );
        os = lib.mapAttrs (_: f: { rustcFlags = f; }) (
          lib.filterAttrs (
            os: _:
            builtins.elem os [
              "linux"
              "macos"
            ]
          ) flags
        );
      };

  nativeOf = info: {
    nativeLibraries = [
      {
        name = ctx: (lib.toFunction info (oldContext ctx)).lib_name;
        staticLib = ctx: (lib.toFunction info (oldContext ctx)).static_lib_path;
        linkSearchPath =
          (lib.toFunction info (oldContext {
            name = "_";
            version = "0.0.0";
          })).link_search_path or "out_dir";
      }
    ];
  };

  entries =
    lib.mapAttrsToList (key: f: {
      inherit key;
      fields = buildScriptOf f;
    }) buildScriptFixups
    ++ lib.mapAttrsToList (key: f: {
      inherit key;
      fields = flagsOf f;
    }) rustcFlags
    ++ lib.mapAttrsToList (key: f: {
      inherit key;
      fields = nativeOf f;
    }) nativeLibraries;

  definitionOf =
    entry:
    let
      parsed = parseKey entry.key;
    in
    {
      rust.${parsed.name} =
        if parsed.version == null then
          entry.fields
        else
          {
            versions = [
              (
                entry.fields
                // {
                  # Exactly this version: nothing sorts between it and
                  # itself with a zero component appended
                  when = {
                    atLeast = parsed.version;
                    below = "${parsed.version}.0.1";
                  };
                }
              )
            ];
          };
    };
in
{
  _file = "turnkey's legacy fixup registries (buck2.rust.buildScriptFixups, buck2.rust.rustcFlagsRegistry)";
  imports = map definitionOf entries;
}
