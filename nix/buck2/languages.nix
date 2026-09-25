# The languages turnkey manages dependencies for, one record each.
#
# Everything turnkey knows about a language's dependencies lives in its
# record: the Buck2 cell that holds them, the deps file, the generator that
# writes it and the sync rules that say when to run it. The flake-parts
# module builds the cells from these records, and the devenv module derives
# the cell symlinks, the shell's generators and .turnkey/sync.toml from them,
# so adding a language is a change to this file.
#
# Each record has:
#   name         the option name under turnkey.buck2 (e.g. "go")
#   cellName     the Buck2 cell and the .turnkey/<cellName> symlink
#   description  shown by the shell in verbose mode
#   generator    the package providing the deps file's generator
#   mkCell       { langCfg, userPatchesDir } -> the cell, built from
#                langCfg.depsFile (which must exist)
#   syncRules    langCfg -> the [[deps]] rules of .turnkey/sync.toml, in
#                the order tk sync must run them
#
# `langCfg` is the language's options (nix/buck2/options.nix).
{ pkgs, lib }:

let
  depsCell = import ../lib/deps-cell { inherit pkgs lib; };

  # A language's deps file by name, as sync.toml uses it: depsFile is a
  # path from the flake-parts module, or a file name.
  depsFileName =
    langCfg: default:
    if langCfg.depsFile != null then baseNameOf (toString langCfg.depsFile) else default;

  # Whether a language has a cell to keep in sync: a built one or a deps
  # file to build it from.
  hasCell = langCfg: langCfg.enable && (langCfg.cell != null || langCfg.depsFile != null);
in
[
  {
    name = "go";
    cellName = "godeps";
    description = "Go deps";
    generator = import ../packages/godeps-gen.nix { inherit pkgs lib; };
    mkCell =
      { langCfg, userPatchesDir }:
      depsCell.mkGoDepsCell {
        inherit (langCfg) depsFile;
        inherit userPatchesDir;
        buckgen = import ../packages/buckgen.nix { inherit pkgs lib; };
      };
    # go-deps.toml has a default name, so an enabled Go always has a rule
    syncRules =
      langCfg:
      lib.optional langCfg.enable {
        name = "go";
        sources = [
          langCfg.modFile
          langCfg.sumFile
        ];
        target = depsFileName langCfg "go-deps.toml";
        generator = [
          "godeps-gen"
          "--go-mod"
          langCfg.modFile
          "--go-sum"
          langCfg.sumFile
          "--prefetch"
        ];
      };
  }

  {
    name = "rust";
    cellName = "rustdeps";
    description = "Rust deps";
    generator = import ../packages/rustdeps-gen.nix { inherit pkgs lib; };
    mkCell =
      { langCfg, userPatchesDir }:
      let
        rustFixups = import ../lib/deps-cell/fixups/rust { inherit pkgs lib; };
      in
      depsCell.mkRustDepsCell {
        inherit (langCfg) depsFile;
        inherit userPatchesDir;
        featuresFile =
          if langCfg.featuresFile != null && builtins.pathExists langCfg.featuresFile then
            langCfg.featuresFile
          else
            null;
        genRustBuck = import ../packages/gen-rust-buck.nix { inherit pkgs lib; };
        computeUnifiedFeatures = import ../packages/compute-unified-features.nix { inherit pkgs lib; };
        # The consumer's entries on top of turnkey's defaults
        buildScriptFixups = rustFixups.buildScriptFixups // langCfg.buildScriptFixups;
        rustcFlagsRegistry = rustFixups.rustcFlags // langCfg.rustcFlagsRegistry;
      };
    syncRules =
      langCfg:
      lib.optional (hasCell langCfg) {
        name = "rust";
        sources = [
          langCfg.cargoTomlFile
          langCfg.cargoLockFile
        ];
        target = depsFileName langCfg "rust-deps.toml";
        generator = [
          "rustdeps-gen"
          "--cargo-lock"
          langCfg.cargoLockFile
        ];
      };
  }

  {
    name = "python";
    cellName = "pydeps";
    description = "Python deps";
    generator = import ../packages/pydeps-gen.nix { inherit pkgs lib; };
    mkCell =
      { langCfg, userPatchesDir }:
      depsCell.mkPythonDepsCell {
        inherit (langCfg) depsFile;
        inherit userPatchesDir;
      };
    # With a uv lock, pylock.toml is exported from it first, so a `uv add`
    # reaches python-deps.toml in one tk sync.
    syncRules =
      langCfg:
      lib.optional (hasCell langCfg && langCfg.uvLockFile != null) {
        name = "pylock";
        sources = [
          langCfg.pyprojectFile
          langCfg.uvLockFile
        ];
        target =
          if langCfg.lockFile != null then
            langCfg.lockFile
          else
            throw "turnkey: buck2.python.uvLockFile needs buck2.python.lockFile, the pylock.toml to export it to";
        generator = [
          "uv"
          "export"
          "--all-packages"
          "--no-dev"
          "--format"
          "pylock.toml"
          "--quiet"
        ];
      }
      ++ lib.optional (hasCell langCfg) (
        if langCfg.lockFile != null then
          {
            name = "python";
            sources = [ langCfg.lockFile ];
            target = depsFileName langCfg "python-deps.toml";
            generator = [
              "pydeps-gen"
              "--lock"
              langCfg.lockFile
            ];
          }
        else
          {
            name = "python";
            sources = [ langCfg.pyprojectFile ];
            target = depsFileName langCfg "python-deps.toml";
            generator = [
              "pydeps-gen"
              "--pyproject"
              langCfg.pyprojectFile
            ];
          }
      );
  }

  {
    name = "javascript";
    cellName = "jsdeps";
    description = "JavaScript deps";
    generator = import ../packages/jsdeps-gen.nix { inherit pkgs lib; };
    mkCell =
      { langCfg, userPatchesDir }:
      depsCell.mkJsDepsCell {
        inherit (langCfg) depsFile;
        inherit userPatchesDir;
      };
    syncRules =
      langCfg:
      lib.optional (hasCell langCfg) {
        name = "javascript";
        sources = [ langCfg.lockFile ];
        target = depsFileName langCfg "js-deps.toml";
        generator = [
          "jsdeps-gen"
          "--lock"
          langCfg.lockFile
        ]
        ++ lib.optionals langCfg.includeDevDependencies [ "--include-dev" ];
      };
  }

  {
    name = "solidity";
    cellName = "soldeps";
    description = "Solidity deps";
    generator = import ../packages/soldeps-gen.nix { inherit pkgs lib; };
    # The Solidity adapter builds its cell without the generic builder, so
    # it takes no user patches.
    mkCell =
      { langCfg, userPatchesDir }:
      (import ../lib/deps-cell/adapters/solidity.nix { inherit pkgs lib; }).mkSolDepsCell {
        inherit (langCfg) depsFile;
      };
    syncRules =
      langCfg:
      lib.optional (hasCell langCfg) {
        name = "solidity";
        sources = [ langCfg.foundryTomlFile ];
        target = depsFileName langCfg "solidity-deps.toml";
        generator = [
          "soldeps-gen"
          "--foundry"
          langCfg.foundryTomlFile
        ];
      };
  }
]
