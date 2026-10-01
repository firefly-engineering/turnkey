# The languages turnkey manages dependencies for, one record each.
#
# Everything turnkey knows about a language's dependencies lives in its
# record: the Buck2 cell that holds them, the deps file, the generator that
# writes it and the sync rules that say when to run it. The flake-parts
# module builds the cells from these records, and the devenv module derives
# the cell symlinks, the shell's generators and .turnkey/sync.toml from them,
# so adding a language is a change to this file. sync.toml carries each
# record's cell and deps file to rules sync (src/rust/rules-syncer).
#
# Each record has:
#   name         the option name under turnkey.buck2 (e.g. "go")
#   cellName     the Buck2 cell and the .turnkey/<cellName> symlink
#   cellLink     added to every record: that symlink's path, relative to
#                the project root (the devenv module creates it)
#   description  shown by the shell in verbose mode
#   depsFile     langCfg -> the deps file, relative to the project root:
#                what the language's sync rules write and rules sync
#                maps deps to cellName from (sync.toml's [[languages]])
#   generator    the package providing the deps file's generator
#   mkCell       { cellName, langCfg, userPatchesDir, conditions,
#                resolveFixups } -> the cell named cellName (the
#                record's), built from langCfg.depsFile (which must exist)
#                for the platforms in conditions (nix/buck2/platforms.nix's
#                conditions), each locked dependency with its fixup:
#                resolveFixups is nix/lib/fixups's resolve for the
#                language, applied
#   syncRules    { langCfg, conditions } -> the [[deps]] rules of
#                .turnkey/sync.toml, in the order tk sync must run them,
#                for the platforms in conditions (as mkCell's)
#   wrapper      optional: the native tool tw wraps (`tool`), and
#                langCfg -> its [[wrappers]] rule, which names the sync
#                rule to run when the tool changes the language's files,
#                or null when the tool can't change what the rules read
#
# `langCfg` is the language's options (nix/buck2/options.nix).
{ pkgs, lib }:

let
  depsCell = import ../lib/deps-cell { inherit pkgs lib; };
  platforms = import ./platforms.nix { inherit lib; };

  # A language's deps file as sync.toml names it: relative to the project
  # root, where tk sync runs. depsFile is a relative file name, or a path in
  # the flake (e.g. ./.turnkey/go-deps.toml), which evaluates to a file
  # under the flake's source in the store: what follows the source's
  # directory is its path in the project. A path outside the store has no
  # such prefix, so only its name is kept.
  depsFileName =
    langCfg: default:
    let
      file = langCfg.depsFile;
      inStore = lib.removePrefix "${builtins.storeDir}/" (toString file);
    in
    if file == null then
      default
    else if !builtins.isPath file then
      file
    else if inStore == toString file then
      baseNameOf file
    else
      lib.concatStringsSep "/" (builtins.tail (lib.splitString "/" inStore));

  # Whether a language has a cell to keep in sync: a built one or a deps
  # file to build it from.
  hasCell = langCfg: langCfg.enable && (langCfg.cell != null || langCfg.depsFile != null);

  # Where a cell is linked into the project, relative to its root
  cellLink = cellName: ".turnkey/${cellName}";

  # The files that choose the Go workspace: a go.work at the project root,
  # or the go.mod alone. Its members' go.mod and go.sum come from the deps
  # file, through the sync rule's target_sources.
  goSources = langCfg: [
    "go.work"
    "go.work.sum"
    langCfg.modFile
    langCfg.sumFile
  ];

  # Solidity's root remappings.txt, next to foundry.toml, where forge reads it
  remappingsFile =
    langCfg:
    let
      dir = dirOf langCfg.foundryTomlFile;
    in
    if dir == "." then "remappings.txt" else "${dir}/remappings.txt";
