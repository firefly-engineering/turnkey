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
          packages.compute-unified-features = import ./nix/packages/compute-unified-features.nix { inherit pkgs lib; };
          packages.gen-rust-buck = import ./nix/packages/gen-rust-buck.nix { inherit pkgs lib; };
          packages.pytest-uv-shim = import ./nix/packages/pytest-uv-shim.nix { inherit pkgs lib; };
          packages.check-rust-edition-rs = import ./nix/packages/check-rust-edition-rs.nix { inherit pkgs lib; };
          packages.check-source-coverage-rs = import ./nix/packages/check-source-coverage-rs.nix { inherit pkgs lib; };
          packages.tw-go = (import ./nix/packages/tw-wrappers.nix {
            inherit pkgs lib;
            tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
          }).tw-go;
          packages.tw-cargo = (import ./nix/packages/tw-wrappers.nix {
            inherit pkgs lib;
            tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
          }).tw-cargo;
          packages.tw-uv = (import ./nix/packages/tw-wrappers.nix {
            inherit pkgs lib;
            tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
          }).tw-uv;

          # turnkey-test-runner, built for the pinned buck2 release
          packages.turnkey-test-runner = import ./nix/packages/turnkey-test-runner.nix {
            inherit pkgs lib;
          };

          # Expose turnkey-prelude for CI builds: the pinned buck2 release's
          # prelude, as the dev shell uses it
          packages.turnkey-prelude = (self.lib.pinnedBuck2Release system).prelude;

          # The pinned buck2 release holds together: the binary and prelude
          # are the release's, and the shell gets exactly what the flake
          # publishes. Checked at evaluation, so `nix flake check --no-build`
          # runs it.
          checks.pinned-buck2-release =
            let
              release = self.lib.pinnedBuck2Release system;
              shellBuck2 = config.devenv.shells.default.turnkey.buck2;
            in
            assert lib.assertMsg (release.buck2.version == release.version)
              "pinned buck2 release: binary is ${release.buck2.version}, not ${release.version}";
            assert lib.assertMsg (release.upstreamPrelude.version == release.version)
              "pinned buck2 release: upstream prelude is ${release.upstreamPrelude.version}, not ${release.version}";
            assert lib.assertMsg (shellBuck2.package.drvPath == release.buck2.drvPath)
              "pinned buck2 release: the default shell's buck2 is not the pinned binary";
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
                declarationFile = builtins.toFile "toolchain.toml" ''
                  [toolchains]
                  buck2 = {}
                '';
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
            assert lib.assertMsg (!(builtins.tryEval declaringBuck2).success)
              "toolchain declaration: declaring buck2 in toolchain.toml resolves instead of failing";
            assert lib.assertMsg (builtins.any (pkg: pkg.drvPath == release.buck2.drvPath) config.packages.toolchain-profile.toolchainPackages)
              "toolchain declaration: the toolchain profile lacks the pinned buck2";
            assert lib.assertMsg (strayBuck2 == [ ])
              "toolchain declaration: ${lib.concatMapStringsSep ", " toString strayBuck2} use a buck2 other than the pinned one";
            pkgs.runCommand "toolchain-declaration-check" { } "touch $out";

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

              # Go dependencies
              go = {
                enable = true;
                depsFile = ./go-deps.toml; # Auto-generated by .envrc, tracked in git
                generateOnShellEntry = false; # .envrc handles generation
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
                lockFile = "pylock.toml";      # PEP 751 lock exported from uv
                uvLockFile = "uv.lock";        # tk sync re-exports pylock.toml from it
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

              # Pre-commit checks
              tk = {
                jsTestConfigCheck = true;
                rustEditionCheck = true;
                monorepoDepCheck = true;
                foundryConfigCheck = true;
                sourceCoverageCheck = true;
                sourceScope = "src/";  # Only check source files under src/
              };
            };
          };
        };
    };
}
