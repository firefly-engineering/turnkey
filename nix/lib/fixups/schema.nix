# The schema of a fixup set: the options a module of class turnkeyFixups
# sets (docs/adr/0003-fixup-sets-are-modules.md).
#
# A fixup is what turnkey supplies for one dependency, named as its own
# ecosystem names it, in place of its build step or to correct its source.
# Every language's fixup has the shared base (enable, patches, env,
# versions); Rust adds its build script, rustc flags, native libraries and
# per-OS/per-CPU overlays. Merging is the module system's: lists
# concatenate, and two sets that disagree on a value fail evaluation,
# naming both files.
{ lib }:

let
  inherit (lib) mkOption types;

  # A string, or a function of the fixup's context returning one; a string
  # is a function ignoring the context, so definitions of either kind merge
  # (and conflict) alike
  strOrFn = types.coercedTo types.str (s: _: s) (types.functionTo types.str);

  # The shared base's fields that overlays and version entries repeat
  baseFields = {
    patches = mkOption {
      type = types.listOf types.path;
      default = [ ];
      description = ''
        Patches applied to the dependency's source, in order, inside its own
        derivation and before its build script output is generated. Paths
        are relative to the dependency's root, applied with -p1, so a plain
        `git diff` in an upstream checkout works as is.
      '';
    };
    env = mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = "Environment variables the dependency is compiled with.";
    };
  };

  # A pre-built native library a Rust fixup's build script produced
  nativeLibrary = types.submodule {
    options = {
      name = mkOption {
        type = strOrFn;
        description = "The library's name (e.g. ring_core_0_17_14__), or a function of the fixup's context.";
      };
      staticLib = mkOption {
        type = strOrFn;
        description = "The static library's path, relative to the crate, or a function of the fixup's context.";
      };
      linkSearchPath = mkOption {
        type = types.str;
        default = "out_dir";
        description = "Where rustc searches for the library, relative to the crate.";
      };
    };
  };

  # What Rust overlays may hold: declarative fields only, never a build
  # script (a crate has one build script, branching on ctx.platform)
  rustDeclarative = baseFields // {
    rustcFlags = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Flags passed to rustc, such as the --cfg directives the crate's build script would emit.";
    };
    nativeLibraries = mkOption {
      type = types.listOf nativeLibrary;
      default = [ ];
      description = ''
        Native libraries the build script produced. They exist only for the
        platform the cell is built on, so the crate links them only there.
      '';
    };
  };

  overlays =
    keys: what:
    mkOption {
      type = types.submodule {
        options = lib.genAttrs keys (
          key:
          mkOption {
            type = types.submodule { options = rustDeclarative; };
            default = { };
            description = "Fields added on ${what} ${key}.";
          }
        );
      };
      default = { };
      description = "Declarative fields added per ${what}; they become select()s over the cell's platforms.";
    };

  rustFields = rustDeclarative // {
    buildScript = mkOption {
      type = types.nullOr (
        types.submodule {
          options = {
            generate = mkOption {
              type = types.nullOr strOrFn;
              default = null;
              description = ''
                Shell commands producing what the crate's build.rs would,
                or a function of the fixup's context returning them. They
                run in the crate's source, with $CRATE_SRC its root and
                $OUT_DIR (created) the build script's output directory.
              '';
            };
            skip = mkOption {
              type = types.bool;
              default = false;
              description = "The crate's build.rs produces nothing the build needs.";
            };
          };
        }
      );
      default = null;
      description = "What stands in for the crate's build.rs: exactly one of generate or skip.";
    };
    os = overlays [
      "linux"
      "macos"
    ] "OS";
    cpu = overlays [
      "x86_64"
      "arm64"
    ] "CPU";
  };

  # A language's fixups, keyed by dependency name, each with the fields,
  # enable, and version-conditioned entries holding the same fields
  fixupsOf =
    language: fields:
    mkOption {
      type = types.attrsOf (
        types.submodule {
          options = fields // {
            enable = mkOption {
              type = types.bool;
              default = true;
              description = "Whether this fixup applies. Disable one an imported set brings.";
            };
            versions = mkOption {
              type = types.listOf (
                types.submodule {
                  options = fields // {
                    when = {
                      atLeast = mkOption {
                        type = types.nullOr types.str;
                        default = null;
                        description = "The entry applies from this version on.";
                      };
                      below = mkOption {
                        type = types.nullOr types.str;
                        default = null;
                        description = "The entry applies below this version.";
                      };
                    };
                  };
                }
              );
              default = [ ];
              description = ''
                Fields added for the locked versions each entry's `when`
                bounds match; every matching entry applies.
              '';
            };
          };
        }
      );
      default = { };
      description = "${language} fixups, keyed by the dependency's name.";
    };
in
{
  options = {
    rust = fixupsOf "Rust" rustFields;
    go = fixupsOf "Go" baseFields;
    python = fixupsOf "Python" baseFields;
    javascript = fixupsOf "JavaScript" baseFields;
    solidity = fixupsOf "Solidity" baseFields;
  };
}