in
map (language: language // { cellLink = cellLink language.cellName; }) [
  rec {
    name = "go";
    cellName = "godeps";
    description = "Go deps";
    depsFile = langCfg: depsFileName langCfg "go-deps.toml";
    generator = import ../packages/godeps-gen.nix { inherit pkgs lib; };
    mkCell =
      {
        cellName,
        langCfg,
        userPatchesDir,
        conditions,
        resolveFixups,
      }:
      depsCell.mkGoDepsCell {
        inherit cellName conditions;
        inherit (langCfg) depsFile allowedBuildTags;
        inherit userPatchesDir resolveFixups;
        buckgen = import ../packages/buckgen.nix { inherit pkgs lib; };
      };
    # go-deps.toml has a default name, so an enabled Go always has a rule.
    # A go.work at the project root makes its members the workspace (ADR
    # 0007), and godeps-gen lists their go.mod and go.sum files in the deps
    # file's `sources`: target_sources makes them sources too
    syncRules =
      { langCfg, ... }:
      lib.optional langCfg.enable {
        name = "go";
        sources = goSources langCfg;
        target_sources = "sources";
        target = depsFile langCfg;
        generator = [
          "godeps-gen"
          "--go-work"
          "go.work"
          "--go-mod"
          langCfg.modFile
          "--go-sum"
          langCfg.sumFile
        ];
      };
    wrapper = {
      tool = "go";
      rule = langCfg: {
        mutating_subcommands = [
          "get"
          "mod"
          "work"
        ];
        watch_files = goSources langCfg;
        deps_rule = "go";
        post_commands = [ "go mod tidy" ];
      };
    };
  }

  rec {
    name = "rust";
    cellName = "rustdeps";
    description = "Rust deps";
    depsFile = langCfg: depsFileName langCfg "rust-deps.toml";
    generator = import ../packages/rustdeps-gen.nix { inherit pkgs lib; };
    mkCell =
      {
        cellName,
        langCfg,
        userPatchesDir,
        conditions,
        resolveFixups,
      }:
      depsCell.mkRustDepsCell {
        inherit cellName;
        inherit (langCfg) depsFile;
        inherit userPatchesDir resolveFixups conditions;
        rustRulesGen = import ../packages/rust-rules-gen.nix { inherit pkgs lib; };
      };
    # rustdeps-gen resolves each crate's package slice with cargo, once per
    # platform, and lists every workspace member's Cargo.toml in the deps
    # file's `manifests`: target_sources makes them sources too
    syncRules =
      { langCfg, conditions }:
      lib.optional (hasCell langCfg) {
        name = "rust";
        sources = [
          langCfg.cargoTomlFile
          langCfg.cargoLockFile
        ];
        target_sources = "manifests";
        target = depsFile langCfg;
        generator = [
          "rustdeps-gen"
          "--cargo-lock"
          langCfg.cargoLockFile
        ]
        ++ builtins.concatMap (platform: [
          "--platform"
          "${platforms.name platform}=${platforms.rustTarget platform}"
        ]) conditions.platforms;
      };
    wrapper = {
      tool = "cargo";
      rule = langCfg: {
        mutating_subcommands = [
          "add"
          "remove"
          "update"
        ];
        watch_files = [
          langCfg.cargoTomlFile
          langCfg.cargoLockFile
        ];
        deps_rule = "rust";
      };
    };
  }

  rec {
    name = "python";
    cellName = "pydeps";
    description = "Python deps";
    depsFile = langCfg: depsFileName langCfg "python-deps.toml";
    generator = import ../packages/pydeps-gen.nix { inherit pkgs lib; };
    mkCell =
      {
        cellName,
        langCfg,
        userPatchesDir,
        conditions,
        resolveFixups,
      }:
      depsCell.mkPythonDepsCell {
        inherit cellName conditions;
        pydepsCell = import ../packages/pydeps-cell.nix { inherit pkgs lib; };
        inherit (langCfg) depsFile;
        inherit userPatchesDir resolveFixups;
      };
    # With a uv lock, pylock.toml is exported from it first, so a `uv add`
    # reaches python-deps.toml in one tk sync.
    syncRules =
      { langCfg, ... }:
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
            # uv.lock holds the dependency graph: each dependency's marker,
            # each package's extras
            sources = [ langCfg.lockFile ] ++ lib.optional (langCfg.uvLockFile != null) langCfg.uvLockFile;
            target = depsFile langCfg;
            generator = [
              "pydeps-gen"
              "--lock"
              langCfg.lockFile
            ]
            ++ lib.optionals (langCfg.uvLockFile != null) [
              "--uv-lock"
              langCfg.uvLockFile
            ];
          }
        else
          {
            name = "python";
            sources = [ langCfg.pyprojectFile ];
            target = depsFile langCfg;
            generator = [
              "pydeps-gen"
              "--pyproject"
              langCfg.pyprojectFile
            ];
          }
      );
    # uv changes pyproject.toml and uv.lock. With a uv lock, syncing starts
    # at the pylock rule; without a lock file, the python rule reads
    # pyproject.toml. A lock file uv doesn't export is not uv's to change.
    wrapper = {
      tool = "uv";
      rule =
        langCfg:
        if langCfg.lockFile != null && langCfg.uvLockFile == null then
          null
        else
          {
            mutating_subcommands = [
              "add"
              "remove"
              "lock"
              "sync"
            ];
            watch_files = [
              langCfg.pyprojectFile
            ]
            ++ lib.optional (langCfg.uvLockFile != null) langCfg.uvLockFile;
            deps_rule = if langCfg.uvLockFile != null then "pylock" else "python";
          };
    };
  }

  rec {
    name = "javascript";
    cellName = "jsdeps";
    description = "JavaScript deps";
    depsFile = langCfg: depsFileName langCfg "js-deps.toml";
    generator = import ../packages/jsdeps-gen.nix { inherit pkgs lib; };
    mkCell =
      {
        cellName,
        langCfg,
        userPatchesDir,
        conditions,
        resolveFixups,
      }:
      depsCell.mkJsDepsCell {
        inherit cellName conditions;
        inherit (langCfg) depsFile;
        inherit userPatchesDir resolveFixups;
      };
    syncRules =
      { langCfg, ... }:
      lib.optional (hasCell langCfg) {
        name = "javascript";
        sources = [ langCfg.lockFile ];
        target = depsFile langCfg;
        generator = [
          "jsdeps-gen"
          "--lock"
          langCfg.lockFile
        ]
        ++ lib.optionals langCfg.includeDevDependencies [ "--include-dev" ];
      };
  }

  rec {
    name = "solidity";
    cellName = "soldeps";
    description = "Solidity deps";
    # The root remappings.txt the sync writes, which the Buck2 Solidity rules
    # stage too (nix/devenv/turnkey/buck2.nix's .buckconfig [solidity])
    inherit remappingsFile;
    depsFile = langCfg: depsFileName langCfg "solidity-deps.toml";
    generator = import ../packages/soldeps-gen.nix { inherit pkgs lib; };
    mkCell =
      {
        cellName,
        langCfg,
        userPatchesDir,
        conditions,
        resolveFixups,
      }:
      depsCell.mkSolDepsCell {
        inherit cellName;
        inherit (langCfg) depsFile;
        inherit userPatchesDir resolveFixups;
      };
    # solidity-deps.toml first, then the root remappings.txt generated from
    # it: a rule has one target, so each file gets its own rule, run in
    # order as python's pylock and python rules are
    syncRules =
      { langCfg, ... }:
      let
        # Where the cell vendors packages (nix/lib/deps-cell): remapping
        # overrides in foundry.toml must point inside it, and the root
        # remappings.txt targets it. Both are relative to foundry.toml's
        # directory, which soldeps-gen reads from --foundry.
        foundryAndVendorDir = [
          "--foundry"
          langCfg.foundryTomlFile
          "--vendor-dir"
          "${cellLink cellName}/vendor/"
        ];
      in
      lib.optionals (hasCell langCfg) [
        {
          name = "solidity";
          # Git deps come from foundry.toml, npm Solidity packages from
          # package.json at the versions the pnpm lock pins
          sources = [
            langCfg.foundryTomlFile
            langCfg.packageJsonFile
            langCfg.pnpmLockFile
          ];
          target = depsFile langCfg;
          generator = [
            "soldeps-gen"
            "--package-json"
            langCfg.packageJsonFile
            "--pnpm-lock"
            langCfg.pnpmLockFile
            # The file this run replaces: git packages whose pin is unchanged
            # keep the remapping target recorded there, npm packages their
            # verdict on holding Solidity
            "--previous"
            (depsFile langCfg)
          ]
          ++ foundryAndVendorDir;
        }
        {
          name = "solidity-remappings";
          # Each package's recorded remapping, for native forge and the Buck2
          # Solidity rules, with targets under the cell link's vendor/
          sources = [ (depsFile langCfg) ];
          target = remappingsFile langCfg;
          generator = [
            "soldeps-gen"
            "remappings"
            "--deps"
            (depsFile langCfg)
          ]
          ++ foundryAndVendorDir;
        }
      ];
  }
]
