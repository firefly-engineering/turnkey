{
  description = "Turnkey toolchain management for Nix flakes";

  inputs = {
    nix-pins.url = "github:firefly-engineering/nix-pins";
    nixpkgs.follows = "nix-pins/nixpkgs";
    flake-parts.follows = "nix-pins/flake-parts";

    # Required by devenv for container support (even if unused)
    nix2container = {
      url = "github:nlewo/nix2container";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    mk-shell-bin.url = "github:rrbutani/nix-mk-shell-bin";

    # Teller - versioned toolchain registry library
    teller = {
      url = "github:firefly-engineering/teller";
      inputs.nix-pins.follows = "nix-pins";
    };

    # Toolbox - package registry (provides beads, beads_viewer, jj, go, rust, etc.)
    toolbox = {
      url = "github:firefly-engineering/toolbox";
      inputs.nix-pins.follows = "nix-pins";
      inputs.teller.follows = "teller";
    };

    devenv.follows = "toolbox/devenv";

    # Placeholder input that downstream CI can override to inject the
    # working directory at flake-eval time (devenv needs a concrete
    # project root to evaluate devShells outside of `nix develop`).
    # Override with: --override-input devenv-root file+file://$PWD/.devenv-root
    devenv-root = {
      url = "file+file:///dev/null";
      flake = false;
    };
  };

  outputs =
    inputs@{
      self,
      flake-parts,
      nixpkgs,
      ...
    }:
    let
      # nixpkgs with teller's and toolbox's overlays: turnkey's own registry
      toolboxPkgs =
        system:
        import inputs.nixpkgs {
          inherit system;
          overlays = [
            inputs.teller.overlays.default
            inputs.toolbox.overlays.default
          ];
        };
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      # Reusable helpers exposed at the flake's lib output. Surfacing
      # these makes the teller+toolbox setup that turnkey bundles
      # available to consumers outside the flake-parts module — useful
      # for one-off Nix expressions, NixOS modules, or downstream test
      # harnesses that need the same registry construction.
      #
      # defaultTellerLib is system-agnostic; defaultTellerRegistry is a
      # function-of-system because nixpkgs evaluation is per-system and
      # flake.lib stays system-agnostic by convention.
      flake.lib = {
        defaultTellerLib = inputs.teller.lib;
        defaultTellerRegistry = system: (toolboxPkgs system).turnkeyRegistry;

        # The pinned buck2 release (nix/buck2/buck2-source.nix): the binary,
        # the upstream prelude, turnkey's patched prelude, the protocol
        # sources and the version, all from turnkey's own registry
        # (docs/adr/0002-turnkey-owns-the-buck2-version.md). Build it once
        # per system and read fields from it.
        pinnedBuck2Release =
          system:
          let
            pkgs = toolboxPkgs system;
          in
          import ./nix/buck2/buck2-source.nix {
            inherit pkgs;
            inherit (pkgs) lib;
            registry = pkgs.turnkeyRegistry;
          };

        # Single source of truth for the packages worth caching/publishing.
        # Consumed by .github/workflows/cachix.yaml. Two categories are
        # deliberately excluded:
        #   - Project-specific outputs (per-language *-cell derivations and
        #     the toolchain-profile buildEnv) — downstream consumers build
        #     their own from their own toolchain.toml.
        #   - turnkey-composed — a macFUSE daemon that needs the kext
        #     installed at build time. Linux runners lack pkg-config + the
        #     libfuse dev libs; macOS runners can't install macFUSE
        #     (system-extension prompts can't be answered in CI). Users who
        #     actually run the daemon install macFUSE locally and build it
        #     once on their own machine.
        publicPackages = [
          # Deps generators
          "godeps-gen"
          "pydeps-gen"
          "rustdeps-gen"
          "jsdeps-gen"
          "soldeps-gen"
          # Turnkey CLIs
          "tk"
          "tw"
          # Native-tool wrappers (tw-driven)
          "tw-go"
          "tw-cargo"
          "tw-uv"
          # Buck2 prelude + build helpers
          "turnkey-prelude"
          "deps-extract"
          "buckgen"
          "cargo-prune-workspace"
          "compute-unified-features"
          "gen-rust-buck"
          # Misc
          "nix-prefetch-cached"
          "pytest-uv-shim"
          "check-rust-edition-rs"
          "check-source-coverage-rs"
          "e2e-runner"
        ];
      };

      # Export the turnkey flake-parts module. We import it with
      # turnkeyLib = self.lib so the module can reach the bundled teller
      # and toolbox helpers without re-importing turnkey's own flake.
      # Consumers don't need to do anything different — flakeModules.turnkey
      # is still the same value to import in their flake-parts setup.
      flake.flakeModules = {
        turnkey = import ./nix/flake-parts/turnkey {
          turnkeyLib = self.lib;
          devenvRoot = inputs.devenv-root;
        };
      };

      # turnkey's own fixup set, one module per family plus `default`
      # (nix/fixups, docs/adr/0003-fixup-sets-are-modules.md): what
      # turnkey's repository needs, for other repositories to import
      flake.modules.turnkeyFixups = import ./nix/fixups;

      # Export home-manager module for turnkey-composed service
      flake.homeManagerModules = {
        turnkey-composed = ./nix/home-manager/turnkey-composed.nix;
      };

      # Flake templates for project initialization
      flake.templates = {
        default = {
          path = ./templates/default;
          description = "Buck2 project with turnkey toolchain management";
        };
      };

      # Use the module ourselves as a working example. Import via the
      # wrapped flakeModule export above so we eat our own dog food
      # (same closure consumers receive).
      imports = [
        inputs.devenv.flakeModule
        # flake.modules, where the fixup set is published
        flake-parts.flakeModules.modules
        (import ./nix/flake-parts/turnkey {
          turnkeyLib = self.lib;
          devenvRoot = inputs.devenv-root;
        })
      ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem =
        {
          config,
          pkgs,
          lib,
          system,
          ...
        }:
        {
          # Export tools as packages
          packages.godeps-gen = import ./nix/packages/godeps-gen.nix { inherit pkgs lib; };
          packages.nix-prefetch-cached = import ./nix/packages/nix-prefetch-cached.nix { inherit pkgs lib; };
          packages.pydeps-gen = import ./nix/packages/pydeps-gen.nix { inherit pkgs lib; };
          packages.rustdeps-gen = import ./nix/packages/rustdeps-gen.nix { inherit pkgs lib; };
          packages.buckgen = import ./nix/packages/buckgen.nix { inherit pkgs lib; };
          packages.cargo-prune-workspace = import ./nix/packages/cargo-prune-workspace.nix {
            inherit pkgs lib;
          };
          packages.tk = import ./nix/packages/tk.nix {
            inherit pkgs lib;
            inherit ((self.lib.pinnedBuck2Release system)) buck2;
          };
          packages.tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
          packages.e2e-runner = import ./nix/packages/e2e-runner.nix { inherit pkgs lib; };
          packages.jsdeps-gen = import ./nix/packages/jsdeps-gen.nix { inherit pkgs lib; };
          packages.soldeps-gen = import ./nix/packages/soldeps-gen.nix { inherit pkgs lib; };
          packages.deps-extract = import ./nix/packages/deps-extract.nix { inherit pkgs lib; };
          packages.turnkey-composed = import ./nix/packages/turnkey-composed.nix { inherit pkgs lib; };

          # Internal-but-cross-project derivations: invoked by the
          # flake-parts module or rust-deps-cell builder rather than by
          # name, so they need to be at packages.* for the cachix
          # workflow to publish them under stable names.
          packages.compute-unified-features = import ./nix/packages/compute-unified-features.nix {
            inherit pkgs lib;
          };
          packages.gen-rust-buck = import ./nix/packages/gen-rust-buck.nix { inherit pkgs lib; };
          packages.pytest-uv-shim = import ./nix/packages/pytest-uv-shim.nix { inherit pkgs lib; };
          packages.check-rust-edition-rs = import ./nix/packages/check-rust-edition-rs.nix {
            inherit pkgs lib;
          };
          packages.check-source-coverage-rs = import ./nix/packages/check-source-coverage-rs.nix {
            inherit pkgs lib;
          };
          # tw-<tool>, one per native tool tw wraps (nix/packages/tw-wrappers.nix)
          imports = [
            (
              { pkgs, lib, ... }:
              {
                packages =
                  (import ./nix/packages/tw-wrappers.nix {
                    inherit pkgs lib;
                    tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
                  }).packages;
              }
            )
          ];

          # turnkey-test-runner, built for the pinned buck2 release
          packages.turnkey-test-runner = import ./nix/packages/turnkey-test-runner.nix {
            inherit pkgs lib;
          };

          # Expose turnkey-prelude for CI builds: the pinned buck2 release's
          # prelude, as the dev shell uses it
          packages.turnkey-prelude = (self.lib.pinnedBuck2Release system).prelude;

          # `nix fmt` formats the Nix files with the nixfmt checks.nix-format
          # holds them to
          formatter = pkgs.nixfmt;

          # Every Nix file is formatted as `nix fmt` formats it. The flake's
          # source holds only tracked files, so nothing untracked is read.
          checks.nix-format =
            let
              src = lib.fileset.toSource {
                root = ./.;
                fileset = lib.fileset.fileFilter (file: file.hasExt "nix") ./.;
              };
            in
            pkgs.runCommand "nix-format-check" { nativeBuildInputs = [ config.formatter ]; } ''
              cd ${src}
              if ! find . -name '*.nix' -print0 | xargs -0 nixfmt --check; then
                echo "Nix files are not formatted: run nix fmt" >&2
                exit 1
              fi
              touch $out
            '';

          # The pinned buck2 release holds together: the binary and prelude
          # are the release's, and the shell gets exactly what the flake
          # publishes. Checked at evaluation, so `nix flake check --no-build`
          # runs it.
          checks.pinned-buck2-release =
            let
              release = self.lib.pinnedBuck2Release system;
              shellBuck2 = config.devenv.shells.default.turnkey.buck2;
            in
            assert lib.assertMsg (
              release.buck2.version == release.version
            ) "pinned buck2 release: binary is ${release.buck2.version}, not ${release.version}";
            assert lib.assertMsg (release.upstreamPrelude.version == release.version)
              "pinned buck2 release: upstream prelude is ${release.upstreamPrelude.version}, not ${release.version}";
            assert lib.assertMsg (
              shellBuck2.package.drvPath == release.buck2.drvPath
            ) "pinned buck2 release: the default shell's buck2 is not the pinned binary";
            assert lib.assertMsg (
              shellBuck2.prelude.path == null
              && shellBuck2.prelude.package.drvPath == config.packages.turnkey-prelude.drvPath
            ) "pinned buck2 release: the default shell's prelude is not packages.turnkey-prelude";
            pkgs.runCommand "pinned-buck2-release-check" { } "touch $out";

          # buck2 comes only from the pinned release
          # (docs/adr/0002-turnkey-owns-the-buck2-version.md): declaring it is
          # an error, the toolchain profile gets the pinned one, and no Nix
          # file reaches for another.
          checks.toolchain-declaration =
            let
              release = self.lib.pinnedBuck2Release system;
              resolve = (import ./nix/lib/toolchain-declaration.nix { inherit lib; }).resolve;
              declaringBuck2 = resolve {
                tellerLib = self.lib.defaultTellerLib;
                registry = self.lib.defaultTellerRegistry system;
                # A file in the flake source, not builtins.toFile: `nix flake
                # check --no-build` evaluates in read-only mode, where a
                # toFile path is never written and so cannot be read back.
                declarationFile = ./nix/lib/testdata/declares-buck2.toml;
              };
              nixFiles = builtins.filter (lib.hasSuffix ".nix") (lib.filesystem.listFilesRecursive ./nix);
              reachesForBuck2 =
                file:
                let
                  text = builtins.readFile file;
                in
                builtins.any (use: lib.hasInfix use text) [
                  "pkgs.buck2}"
                  "pkgs.buck2;"
                  "pkgs.buck2 "
                ];
              strayBuck2 = builtins.filter reachesForBuck2 nixFiles;
            in
            assert lib.assertMsg (
              !(builtins.tryEval declaringBuck2).success
            ) "toolchain declaration: declaring buck2 in toolchain.toml resolves instead of failing";
            assert lib.assertMsg (builtins.any (pkg: pkg.drvPath == release.buck2.drvPath)
              config.packages.toolchain-profile.toolchainPackages
            ) "toolchain declaration: the toolchain profile lacks the pinned buck2";
            assert lib.assertMsg (strayBuck2 == [ ])
              "toolchain declaration: ${
                lib.concatMapStringsSep ", " toString strayBuck2
              } use a buck2 other than the pinned one";
            pkgs.runCommand "toolchain-declaration-check" { } "touch $out";

          # The files turnkey generates for Buck2, through the pure functions
          # that render them, with a stand-in registry: the toolchains cell
          # (nix/buck2/toolchains-cell.nix), the .buckconfig
          # (nix/buck2/buckconfig.nix) and .turnkey/sync.toml
          # (nix/buck2/sync-config.nix). Checked at evaluation.
          checks.buck2-generators =
            let
              # Every registry name, each a store-path-like stand-in
              registry = lib.mapAttrs (name: _: { outPath = "/nix/store/stand-in-${name}"; }) (
                self.lib.defaultTellerRegistry system
              );
              preprocessor = {
                outPath = "/nix/store/stand-in-mdbook-admonish";
              };
              toolchainsCell =
                args:
                import ./nix/buck2/toolchains-cell.nix { inherit lib; } (
                  {
                    mappings = import ./nix/buck2/mappings.nix { inherit lib; };
                    resolvedRegistry = registry;
                  }
                  // args
                );
              goCell = toolchainsCell { declaredToolchains.go = { }; };
              mdbookCell = toolchainsCell {
                mappings = import ./nix/buck2/mappings.nix {
                  inherit lib;
                  mdbookPreprocessors = [ preprocessor ];
                };
                declaredToolchains.mdbook = { };
              };
              plainMdbookCell = toolchainsCell {
                resolvedRegistry = builtins.removeAttrs registry [ "mdbook-toolchain" ];
                declaredToolchains.mdbook = { };
              };

              buckconfig =
                testCache:
                import ./nix/buck2/buckconfig.nix { inherit lib; } {
                  cells = [
                    {
                      name = "godeps";
                      path = ".turnkey/godeps";
                    }
                    {
                      name = "prelude";
                      path = ".turnkey/prelude";
                    }
                  ];
                  toolchainsCellPath = ".turnkey/toolchains";
                  testRunnerProtocol = "/nix/store/stand-in-protocol";
                  inherit testCache;
                };
              uncached = buckconfig null;
              cached = buckconfig {
                runner = "/nix/store/stand-in-runner";
                path = "/nix/store/stand-in-bash/bin";
                address = "grpc://127.0.0.1:47301";
                tls = false;
              };

              buck2Options =
                (lib.evalModules {
                  modules = [
                    (import ./nix/buck2/options.nix {
                      inherit lib;
                      version = "check";
                    })
                    {
                      go.depsFile = "go-deps.toml";
                      python = {
                        depsFile = "python-deps.toml";
                        lockFile = "pylock.toml";
                        uvLockFile = "uv.lock";
                      };
                      rust.enable = false;
                      javascript.enable = false;
                      solidity.enable = false;
                    }
                  ];
                }).config;
              syncConfig = import ./nix/buck2/sync-config.nix { inherit pkgs lib; } {
                languages = import ./nix/buck2/languages.nix { inherit pkgs lib; };
                buck2 = buck2Options;
              };
              syncToml = syncConfig.value;
              platforms = import ./nix/buck2/platforms.nix { inherit lib; };
              platformSettings = platforms.settingsBuckFile (map platforms.fromSystem buck2Options.platforms) [ ];
              taggedSettings = platforms.settingsBuckFile (map platforms.fromSystem [ "x86_64-linux" ]) [
                "integration"
              ];
              taggedBuckconfig = import ./nix/buck2/buckconfig.nix { inherit lib; } {
                cells = [ ];
                toolchainsCellPath = ".turnkey/toolchains";
                testRunnerProtocol = "/nix/store/stand-in-protocol";
                testCache = null;
                goAllowedBuildTags = [
                  "integration"
                  "e2e"
                ];
              };

              # How the shell describes the cache to tk, as tk's tests read it
              shellContract = builtins.fromJSON (
                builtins.readFile ./src/go/pkg/testcache/testdata/shell-contract.json
              );
              describes =
                example:
                let
                  env =
                    (import ./nix/buck2/test-cache.nix { inherit lib; } {
                      testCache = example.options // {
                        enable = true;
                      };
                      enabled = true;
                      runner = "/nix/store/stand-in-runner";
                      path = "/nix/store/stand-in-bash/bin";
                      inherit (example) port server;
                    }).env;
                in
                env ? ${shellContract.env} && builtins.fromJSON env.${shellContract.env} == example.descriptor;
            in
            assert lib.assertMsg (lib.all (name: builtins.elem name goCell.toolchains) [
              "go"
              "python"
              "cxx"
              "genrule"
            ]) "toolchains cell: go brings ${toString goCell.toolchains}, not go, python, cxx and genrule";
            assert lib.assertMsg (builtins.elem "lld" goCell.runtimeDeps)
              "toolchains cell: cxx's actions don't get lld on PATH";
            assert lib.assertMsg (lib.hasInfix ''"${preprocessor}/bin"'' mdbookCell.buckFile)
              "toolchains cell: mdbook lacks the configured preprocessor's bin/";
            assert lib.assertMsg (
              !(lib.hasInfix "preprocessor_paths" plainMdbookCell.buckFile)
            ) "toolchains cell: mdbook gets preprocessor_paths with none configured";
            assert lib.assertMsg (
              lib.hasInfix "godeps = .turnkey/godeps" uncached
              && lib.hasInfix "target:godeps//...->prelude//platforms:default" uncached
            ) "buckconfig: a Nix-backed cell is missing from [cells] or the platform detectors";
            assert lib.assertMsg (
              !(lib.hasInfix "[test]" uncached) && !(lib.hasInfix "[buck2_re_client]" uncached)
            ) "buckconfig: test result caching is configured with the test cache off";
            assert lib.assertMsg (
              lib.hasInfix "v2_test_executor = /nix/store/stand-in-runner/bin/turnkey-test-runner" cached
              && lib.hasInfix "action_cache_address = grpc://127.0.0.1:47301" cached
              && lib.hasInfix "tls = false" cached
            ) "buckconfig: the test cache's runner, endpoint or TLS setting is missing";
            assert lib.assertMsg
              (
                map (rule: rule.name) syncToml.deps == [
                  "go"
                  "pylock"
                  "python"
                ]
              )
              "sync.toml: [[deps]] are ${toString (map (rule: rule.name) syncToml.deps)}, not go, pylock, python";
            assert lib.assertMsg (
              map (wrapper: wrapper.deps_rule) syncToml.wrappers == [
                "go"
                "pylock"
              ]
            ) "sync.toml: the go and uv wrappers don't run the go and pylock rules";
            assert lib.assertMsg (lib.all describes shellContract.caches)
              "test cache: the shell doesn't describe the cache as testdata/shell-contract.json says";
            assert lib.assertMsg (
              syncToml.conditions.settings == "toolchains//conditions"
              &&
                syncToml.conditions.platforms == [
                  {
                    os = "linux";
                    cpu = "x86_64";
                  }
                  {
                    os = "linux";
                    cpu = "arm64";
                  }
                  {
                    os = "macos";
                    cpu = "x86_64";
                  }
                  {
                    os = "macos";
                    cpu = "arm64";
                  }
                ]
            ) "sync.toml: [conditions] doesn't list buck2.platforms' default four platforms in Buck2's names";
            assert lib.assertMsg (
              lib.hasInfix ''name = "macos-arm64"'' platformSettings
              && lib.hasInfix ''"prelude//cpu/constraints:cpu[arm64]"'' platformSettings
            ) "toolchains cell: no combined config_setting for macos-arm64";
            assert lib.assertMsg (
              lib.hasInfix ''name = "linux-integration"'' taggedSettings
              && lib.hasInfix ''name = "linux-x86_64-no_integration"'' taggedSettings
              && lib.hasInfix ''"prelude//go/tags/constraints:integration[set]"'' taggedSettings
            ) "toolchains cell: no config_setting combining the OS and an allowed Go build tag";
            assert lib.assertMsg (lib.hasInfix "allowed_build_tags = integration,e2e" taggedBuckconfig)
              "buckconfig: buck2.go.allowedBuildTags doesn't reach go.allowed_build_tags";
            assert lib.assertMsg (
              syncToml.conditions.go_tags == [ ]
            ) "sync.toml: [conditions] go_tags isn't buck2.go.allowedBuildTags";
            pkgs.runCommand "buck2-generators-check" { } "touch $out";

          # nix/buck2/platforms.nix's split agrees with the conditions
          # module's (src/go/pkg/conditions) on its test cases, and every
          # combined key it writes names a config_setting settingsBuckFile
          # defines. Cases with Go build tag dimensions are the Go module's
          # alone. Checked at evaluation.
          checks.split-vectors =
            let
              platforms = import ./nix/buck2/platforms.nix { inherit lib; };
              vectors = builtins.fromJSON (builtins.readFile ./src/go/pkg/conditions/testdata/split-vectors.json);
              cases = builtins.filter (case: case.dimensions == [ ]) vectors.cases;
              # A platform's labels: those of every rule whose when it includes
              labelsOf =
                case: platform:
                lib.concatMap (
                  rule:
                  lib.optionals (lib.all (dim: platform.${dim} == rule.when.${dim}) (
                    lib.attrNames rule.when
                  )) rule.labels
                ) case.labels;
              got = case: platforms.split { inherit (case) settings platforms; } (labelsOf case);
              want = case: {
                inherit (case) common;
                branches = map (branch: {
                  inherit (branch) key;
                  values = branch.labels;
                }) case.branches;
              };
              splitProblems = map (
                case: "${case.name}: ${builtins.toJSON (got case)}, want ${builtins.toJSON (want case)}"
              ) (builtins.filter (case: got case != want case) cases);
              # The <os>-<cpu> part of each combined key
              settingNames =
                case:
                map (branch: lib.removePrefix "${case.settings}:" branch.key) (
                  builtins.filter (branch: lib.hasPrefix "${case.settings}:" branch.key) case.branches
                );
              namingProblems = lib.concatMap (
                case:
                let
                  buckFile = platforms.settingsBuckFile case.platforms [ ];
                in
                map (name: "${case.name}: no config_setting named ${name}") (
                  builtins.filter (name: !(lib.hasInfix ''name = "${name}",'' buckFile)) (settingNames case)
                )
              ) cases;
            in
            assert lib.assertMsg (
              cases != [ ] && lib.concatMap settingNames cases != [ ]
            ) "split vectors: no platform-only cases, or none with a combined key";
            assert lib.assertMsg (
              splitProblems == [ ]
            ) "split vectors: ${lib.concatStringsSep "; " splitProblems}";
            assert lib.assertMsg (
              namingProblems == [ ]
            ) "split vectors: ${lib.concatStringsSep "; " namingProblems}";
            pkgs.runCommand "split-vectors-check" { } "touch $out";

          # .turnkey/sync.toml as the shell writes it for a project with every
          # language, byte for byte what rulessync's tests read
          # (src/go/pkg/rulessync/testdata/sync.toml): the seam between the
          # language records and rules sync, checked from both sides.
          checks.sync-config-contract =
            let
              buck2Options =
                (lib.evalModules {
                  modules = [
                    (import ./nix/buck2/options.nix {
                      inherit lib;
                      version = "check";
                    })
                    {
                      rules.enabled = true;
                      go = {
                        depsFile = "go-deps.toml";
                        allowedBuildTags = [ "integration" ];
                      };
                      rust.depsFile = "rust-deps.toml";
                      python = {
                        depsFile = "python-deps.toml";
                        lockFile = "pylock.toml";
                        uvLockFile = "uv.lock";
                      };
                      javascript.depsFile = "js-deps.toml";
                      solidity.depsFile = "solidity-deps.toml";
                    }
                  ];
                }).config;
              syncConfig = import ./nix/buck2/sync-config.nix { inherit pkgs lib; } {
                languages = import ./nix/buck2/languages.nix { inherit pkgs lib; };
                buck2 = buck2Options;
              };
              fixture = ./src/go/pkg/rulessync/testdata/sync.toml;
            in
            pkgs.runCommand "sync-config-contract-check" { } ''
              if ! diff -u ${fixture} ${syncConfig.file}; then
                echo "sync.toml changed: copy ${syncConfig.file} to src/go/pkg/rulessync/testdata/sync.toml" >&2
                exit 1
              fi
              touch $out
            '';

          # The language records (nix/buck2/languages.nix) agree with
          # themselves: rule names are unique, a rule runs after the rule
          # that writes its source, and each wrapper runs a rule of its own
          # language that reads a file the wrapper watches. Checked with and
          # without a uv lock, at evaluation.
          checks.language-records =
            let
              languages = import ./nix/buck2/languages.nix { inherit pkgs lib; };
              buck2Options =
                python:
                (lib.evalModules {
                  modules = [
                    (import ./nix/buck2/options.nix {
                      inherit lib;
                      version = "check";
                    })
                    {
                      go.depsFile = "go-deps.toml";
                      rust.depsFile = "rust-deps.toml";
                      python = python // {
                        depsFile = "python-deps.toml";
                      };
                      javascript.depsFile = "js-deps.toml";
                      solidity.depsFile = "solidity-deps.toml";
                    }
                  ];
                }).config;
              problems =
                python:
                let
                  options = buck2Options python;
                  rulesOf = language: language.syncRules options.${language.name};
                  rules = builtins.concatMap rulesOf languages;
                  names = map (rule: rule.name) rules;
                  indexOf = name: lib.lists.findFirstIndex (n: n == name) null names;
                  writerOf = file: lib.findFirst (rule: rule.target == file) null rules;
                  misordered = builtins.filter (
                    rule:
                    builtins.any (
                      source:
                      let
                        writer = writerOf source;
                      in
                      writer != null && indexOf writer.name > indexOf rule.name
                    ) rule.sources
                  ) rules;
                  wrapperProblems = builtins.concatMap (
                    language:
                    let
                      wrapper = language.wrapper.rule options.${language.name};
                      rule =
                        if wrapper == null then
                          null
                        else
                          lib.findFirst (rule: rule.name == wrapper.deps_rule) null (rulesOf language);
                    in
                    if wrapper == null then
                      [ ]
                    else if rule == null then
                      [ "${language.wrapper.tool} runs ${wrapper.deps_rule}, not a ${language.name} rule" ]
                    else
                      lib.optional (lib.intersectLists wrapper.watch_files rule.sources == [ ])
                        "${language.wrapper.tool} watches ${toString wrapper.watch_files}, none of which ${rule.name} reads"
                  ) (builtins.filter (language: language ? wrapper) languages);
                  # solidity-deps.toml comes from foundry.toml and from the
                  # Solidity packages in package.json, pinned by the pnpm
                  # lock, so the rule must both read and watch all three
                  solidityRule = lib.findFirst (rule: rule.name == "solidity") null rules;
                  solidityInputs = [
                    options.solidity.foundryTomlFile
                    options.solidity.packageJsonFile
                    options.solidity.pnpmLockFile
                  ];
                  readAndWatched = file: lib.elem file solidityRule.sources && lib.elem file solidityRule.generator;
                  solidityProblems = map (file: "solidity does not read and watch ${file}") (
                    builtins.filter (file: !readAndWatched file) solidityInputs
                  );
                in
                lib.optional (lib.unique names != names) "rule names repeat: ${toString names}"
                ++ map (rule: "${rule.name} runs before the rule that writes its sources") misordered
                ++ wrapperProblems
                ++ solidityProblems;
              allProblems =
                problems { }
                ++ problems { lockFile = "pylock.toml"; }
                ++ problems {
                  lockFile = "pylock.toml";
                  uvLockFile = "uv.lock";
                };
              # A deps file given as a path in the flake keeps its directory
              # in the project
              goRecord = lib.findFirst (language: language.name == "go") null languages;
              nestedGoRule = builtins.head (
                goRecord.syncRules ((buck2Options { }).go // { depsFile = ./.turnkey/go-deps.toml; })
              );
            in
            assert lib.assertMsg (
              allProblems == [ ]
            ) "language records: ${lib.concatStringsSep "; " allProblems}";
            assert lib.assertMsg (
              nestedGoRule.target == ".turnkey/go-deps.toml"
            ) "language records: depsFile ./.turnkey/go-deps.toml is synced to ${nestedGoRule.target}";
            pkgs.runCommand "language-records-check" { } "touch $out";

          # Fixup sets (docs/adr/0003-fixup-sets-are-modules.md): sets
          # published by other flakes merge with a repository's own
          # fixups, per field; conflicts fail; enable = false drops a
          # fixup; only the repository's own unused fixups warn; version
          # entries and OS/CPU overlays resolve per locked dependency.
          # Fixtures in nix/lib/testdata/fixups. Checked at evaluation.
          checks.fixup-sets =
            let
              fixups = import ./nix/lib/fixups { inherit lib; };
              testdata = ./nix/lib/testdata/fixups;
              acme = import (testdata + "/acme.nix");
              other = import (testdata + "/other.nix");
              conflicting = import (testdata + "/conflicting.nix");
              inline = {
                _file = "inline.nix";
                rust.serde.rustcFlags = [
                  "--cfg"
                  "mine"
                ];
                rust.typo-crate.buildScript.skip = true;
              };
              platform = {
                system = "x86_64-linux";
                os = "linux";
                cpu = "x86_64";
              };
              locked = [
                {
                  key = "serde@1.0.219";
                  name = "serde";
                  version = "1.0.219";
                }
                {
                  key = "ring@0.17.14";
                  name = "ring";
                  version = "0.17.14";
                }
                {
                  key = "ring@0.16.20";
                  name = "ring";
                  version = "0.16.20";
                }
                {
                  key = "rustix@1.0.7";
                  name = "rustix";
                  version = "1.0.7";
                }
              ];
              resolveWith =
                modules: language: deps:
                fixups.resolve {
                  evaluated = fixups.evalFixups { inherit pkgs modules; };
                  inherit language deps platform;
                  inlineFiles = [ "inline.nix" ];
                };
              rust = resolveWith [
                acme
                other
                inline
              ] "rust" locked;
              fails = value: !(builtins.tryEval (builtins.deepSeq value value)).success;
              serde = rust.fixups."serde@1.0.219";
              ring17 = rust.fixups."ring@0.17.14";
              ring16 = rust.fixups."ring@0.16.20";
              after =
                first: second: text:
                lib.hasInfix second (lib.last (lib.splitString first text));

              go = resolveWith [ acme ] "go" [
                {
                  key = "github.com/acme/lib@v1.2.0";
                  name = "github.com/acme/lib";
                  version = "v1.2.0";
                }
              ];

              # The registries of before fixup sets, as a module
              legacy =
                (resolveWith [
                  (import ./nix/lib/fixups/legacy.nix { inherit lib; } {
                    buildScriptFixups.serde = { patchVersion, vendorPath, ... }: "echo ${vendorPath} ${patchVersion}";
                    rustcFlags."rustix@1.0.7".linux = [
                      "--cfg"
                      "old"
                    ];
                    nativeLibraries.ring =
                      { patchVersion, ... }:
                      {
                        lib_name = "ring_${patchVersion}";
                        static_lib_path = "out_dir/libring.a";
                      };
                  })
                ] "rust" locked).fixups;

              # turnkey's own set, as another repository imports it, on
              # two platforms
              turnkeySet =
                platform: modules:
                (fixups.resolve {
                  evaluated = fixups.evalFixups { inherit pkgs modules; };
                  language = "rust";
                  deps = locked;
                  inherit platform;
                }).fixups;
              ownOnLinux = turnkeySet platform [ self.modules.turnkeyFixups.default ];
              ownOnIntelMac = turnkeySet {
                system = "x86_64-darwin";
                os = "macos";
                cpu = "x86_64";
              } [ self.modules.turnkeyFixups.default ];
              serdeOnly = turnkeySet platform [ self.modules.turnkeyFixups.serde ];

              expectations = {
                "sets and inline fixups merge per field" =
                  serde.gen.rustcFlags.common == [
                    "--cfg"
                    "from_other"
                    "--cfg"
                    "mine"
                  ]
                  && serde.gen.outDir;
                "patches apply in import order, before the build script" =
                  after "first.patch" "second.patch" serde.commands
                  && after "second.patch" "echo serde 219" serde.commands;
                "version entries apply to the versions their bounds hold for" =
                  ring17.gen.rustcFlags.common == [
                    "--cfg"
                    "ring_017"
                  ]
                  &&
                    ring16.gen.rustcFlags.common == [
                      "--cfg"
                      "ring_016"
                    ];
                "overlays stay per OS and per CPU" =
                  ring17.gen.rustcFlags.os.linux == [
                    "--cfg"
                    "linux_like"
                  ]
                  &&
                    ring17.gen.env.cpu.x86_64 == {
                      RING_X86 = "1";
                    };
                "the build script sees the platform the cell is built on" =
                  lib.hasInfix "ring for linux-x86_64" ring17.commands;
                "native library names are computed per version" =
                  (builtins.head ring17.gen.nativeLibraries).lib_name == "ring_core_0_17_14__";
                "a crate no set fixes has no fixup" = !(rust.fixups ? "rustix@1.0.7");
                "only the repository's own unused fixups warn" =
                  builtins.length rust.warnings == 1
                  && lib.hasInfix "typo-crate" (builtins.head rust.warnings)
                  && rust.errors == [ ];
                "two sets giving one build script conflict" = fails (
                  (resolveWith [
                    acme
                    conflicting
                  ] "rust" locked).fixups."serde@1.0.219".commands
                );
                "enable = false drops an imported fixup" =
                  !(
                    (resolveWith [
                      acme
                      { rust.ring.enable = false; }
                    ] "rust" locked).fixups ? "ring@0.17.14"
                  );
                "a build script in an overlay is a type error" = fails (
                  (resolveWith [ { rust.serde.os.linux.buildScript.skip = true; } ] "rust" locked).fixups
                );
                "a build script with both generate and skip is an error" =
                  lib.any (lib.hasInfix "both generate and skip")
                    (
                      (resolveWith [
                        {
                          rust.serde.buildScript = {
                            generate = "true";
                            skip = true;
                          };
                        }
                      ] "rust" locked).errors
                    );
                "every language's fixups apply patches" =
                  lib.hasInfix "first.patch"
                    go.fixups."github.com/acme/lib@v1.2.0".commands;
                "env isn't supported outside Rust yet" =
                  (resolveWith [ { go."github.com/acme/lib".env.X = "1"; } ] "go" [
                    {
                      key = "github.com/acme/lib@v1.2.0";
                      name = "github.com/acme/lib";
                      version = "v1.2.0";
                    }
                  ]).errors != [ ];
                "the old registries resolve as they used to" =
                  lib.hasInfix "echo . 219" legacy."serde@1.0.219".commands
                  &&
                    legacy."rustix@1.0.7".gen.rustcFlags.os.linux == [
                      "--cfg"
                      "old"
                    ]
                  && (builtins.head legacy."ring@0.17.14".gen.nativeLibraries).lib_name == "ring_14";
                "turnkey's published set fixes the crates it locks" =
                  lib.hasInfix "pub mod __private219" ownOnLinux."serde@1.0.219".commands
                  && ownOnLinux."rustix@1.0.7".gen.rustcFlags.os.macos != [ ]
                  && (builtins.head ownOnLinux."ring@0.17.14".gen.nativeLibraries).lib_name == "ring_core_0_17_14__";
                "ring's build script assembles the platform's own object format" =
                  lib.hasInfix "chacha-x86_64-elf.S" ownOnLinux."ring@0.17.14".commands
                  && lib.hasInfix "chacha-x86_64-macosx.S" ownOnIntelMac."ring@0.17.14".commands
                  && !(lib.hasInfix "-elf.S" ownOnIntelMac."ring@0.17.14".commands);
                "turnkey's ring fixup is for ring 0.17 only" =
                  !ownOnLinux."ring@0.16.20".accounted && ownOnLinux."ring@0.16.20".commands == "";
                "a family module brings its family only" =
                  serdeOnly ? "serde@1.0.219" && !(serdeOnly ? "ring@0.17.14");
              };
              failed = builtins.attrNames (lib.filterAttrs (_: ok: !ok) expectations);
            in
            assert lib.assertMsg (failed == [ ]) "fixup sets: ${lib.concatStringsSep "; " failed}";
            pkgs.runCommand "fixup-sets-check" { } "touch $out";

          # The soldeps cell's remappings.txt keeps the subdirectory of each
          # remapping soldeps-gen emits, moved from its lib/ or node_modules/
          # layout to the cell's vendor/ one: forge-std's sources live in
          # src/, so `forge-std/` must point there. Checked at evaluation.
          checks.solidity-remappings =
            let
              inherit ((import ./nix/lib/deps-cell { inherit pkgs lib; }).adapters) solidity;
              remapping =
                pkg:
                let
                  r = solidity.cellRemapping pkg;
                in
                "${r.prefix}=${r.target}";
              expected = {
                # A git dependency, Foundry layout
                "forge-std/=vendor/forge-std/src/" = {
                  name = "forge-std";
                  remapping = "forge-std/=lib/forge-std/src/";
                };
                # An npm dependency, scoped name
                "@openzeppelin/contracts/=vendor/@openzeppelin/contracts/" = {
                  name = "@openzeppelin/contracts";
                  remapping = "@openzeppelin/contracts/=node_modules/@openzeppelin/contracts/";
                };
                # No remapping: the package root, under its own name
                "solady/=vendor/solady/" = {
                  name = "solady";
                };
              };
              wrong = lib.filterAttrs (want: pkg: remapping pkg != want) expected;
              outside = solidity.cellRemapping {
                name = "forge-std";
                remapping = "forge-std/=elsewhere/forge-std/src/";
              };
            in
            assert lib.assertMsg (wrong == { })
              "solidity remappings: ${
                lib.concatStringsSep "; " (lib.mapAttrsToList (want: pkg: "${remapping pkg}, want ${want}") wrong)
              }";
            assert lib.assertMsg (
              !(builtins.tryEval outside.target).success
            ) "solidity remappings: a target outside lib/<name>/ or node_modules/<name>/ is accepted";
            pkgs.runCommand "solidity-remappings-check" { } "touch $out";

          # Configure turnkey to use our local toolchain files. tellerLib
          # and tellerRegistry default to self.lib.defaultTellerLib /
          # self.lib.defaultTellerRegistry system via the flake-parts
          # module, so we only declare the project-specific bits here.
          # Each declarationFile creates a corresponding shell.
          turnkey.toolchains = {
            enable = true;
            declarationFiles = {
              default = ./toolchain.toml; # Creates devShells.default with buck2 + nix + beads + go
              docs = ./docs/toolchain.toml; # Lightweight shell for building documentation
            };
            # Extend registry with turnkey-specific tools
            # (tk is already a built-in extension provided by the turnkey module)
            # (jsonnet is now provided by toolbox as an alias for jrsonnet)
            registryExtensions =
              let
                single = pkg: {
                  versions = {
                    "default" = pkg;
                  };
                  default = "default";
                };
              in
              {
                tw = single (import ./nix/packages/tw.nix { inherit pkgs lib; });
                turnkey-composed = single (import ./nix/packages/turnkey-composed.nix { inherit pkgs lib; });
              };
            # Enable Buck2 toolchain generation
            buck2 = {
              enable = true;
              # prelude.strategy defaults to "nix" - uses turnkey-prelude derivation
              welcomeMessage = "Welcome to turnkey dev shell";

              # The fixups the crates turnkey locks need: its own set
              fixups.imports = [ self.modules.turnkeyFixups.default ];

              # Go dependencies
              go = {
                enable = true;
                depsFile = ./go-deps.toml; # Regenerated by tk sync, tracked in git
              };

              # Rust dependencies
              rust = {
                enable = true;
                depsFile = ./rust-deps.toml; # Rust crate dependencies
                featuresFile = ./rust-features.toml; # Manual feature overrides
              };

              # Python dependencies
              python = {
                enable = true;
                depsFile = ./python-deps.toml; # Python package dependencies
                lockFile = "pylock.toml"; # PEP 751 lock exported from uv
                uvLockFile = "uv.lock"; # tk sync re-exports pylock.toml from it
              };

              # JavaScript/TypeScript dependencies
              javascript = {
                enable = true;
                depsFile = ./js-deps.toml; # npm package dependencies
              };

              # Solidity dependencies
              solidity = {
                enable = true;
                depsFile = ./solidity-deps.toml;
              };

              # Keep rules.star deps in step with the sources: tk syncs them
              # before build, test and the other build-graph commands
              rules.enabled = true;

              # Pre-commit checks
              tk = {
                jsTestConfigCheck = true;
                rustEditionCheck = true;
                monorepoDepCheck = true;
                foundryConfigCheck = true;
                sourceCoverageCheck = true;
                sourceScope = "src/"; # Only check source files under src/
              };
            };
          };
        };
    };
}
