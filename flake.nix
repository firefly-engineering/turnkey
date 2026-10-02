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
        #     their own from their own toolchain.toml. turnkey's own cells
        #     still reach the cache, through what the test-runner parity
        #     job builds (.github/workflows/test-runner-parity.yaml).
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
          "rust-rules-gen"
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
      # and toolbox helpers without re-importing turnkey's own flake, and
      # with turnkeyFlake = self so it can tell turnkey's own repository
      # from a consumer's.
      # Consumers don't need to do anything different — flakeModules.turnkey
      # is still the same value to import in their flake-parts setup.
      flake.flakeModules = {
        turnkey = import ./nix/flake-parts/turnkey {
          turnkeyLib = self.lib;
          devenvRoot = inputs.devenv-root;
          turnkeyFlake = self;
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
          turnkeyFlake = self;
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
          packages.rust-rules-gen = import ./nix/packages/rust-rules-gen.nix { inherit pkgs lib; };
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
              # The solc native forge gets (FOUNDRY_SOLC) is the cell's own
              solidityCell = toolchainsCell { declaredToolchains.solidity-toolchain = { }; };
              solcCell = toolchainsCell { declaredToolchains.solc = { }; };
              bothSolcCell = toolchainsCell {
                declaredToolchains = {
                  solc = { };
                  solidity-toolchain = { };
                };
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
                ignore = [
                  "e2e/fixtures/a"
                  "e2e/fixtures/b"
                ];
              };

              # The Solidity rules' inputs (prelude solidity.bzl's macros read
              # [solidity]), with and without a soldeps cell
              solidityBuckconfig =
                solidity:
                import ./nix/buck2/buckconfig.nix { inherit lib; } {
                  cells = [ ];
                  toolchainsCellPath = ".turnkey/toolchains";
                  testRunnerProtocol = "/nix/store/stand-in-protocol";
                  testCache = null;
                  inherit solidity;
                };
              withSoldeps = solidityBuckconfig {
                foundryToml = "root//:foundry.toml";
                remappingsTxt = "root//:remappings.txt";
                soldepsBundle = "soldeps//:bundle";
                soldepsDir = ".turnkey/soldeps";
              };
              withoutSoldeps = solidityBuckconfig {
                foundryToml = "root//:foundry.toml";
                remappingsTxt = null;
                soldepsBundle = null;
                soldepsDir = null;
              };

              # How the shell describes the cache to tk, as tk's tests read it
              shellContract = builtins.fromJSON (builtins.readFile ./src/testdata/shell-contract.json);
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
              solidityCell.solcPath == "/nix/store/stand-in-solidity-toolchain/bin/solc"
              && lib.hasInfix ''solc_path = "${solidityCell.solcPath}"'' solidityCell.buckFile
            ) "toolchains cell: solcPath is not the solc the solidity-toolchain cell's solc target runs";
            assert lib.assertMsg (
              solcCell.solcPath == "/nix/store/stand-in-solc/bin/solc"
            ) "toolchains cell: solcPath is not the declared solc's";
            assert lib.assertMsg (
              goCell.solcPath == null
            ) "toolchains cell: solcPath is set without a Solidity toolchain";
            assert lib.assertMsg (
              !(builtins.tryEval bothSolcCell.solcPath).success
            ) "toolchains cell: declaring both solc and solidity-toolchain picks a solc silently";
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
            assert lib.assertMsg
              (lib.hasInfix "[project]\n    ignore = .git,.jj,.hg,.sl,.devenv,.direnv,e2e/fixtures/a,e2e/fixtures/b\n" taggedBuckconfig)
              "buckconfig: buck2.ignore doesn't reach project.ignore";
            assert lib.assertMsg (
              lib.hasInfix "[solidity]\n    foundry_toml = root//:foundry.toml\n" withSoldeps
              && lib.hasInfix "    remappings_txt = root//:remappings.txt\n" withSoldeps
              && lib.hasInfix "    soldeps_bundle = soldeps//:bundle\n" withSoldeps
              && lib.hasInfix "    soldeps_dir = .turnkey/soldeps\n" withSoldeps
            ) "buckconfig: [solidity] doesn't carry the Solidity rules' inputs";
            assert lib.assertMsg (
              lib.hasInfix "foundry_toml = root//:foundry.toml" withoutSoldeps
              && !(lib.hasInfix "soldeps_dir" withoutSoldeps)
              && !(lib.hasInfix "[solidity]" uncached)
            ) "buckconfig: [solidity] names a soldeps cell there is none of, or appears without Solidity";
            assert lib.assertMsg (
              syncToml.conditions.go_tags == [ ]
            ) "sync.toml: [conditions] go_tags isn't buck2.go.allowedBuildTags";
            pkgs.runCommand "buck2-generators-check" { } "touch $out";

          # nix/buck2/platforms.nix's split agrees with the conditions
          # crate's (src/rust/conditions) on its test cases, and every
          # combined key it writes names a config_setting settingsBuckFile
          # defines. Cases with Go build tag dimensions are the crate's
          # alone. Checked at evaluation.
          checks.split-vectors =
            let
              platforms = import ./nix/buck2/platforms.nix { inherit lib; };
              vectors = builtins.fromJSON (builtins.readFile ./src/testdata/split-vectors.json);
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

          # A Rust crate's package, rules.star included, is built from its
          # own data alone (ADR 0004): changing one crate's slice or hash in
          # rust-deps.toml changes that crate's derivation and no other's.
          # Checked at evaluation, on two crates, prost-derive depending on
          # anyhow.
          checks.rust-crate-isolation =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              platforms = import ./nix/buck2/platforms.nix { inherit lib; };
              anyhow = extra: {
                "anyhow@1.0.100" = {
                  name = "anyhow";
                  version = "1.0.100";
                  hash = lib.fakeHash;
                  features = [ { name = "std"; } ];
                }
                // extra;
              };
              prostDerive = {
                "prost-derive@0.14.1" = {
                  name = "prost-derive";
                  version = "0.14.1";
                  hash = lib.fakeHash;
                  dependencies = [ { package = "anyhow@1.0.100"; } ];
                };
              };
              drvsOf =
                deps:
                lib.mapAttrs (_: drv: drv.drvPath) (
                  depsCell.adapters.rust.mkRustCrates {
                    inherit deps;
                    conditions = platforms.conditions [
                      "x86_64-linux"
                      "aarch64-darwin"
                    ];
                    rustRulesGen = config.packages.rust-rules-gen;
                  }
                );
              base = drvsOf (anyhow { } // prostDerive);
              # anyhow's slice and hash, each changed alone, and a crate
              # added: only the crate changed (if any) gets a new derivation
              itoa = {
                "itoa@1.0.15" = {
                  name = "itoa";
                  version = "1.0.15";
                  hash = lib.fakeHash;
                };
              };
              changes = {
                slice = {
                  deps = anyhow {
                    features = [
                      { name = "std"; }
                      {
                        name = "backtrace";
                        platforms = [ "linux-x86_64" ];
                      }
                    ];
                  };
                  changed = "anyhow@1.0.100";
                };
                hash = {
                  deps = anyhow { hash = builtins.replaceStrings [ "A" ] [ "B" ] lib.fakeHash; };
                  changed = "anyhow@1.0.100";
                };
                added = {
                  deps = anyhow { } // itoa;
                  changed = null;
                };
              };
              leaks = lib.filter (
                change:
                let
                  inherit (changes.${change}) deps changed;
                  drvs = drvsOf (deps // prostDerive);
                in
                lib.any (key: (drvs.${key} != base.${key}) != (key == changed)) (lib.attrNames base)
              ) (lib.attrNames changes);
            in
            assert lib.assertMsg (leaks == [ ])
              "rust crate isolation: ${lib.concatStringsSep ", " leaks} changed a derivation it shouldn't, or left the changed crate's alone";
            pkgs.runCommand "rust-crate-isolation-check" { } "touch $out";

          # A user patch goes to its own package's derivation
          # (nix/lib/deps-cell/adapters/rust.nix's userPatchesOf): routed by
          # its directory, vendor/<package>/, where an unversioned name
          # resolves as the cell's alias does. A flat patch file, from before
          # packages had directories, fails evaluation with where to move it.
          checks.rust-user-patches =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              routed =
                dir:
                depsCell.adapters.rust.userPatchesOf {
                  inherit dir;
                  cellName = "rustdeps";
                  keys = [
                    "anyhow@1.0.99"
                    "anyhow@1.0.100"
                    "prost-derive@0.14.1"
                  ];
                  parseKey = key: {
                    basePath = lib.head (lib.splitString "@" key);
                    version = lib.last (lib.splitString "@" key);
                  };
                };
              got = lib.mapAttrs (_: map baseNameOf) (routed ./nix/lib/deps-cell/testdata/patches);
              flat = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/flat-patches);
            in
            assert lib.assertMsg (
              got == {
                "anyhow@1.0.100" = [ "src-lib.rs.patch" ];
                "prost-derive@0.14.1" = [ "src-lib.rs.patch" ];
              }
            ) "rust user patches: routed ${builtins.toJSON got}";
            assert lib.assertMsg (!flat.success) "rust user patches: a flat patch file was accepted";
            pkgs.runCommand "rust-user-patches-check" { } "touch $out";

          # The godeps cell fetches each module from the proxy URL that
          # godeps-gen hashed, so nix/lib/deps-cell/fetchers.nix must
          # case-escape the path and the version exactly as
          # golang.org/x/mod/module does (vectors from
          # src/cmd/godeps-gen/src/prefetch.rs's tests).
          checks.go-proxy-url =
            let
              inherit (import ./nix/lib/deps-cell/fetchers.nix { inherit pkgs lib; }) goProxyZipUrl;
              got = map goProxyZipUrl [
                {
                  modulePath = "github.com/foo/bar";
                  version = "v1.0.0";
                }
                {
                  modulePath = "github.com/BurntSushi/toml";
                  version = "v1.4.0";
                }
                {
                  modulePath = "github.com/Azure/azure-sdk";
                  version = "v1.0.0-RC1";
                }
              ];
              want = [
                "https://proxy.golang.org/github.com/foo/bar/@v/v1.0.0.zip"
                "https://proxy.golang.org/github.com/!burnt!sushi/toml/@v/v1.4.0.zip"
                "https://proxy.golang.org/github.com/!azure/azure-sdk/@v/v1.0.0-!r!c1.zip"
              ];
            in
            assert lib.assertMsg (got == want) "go proxy url: built ${builtins.toJSON got}";
            pkgs.runCommand "go-proxy-url-check" { } "touch $out";

          # A Go user patch goes to its own module's derivation
          # (nix/lib/deps-cell/adapters/go.nix's userPatchesOf): routed by its
          # directory, vendor/<module path>/, nested modules included. A flat
          # patch file, from the cell before modules had directories, and a
          # directory that is a Go package inside a module, not a module,
          # both fail evaluation.
          checks.go-user-patches =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              routed =
                dir:
                depsCell.adapters.go.userPatchesOf {
                  inherit dir;
                  cellName = "godeps";
                  modulePaths = [
                    "golang.org/x/mod"
                    "cloud.google.com/go"
                    "cloud.google.com/go/storage"
                  ];
                };
              got = lib.mapAttrs (_: map baseNameOf) (routed ./nix/lib/deps-cell/testdata/patches);
              flat = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/flat-patches);
              unknown = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/unknown-patches);
            in
            assert lib.assertMsg (
              got == {
                "golang.org/x/mod" = [ "semver-semver.go.patch" ];
                "cloud.google.com/go/storage" = [ "doc.go.patch" ];
              }
            ) "go user patches: routed ${builtins.toJSON got}";
            assert lib.assertMsg (!flat.success) "go user patches: a flat patch file was accepted";
            assert lib.assertMsg (
              !unknown.success
            ) "go user patches: a package directory was taken for a module";
            pkgs.runCommand "go-user-patches-check" { } "touch $out";

          # A Solidity user patch goes to its own package's derivation
          # (nix/lib/deps-cell/adapters/solidity.nix's userPatchesOf), routed
          # by its directory, vendor/<name>/ (two segments for a scoped npm
          # package), and applies there: the check builds forge-std and
          # @openzeppelin/contracts, pinned as solidity-deps.toml pins them,
          # with their patches. A flat patch file, from the cell before
          # packages had directories, and a directory that is no package,
          # both fail evaluation.
          checks.solidity-user-patches =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              inherit (depsCell.adapters.solidity) userPatchesOf mkSolDepPackage;
              routed =
                dir:
                userPatchesOf {
                  inherit dir;
                  cellName = "soldeps";
                  names = [
                    "forge-std"
                    "@openzeppelin/contracts"
                  ];
                };
              patches = routed ./nix/lib/deps-cell/testdata/patches;
              got = lib.mapAttrs (_: map baseNameOf) patches;
              flat = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/flat-patches);
              unknown = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/unknown-patches);
              forgeStd = mkSolDepPackage {
                name = "forge-std";
                version = "1.8.0";
                source = "git";
                url = "https://github.com/foundry-rs/forge-std/archive/b6a506db2262cad5ff982a87789ee6d1558ec861.tar.gz";
                hash = "sha256-C3TD7/jCXNZIYdXXMunVZZF1BaUIjbCOuPuD50mvh4s=";
                userPatches = patches."forge-std";
              };
              openzeppelin = mkSolDepPackage {
                name = "@openzeppelin/contracts";
                version = "5.4.0";
                source = "npm";
                url = "https://registry.npmjs.org/@openzeppelin%2fcontracts/-/contracts-5.4.0.tgz";
                integrity = "sha512-eCYgWnLg6WO+X52I16TZt8uEjbtdkgLC0SUX/xnAksjjrQI4Xfn4iBRoI5j55dmlOhDv1Y7BoR3cU7e3WWhC6A==";
                userPatches = patches."@openzeppelin/contracts";
              };
            in
            assert lib.assertMsg (
              got == {
                "forge-std" = [ "src-Test.sol.patch" ];
                "@openzeppelin/contracts" = [ "token-ERC20-ERC20.sol.patch" ];
              }
            ) "solidity user patches: routed ${builtins.toJSON got}";
            assert lib.assertMsg (!flat.success) "solidity user patches: a flat patch file was accepted";
            assert lib.assertMsg (
              !unknown.success
            ) "solidity user patches: a directory that is no package was taken for one";
            pkgs.runCommand "solidity-user-patches-check" { } ''
              grep -qx '// Patched.' ${forgeStd}/src/Test.sol
              grep -qx '// Patched.' ${openzeppelin}/token/ERC20/ERC20.sol
              touch $out
            '';

          # A JavaScript user patch goes to its own package's derivation
          # (nix/lib/deps-cell/adapters/javascript.nix's userPatchesOf),
          # routed by its directory, vendor/<name>@<version>/, or
          # vendor/<name>/ for the version a direct dependency resolves to
          # (two segments for a scoped package), and applies there: the check
          # builds lodash and @types/lodash, pinned as js-deps.toml pins
          # them, with their patches. A flat patch file, from the cell before
          # packages had directories, and a directory that is no package,
          # both fail evaluation.
          checks.js-user-patches =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              inherit (depsCell.adapters.javascript) userPatchesOf mkJsDepPackage;
              routed =
                dir:
                userPatchesOf {
                  inherit dir;
                  cellName = "jsdeps";
                  keys = [
                    "lodash@4.17.21"
                    "@types/lodash@4.17.23"
                  ];
                  direct = {
                    "@types/lodash" = "@types/lodash@4.17.23";
                  };
                };
              patches = routed ./nix/lib/deps-cell/testdata/patches;
              got = lib.mapAttrs (_: map baseNameOf) patches;
              flat = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/flat-patches);
              unknown = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/unknown-patches);
              lodash = mkJsDepPackage {
                name = "lodash";
                version = "4.17.21";
                url = "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz";
                integrity = "sha512-v2kDEe57lecTulaDIuNTPy3Ry4gLGJ6Z1O3vE1krgXZNrsQ+LFTGHVxVjcXPs17LhbZVGedAJv8XZ1tvj5FvSg==";
                userPatches = patches."lodash@4.17.21";
              };
              typesLodash = mkJsDepPackage {
                name = "@types/lodash";
                version = "4.17.23";
                url = "https://registry.npmjs.org/@types%2flodash/-/lodash-4.17.23.tgz";
                integrity = "sha512-RDvF6wTulMPjrNdCoYRC8gNR880JNGT8uB+REUpC2Ns4pRqQJhGz90wh7rgdXDPpCczF3VGktDuFGVnz8zP7HA==";
                userPatches = patches."@types/lodash@4.17.23";
              };
            in
            assert lib.assertMsg (
              got == {
                "lodash@4.17.21" = [ "README.md.patch" ];
                "@types/lodash@4.17.23" = [ "common-array.d.ts.patch" ];
              }
            ) "js user patches: routed ${builtins.toJSON got}";
            assert lib.assertMsg (!flat.success) "js user patches: a flat patch file was accepted";
            assert lib.assertMsg (
              !unknown.success
            ) "js user patches: a directory that is no package was taken for one";
            pkgs.runCommand "js-user-patches-check" { } ''
              grep -qx 'Patched.' ${lodash}/README.md
              grep -qx '// Patched.' ${typesLodash}/common/array.d.ts
              touch $out
            '';

          # The jsdeps cell's root package (ADR 0012) names each instance as
          # pnpm names its directory, hashed past the path-component limit,
          # and collapses each dependency cycle into one component: checked
          # at evaluation on nix/lib/deps-cell/adapters/javascript.nix's
          # instanceName and stronglyConnected.
          checks.js-instance-graph =
            let
              inherit ((import ./nix/lib/deps-cell { inherit pkgs lib; }).adapters.javascript)
                instanceName
                stronglyConnected
                ;
              long = "@scope/plugin@1.0.0(${
                lib.concatStringsSep ")(" (map (n: "@scope/peer-${toString n}@1.0.0") (lib.range 1 12))
              })";
              names = map instanceName [
                "lodash@4.17.21"
                "@types/lodash@4.17.23"
                "react-dom@18.2.0(react@18.2.0)"
                "@testing-library/react@14.0.0(@types/react@18.2.0(react@18.2.0))(react@18.2.0)"
                "react@18.2.0(patch_hash=abc123)"
              ];
              wantNames = [
                "lodash@4.17.21"
                "@types+lodash@4.17.23"
                "react-dom@18.2.0_react@18.2.0"
                "@testing-library+react@14.0.0_@types+react@18.2.0_react@18.2.0__react@18.2.0"
                "react@18.2.0_patch_hash=abc123"
              ];
              hashed = instanceName long;
              edges = {
                a = [ "b" ];
                b = [ "c" ];
                c = [
                  "a"
                  "d"
                ];
                d = [ ];
                e = [ "e" ];
                f = [
                  "a"
                  "g"
                ];
                g = [ "f" ];
              };
              components = lib.sort (x: y: lib.head x < lib.head y) (
                stronglyConnected (lib.attrNames edges) (n: edges.${n})
              );
            in
            assert lib.assertMsg (names == wantNames) "js instance names: ${builtins.toJSON names}";
            assert lib.assertMsg (
              builtins.stringLength hashed == 240 && instanceName (long + "x") != hashed
            ) "js instance names: a long name hashes to ${hashed}";
            assert lib.assertMsg (
              components == [
                [
                  "a"
                  "b"
                  "c"
                ]
                [ "d" ]
                [ "e" ]
                [
                  "f"
                  "g"
                ]
              ]
            ) "js instance graph: components ${builtins.toJSON components}";
            pkgs.runCommand "js-instance-graph-check" { } "touch $out";

          # A Python user patch goes to its own distribution's derivation
          # (nix/lib/deps-cell/adapters/python.nix's userPatchesOf), routed by
          # its directory, vendor/<name>/, and applies there: six built with
          # the fixture patch has the line it adds, and its rules.star and
          # target, and is its locked wheel's installed layout (ADR 0013).
          # A flat patch file, from the cell that was one store path,
          # and a directory naming no locked distribution both fail
          # evaluation.
          checks.python-user-patches =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              routed =
                dir:
                depsCell.adapters.python.userPatchesOf {
                  inherit dir;
                  cellName = "pydeps";
                  names = [
                    "six"
                    "requests"
                  ];
                };
              patches = routed ./nix/lib/deps-cell/testdata/patches;
              got = lib.mapAttrs (_: map baseNameOf) patches;
              flat = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/flat-patches);
              unknown = builtins.tryEval (routed ./nix/lib/deps-cell/testdata/unknown-patches);
              six = depsCell.mkPythonDepPackage {
                name = "six";
                version = "1.17.0";
                sha256 = "sha256-D48q9oGU8W9av7f06wJAtI3tpSTY9bJYfhwzLgZEakY=";
                url = "https://files.pythonhosted.org/packages/b7/ce/149a00dd41f10bc29e5921b496af8b574d8413afcd5e30dfa0ed46c2cc5e/six-1.17.0-py2.py3-none-any.whl";
                slice = { };
                conditions = (import ./nix/buck2/platforms.nix { inherit lib; }).conditions [ system ];
                pydepsCell = import ./nix/packages/pydeps-cell.nix { inherit pkgs lib; };
                userPatches = patches.six;
              };
            in
            assert lib.assertMsg (
              got == {
                "six" = [ "six.py.patch" ];
              }
            ) "python user patches: routed ${builtins.toJSON got}";
            assert lib.assertMsg (!flat.success) "python user patches: a flat patch file was accepted";
            assert lib.assertMsg (
              !unknown.success
            ) "python user patches: a directory naming no locked distribution was accepted";
            pkgs.runCommand "python-user-patches-check" { } ''
              grep -qx '__patched__ = "python-user-patches"' ${six}/six.py
              grep -q 'name = "six"' ${six}/rules.star
              [ "$(cat ${six.targets})" = six ]

              # It is the locked wheel, the installed layout (ADR 0013): its
              # dist-info, a resource of its library, and no sdist build file
              [ -f ${six}/six-1.17.0.dist-info/METADATA ]
              [ ! -e ${six}/setup.py ] && [ ! -e ${six}/test_six.py ]
              grep -qF 'resources = glob(["**"], exclude = ["**/*.py", "rules.star"])' ${six}/rules.star
              touch $out
            '';

          # A pydeps wheel is installed as an installer would (ADR 0013,
          # python.nix's installWheel): its <name>.data/purelib and platlib
          # are merged into the root and the rest of <name>.data dropped,
          # while a package directory merely ending in .data stays. A
          # python-deps.toml from before schema 3, whose sources are sdists,
          # fails evaluation.
          checks.python-wheels =
            let
              depsCell = import ./nix/lib/deps-cell { inherit pkgs lib; };
              # An unpacked wheel with every <name>.data scheme
              wheel = pkgs.runCommand "fake-wheel" { } ''
                mkdir -p $out/pkg $out/pkg-1.0.dist-info $out/pkg-1.0.data/{purelib/pure,platlib/plat,scripts,data} $out/keep.data
                touch $out/keep.data/kept.txt
                touch $out/pkg/__init__.py $out/pkg-1.0.dist-info/METADATA
                touch $out/pkg-1.0.data/purelib/pure/__init__.py $out/pkg-1.0.data/platlib/plat/__init__.py
                touch $out/pkg-1.0.data/scripts/tool $out/pkg-1.0.data/data/share.txt
              '';
              installed = pkgs.runCommand "installed-wheel" {
                src = wheel;
              } depsCell.adapters.python.installWheel;
              v2 =
                builtins.tryEval
                  (depsCell.mkPythonDepsCell {
                    cellName = "pydeps";
                    depsFile = builtins.toFile "python-deps.toml" ''
                      schema_version = 2

                      [deps.six]
                      version = "1.17.0"
                      hash = "sha256-S8IT/6DLDC/sE233C6V/PW4rIMlUM/qfkvsiW/tO2N4="
                      url = "https://files.pythonhosted.org/packages/94/e7/b2c673351809dca68a0e064b6af791aa332cf192da575fd474ed7d6f16a2/six-1.17.0.tar.gz"
                    '';
                    conditions = (import ./nix/buck2/platforms.nix { inherit lib; }).conditions [ system ];
                    pydepsCell = import ./nix/packages/pydeps-cell.nix { inherit pkgs lib; };
                  }).depPackages;
            in
            assert lib.assertMsg (
              !v2.success
            ) "python wheels: a schema_version 2 python-deps.toml was accepted";
            pkgs.runCommand "python-wheels-check" { } ''
              cd ${installed}
              [ -f pkg/__init__.py ] && [ -f pkg-1.0.dist-info/METADATA ]
              [ -f pure/__init__.py ] && [ -f plat/__init__.py ]
              [ ! -e pkg-1.0.data ] && [ ! -e tool ] && [ ! -e share.txt ]
              [ -f keep.data/kept.txt ]
              touch $out
            '';

          # A Nix-built Rust tool is built from its workspace projection
          # (nix/lib/cargo.nix), so a workspace change its members don't
          # reach leaves its source and lock alone: an unreachable package's
          # checksum changed, a package added to the lock, a
          # [workspace.dependencies] entry and a member added. A reachable
          # package's checksum does change them, and projecting onto every
          # member gives back Cargo.lock as it is. Checked at evaluation, on
          # rust-rules-gen.
          checks.workspace-projection =
            let
              cargoLib = import ./nix/lib/cargo.nix { inherit pkgs lib; };
              root = ./.;
              manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
              lock = builtins.fromTOML (builtins.readFile ./Cargo.lock);
              project =
                args:
                let
                  projection = cargoLib.workspaceProjection (
                    {
                      inherit root;
                      members = [ "src/cmd/rust-rules-gen" ];
                    }
                    // args
                  );
                in
                {
                  inherit (projection) lock;
                  src = projection.src.drvPath;
                };
              base = project { };
              reachedKeys = map (p: "${p.name} ${p.version}") (builtins.fromTOML base.lock).package;
              isReached = p: lib.elem "${p.name} ${p.version}" reachedKeys;
              # The first registry package on each side of the projection
              unreached = lib.findFirst (p: p ? checksum && !(isReached p)) null lock.package;
              reached = lib.findFirst (p: p ? checksum && isReached p) null lock.package;
              withChecksum =
                target:
                lock
                // {
                  package = map (
                    p:
                    if p == target then p // { checksum = builtins.replaceStrings [ "0" ] [ "1" ] p.checksum; } else p
                  ) lock.package;
                };
              unaffected = {
                unreached-checksum = project { lock = withChecksum unreached; };
                lock-package-added = project {
                  lock = lock // {
                    package = lock.package ++ [
                      {
                        name = "turnkey-projection-check";
                        version = "0.0.0";
                        source = "registry+https://github.com/rust-lang/crates.io-index";
                        checksum = lib.fakeSha256;
                      }
                    ];
                  };
                };
                workspace-dependency-added = project {
                  manifest = lib.recursiveUpdate manifest {
                    workspace.dependencies.turnkey-projection-check = "0.0";
                  };
                };
                member-added = project {
                  manifest = lib.recursiveUpdate manifest {
                    workspace.members = manifest.workspace.members ++ [ "src/examples/rust-hello-projection" ];
                  };
                };
              };
              leaks = lib.attrNames (lib.filterAttrs (_: p: p != base) unaffected);
              affected = project { lock = withChecksum reached; };
              everyMember = cargoLib.workspaceProjection {
                inherit root;
                inherit (manifest.workspace) members;
              };
            in
            assert lib.assertMsg (
              unreached != null && reached != null
            ) "workspace projection: no registry package on one side of rust-rules-gen's projection";
            assert lib.assertMsg (
              leaks == [ ]
            ) "workspace projection: ${lib.concatStringsSep ", " leaks} changed rust-rules-gen's projection";
            assert lib.assertMsg (
              affected.lock != base.lock && affected.src != base.src
            ) "workspace projection: a reachable package's checksum left rust-rules-gen's projection alone";
            assert lib.assertMsg (
              builtins.fromTOML everyMember.lock == lock
            ) "workspace projection: projecting onto every member doesn't give back Cargo.lock";
            pkgs.runCommand "workspace-projection-check" { } "touch $out";

          # use_turnkey re-evaluates the flake when a file the shell was built
          # from no longer matches, and only then
          # (nix/devenv/turnkey/deps-freshness.nix): a deps file, or in
          # turnkey's own repository one of turnkey's Nix sources. The default
          # shell records its deps files and turnkey's sources by content; a
          # deps file the flake can't see is left out, and a consumer project
          # records no turnkey source. A changed or missing file loads the
          # .envrc once more, with nix-direnv's cached shell backdated; an
          # unchanged one doesn't.
          checks.deps-freshness =
            let
              depsFreshness = import ./nix/devenv/turnkey/deps-freshness.nix { inherit lib pkgs; };
              languages = import ./nix/buck2/languages.nix { inherit pkgs lib; };
              shellEntries = depsFreshness.entries {
                inherit languages;
                buck2 = config.devenv.shells.default.turnkey.buck2;
              };
              stringEntries = depsFreshness.entries {
                inherit languages;
                buck2 =
                  (lib.evalModules {
                    modules = [
                      (import ./nix/buck2/options.nix {
                        inherit lib;
                        version = "check";
                      })
                      {
                        enable = true;
                        rust.depsFile = "rust-deps.toml";
                      }
                    ];
                  }).config;
              };
              # turnkey's sources, in turnkey's repository and in a consumer's
              # (any other flake)
              ownSources = depsFreshness.sourceEntries {
                project = self;
                turnkey = self;
              };
              consumerSources = depsFreshness.sourceEntries {
                project = {
                  outPath = "${./templates/default}";
                };
                turnkey = self;
              };
              # A shell built from a rust-deps.toml holding "old" and a
              # nix/module.nix holding "old module"
              refresh = pkgs.writeText "deps-freshness.sh" (
                depsFreshness.refresh [
                  {
                    file = "rust-deps.toml";
                    hash = builtins.hashString "sha256" "old\n";
                  }
                  {
                    file = "nix/module.nix";
                    hash = builtins.hashString "sha256" "old module\n";
                  }
                ]
              );
            in
            assert lib.assertMsg (lib.elem {
              file = "rust-deps.toml";
              hash = builtins.hashFile "sha256" ./rust-deps.toml;
            } shellEntries) "deps freshness: the default shell doesn't record rust-deps.toml's content";
            assert lib.assertMsg (
              stringEntries == [ ]
            ) "deps freshness: a deps file outside the flake is recorded";
            assert lib.assertMsg (lib.elem {
              file = "nix/devenv/turnkey/deps-freshness.nix";
              hash = builtins.hashFile "sha256" ./nix/devenv/turnkey/deps-freshness.nix;
            } ownSources) "deps freshness: turnkey's repository doesn't record its Nix sources";
            assert lib.assertMsg (lib.any (
              entry: lib.hasSuffix ".patch" entry.file
            ) ownSources) "deps freshness: turnkey's repository doesn't record its patches";
            assert lib.assertMsg (lib.all (
              entry:
              lib.hasPrefix "nix/" entry.file
              && (lib.hasSuffix ".nix" entry.file || lib.hasSuffix ".patch" entry.file)
            ) ownSources) "deps freshness: a recorded source isn't a .nix or .patch file under nix/";
            assert lib.assertMsg (
              consumerSources == [ ]
            ) "deps freshness: a consumer project records turnkey's sources";
            assert lib.assertMsg (
              config.devenv.shells.default.turnkey.turnkeySources == ownSources
            ) "deps freshness: the default shell doesn't record turnkey's sources";
            pkgs.runCommand "deps-freshness-check" { } ''
              mkdir layout project
              layout=$PWD/layout
              cd project
              # An .envrc like a consumer's: the library, then use_turnkey's
              # early return, then the links
              cat > .envrc <<'EOF'
              echo load >> loads
              source ${refresh}
              if _turnkey_refresh_shell; then return 0; fi
              echo link >> loads
              EOF
              # direnv's functions, as far as the routine uses them
              load() {
                rm -f loads log
                touch -t 202601010000 "$layout/flake-profile-x.rc"
                bash -c '
                  layout=$1
                  direnv_layout_dir() { echo "$layout"; }
                  log_status() { echo "$*" >> log; }
                  source_env() { . "$1"; }
                  source_env ./.envrc
                ' _ "$layout"
              }
              touch -t 202001010000 reference
              fail() { echo "deps freshness: $*" >&2; exit 1; }
              # The files as the shell was built from them
              unchanged() {
                mkdir -p nix
                printf 'old\n' > rust-deps.toml
                printf 'old module\n' > nix/module.nix
              }

              unchanged
              load
              [ "$(cat loads)" = "$(printf 'load\nlink')" ] || fail "unchanged files: loads were $(cat loads)"
              [ ! -e log ] || fail "unchanged files: logged $(cat log)"
              [ "$layout/flake-profile-x.rc" -nt reference ] || fail "unchanged files: cache backdated"

              # Each change, to a deps file or a turnkey source, loads the
              # .envrc exactly once more, even though the reloaded shell
              # (this same library) still records the old content
              for case in \
                "rust-deps.toml:printf 'new\n' > rust-deps.toml" \
                "rust-deps.toml:rm rust-deps.toml" \
                "nix/module.nix:printf 'new module\n' > nix/module.nix" \
                "nix/module.nix:rm nix/module.nix"; do
                file=''${case%%:*} change=''${case#*:}
                unchanged
                eval "$change"
                load
                [ "$(cat loads)" = "$(printf 'load\nload\nlink')" ] || fail "$change: loads were $(cat loads)"
                grep -qxF "turnkey: re-evaluating the flake for $file" log || fail "$change: no re-evaluation logged"
                grep -qxF "turnkey: the dev shell still doesn't match $file" log || fail "$change: second load didn't report"
                [ reference -nt "$layout/flake-profile-x.rc" ] || fail "$change: cache not backdated"
              done
              touch $out
            '';

          # .turnkey/sync.toml as the shell writes it for a project with every
          # language, byte for byte what rules sync's tests read
          # (src/rust/rules-syncer/testdata/sync.toml): the seam between the
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
              fixture = ./src/rust/rules-syncer/testdata/sync.toml;
            in
            pkgs.runCommand "sync-config-contract-check" { } ''
              if ! diff -u ${fixture} ${syncConfig.file}; then
                echo "sync.toml changed: copy ${syncConfig.file} to src/rust/rules-syncer/testdata/sync.toml" >&2
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
              platforms = import ./nix/buck2/platforms.nix { inherit lib; };
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
                  rulesOf =
                    language:
                    language.syncRules {
                      langCfg = options.${language.name};
                      conditions = platforms.conditions options.platforms;
                    };
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
                goRecord.syncRules {
                  langCfg = (buck2Options { }).go // {
                    depsFile = ./.turnkey/go-deps.toml;
                  };
                  conditions = platforms.conditions (buck2Options { }).platforms;
                }
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
          # entries and the OS, CPU and OS and CPU pair overlays resolve
          # per locked dependency.
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

              # The OS and CPU pair overlay (platform."<os>-<cpu>"), with
              # the OS and CPU overlays and a version entry around it
              serdeLocked = builtins.filter (d: d.name == "serde") locked;
              paired = resolveWith [
                {
                  rust.serde = {
                    os.linux = {
                      rustcFlags = [
                        "--cfg"
                        "o"
                      ];
                      patches = [ (testdata + "/first.patch") ];
                    };
                    cpu.x86_64 = {
                      rustcFlags = [
                        "--cfg"
                        "c"
                      ];
                      patches = [ (testdata + "/third.patch") ];
                    };
                    platform."linux-x86_64" = {
                      rustcFlags = [
                        "--cfg"
                        "p"
                      ];
                      patches = [ (testdata + "/second.patch") ];
                      nativeLibraries = [
                        {
                          name = "on_host";
                          staticLib = "out_dir/libon_host.a";
                        }
                      ];
                    };
                    platform."macos-arm64" = {
                      patches = [ (testdata + "/elsewhere.patch") ];
                      nativeLibraries = [
                        {
                          name = "elsewhere";
                          staticLib = "out_dir/libelsewhere.a";
                        }
                      ];
                    };
                    versions = [
                      {
                        when.atLeast = "1.0";
                        platform."linux-x86_64".rustcFlags = [
                          "--cfg"
                          "v"
                        ];
                      }
                    ];
                  };
                }
                # Another set adding to the same pair
                {
                  rust.serde.platform."linux-x86_64".rustcFlags = [
                    "--cfg"
                    "s"
                  ];
                }
              ] "rust" serdeLocked;
              pairedSerde = paired.fixups."serde@1.0.219";
              pairFlags = pairedSerde.gen.rustcFlags.platform."linux-x86_64";
              envErrors =
                overlays:
                (resolveWith [
                  { rust.serde = overlays; }
                ] "rust" serdeLocked).errors;

              go = resolveWith [ acme ] "go" [
                {
                  key = "github.com/acme/lib@v1.2.0";
                  name = "github.com/acme/lib";
                  version = "v1.2.0";
                }
              ];

              # The options fixup sets replaced, set as they used to be
              retired =
                option:
                (lib.evalModules {
                  modules = [
                    (import ./nix/buck2/options.nix {
                      inherit lib;
                      version = "check";
                    })
                    { rust.${option}.serde = [ ]; }
                  ];
                }).config.rust.${option};

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

              # An unaccounted build script's error, pointing at turnkey's
              # published family when one accounts for it
              catalog = lib.mapAttrs (name: module: {
                inherit module;
                import = "inputs.turnkey.modules.turnkeyFixups.${name}";
              }) (builtins.removeAttrs (import ./nix/fixups) [ "default" ]);
              unaccounted =
                (fixups.resolve {
                  evaluated = fixups.evalFixups {
                    inherit pkgs;
                    modules = [ self.modules.turnkeyFixups.ring ];
                  };
                  language = "rust";
                  deps = locked ++ [
                    {
                      key = "mystery@1.0.0";
                      name = "mystery";
                      version = "1.0.0";
                    }
                  ];
                  inherit platform catalog;
                }).unaccounted;

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
                "an OS and CPU pair's overlay is kept per pair, after sets and version entries add to it" =
                  lib.sort (a: b: a < b) (builtins.filter (f: f != "--cfg") pairFlags) == [
                    "p"
                    "s"
                    "v"
                  ]
                  && lib.last pairFlags == "v"
                  &&
                    pairedSerde.gen.rustcFlags.os.linux == [
                      "--cfg"
                      "o"
                    ]
                  && paired.errors == [ ];
                "the host's overlays patch in OS, CPU, pair order, and other pairs' don't apply" =
                  after "first.patch" "third.patch" pairedSerde.commands
                  && after "third.patch" "second.patch" pairedSerde.commands
                  && !(lib.hasInfix "elsewhere.patch" pairedSerde.commands);
                "the host's pair overlay links its native libraries, and other pairs' don't" =
                  map (l: l.lib_name) pairedSerde.gen.nativeLibraries == [ "on_host" ];
                "a pair outside the supported platforms is a type error" = fails (
                  (resolveWith [
                    { rust.serde.platform."windows-x86_64".rustcFlags = [ ]; }
                  ] "rust" serdeLocked).fixups
                );
                "a build script in a pair overlay is a type error" = fails (
                  (resolveWith [
                    { rust.serde.platform."linux-x86_64".buildScript.skip = true; }
                  ] "rust" serdeLocked).fixups
                );
                "overlays giving one platform different env values are an error" =
                  lib.any
                    (
                      e:
                      lib.hasInfix "serde" e
                      && lib.hasInfix "env.FOO" e
                      && lib.hasInfix "os.linux" e
                      && lib.hasInfix "platform.linux-arm64" e
                    )
                    (envErrors {
                      os.linux.env.FOO = "a";
                      platform."linux-arm64".env.FOO = "b";
                    })
                  && lib.any (e: lib.hasInfix "os.linux" e && lib.hasInfix "cpu.arm64" e) (envErrors {
                    os.linux.env.FOO = "a";
                    cpu.arm64.env.FOO = "b";
                  });
                "overlays giving one platform the same env value agree" =
                  envErrors {
                    os.linux.env.FOO = "a";
                    cpu.x86_64.env.FOO = "a";
                    platform."linux-arm64".env.FOO = "a";
                    # A different value, but on no platform os.linux is
                    platform."macos-arm64".env.FOO = "b";
                  } == [ ];
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
                "the options fixup sets replaced are errors" =
                  fails (retired "buildScriptFixups") && fails (retired "rustcFlagsRegistry");
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
                "an unaccounted build script names the published family that accounts for it" =
                  lib.hasSuffix
                    "a published fixup set does: add `inputs.turnkey.modules.turnkeyFixups.serde` to turnkey.toolchains.buck2.fixups.imports"
                    unaccounted."serde@1.0.219";
                "an unaccounted build script no family accounts for asks for a fixup" =
                  lib.hasInfix "`rust.\"mystery\".buildScript.skip = true`" unaccounted."mystery@1.0.0"
                  && lib.hasInfix "`rust.\"ring\".buildScript.generate`" unaccounted."ring@0.16.20";
                "an accounted build script has no error" = !(unaccounted ? "ring@0.17.14");
                "a family module brings its family only" =
                  serdeOnly ? "serde@1.0.219" && !(serdeOnly ? "ring@0.17.14");
              };
              failed = builtins.attrNames (lib.filterAttrs (_: ok: !ok) expectations);
            in
            assert lib.assertMsg (failed == [ ]) "fixup sets: ${lib.concatStringsSep "; " failed}";
            pkgs.runCommand "fixup-sets-check" { } "touch $out";

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
              ci = ./.github/toolchain.toml; # What buck2 test //... needs, for the CI parity gate
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
              # The CI shell runs buck2 test //... too
              shells = [
                "default"
                "ci"
              ];
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
                # @types packages are devDependencies: [direct] needs them
                # for the examples' jsdeps//:@types/... labels
                includeDevDependencies = true;
              };

              # Solidity dependencies
              solidity = {
                enable = true;
                depsFile = ./solidity-deps.toml;
              };

              # Keep rules.star deps in step with the sources: tk syncs them
              # before build, test and the other build-graph commands
              rules.enabled = true;

              # The e2e fixtures are test data: each is a project of its own,
              # which the e2e tests copy into a fresh turnkey project. Their
              # deps resolve only there, against their own deps cells
              ignore = [ "e2e/fixtures" ];

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
