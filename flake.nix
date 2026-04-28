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
  };

  outputs =
    inputs@{
      self,
      flake-parts,
      nixpkgs,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      # Export the turnkey flake-parts module
      flake.flakeModules = {
        turnkey = ./nix/flake-parts/turnkey;
      };

      # Export the turnkey devenv module separately
      flake.devenvModules = {
        turnkey = ./nix/devenv/turnkey;
      };

      # Flake templates for project initialization
      flake.templates = {
        default = {
          path = ./templates/default;
          description = "Buck2 project with turnkey toolchain management";
        };
      };

      # Use the module ourselves as a working example
      imports = [
        inputs.devenv.flakeModule
        ./nix/flake-parts/turnkey
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
          packages.tk = import ./nix/packages/tk.nix { inherit pkgs lib; };
          packages.tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
          packages.e2e-runner = import ./nix/packages/e2e-runner.nix { inherit pkgs lib; };
          packages.jsdeps-gen = import ./nix/packages/jsdeps-gen.nix { inherit pkgs lib; };
          packages.soldeps-gen = import ./nix/packages/soldeps-gen.nix { inherit pkgs lib; };
          packages.deps-extract = import ./nix/packages/deps-extract.nix { inherit pkgs lib; };
          packages.turnkey-composed = import ./nix/packages/turnkey-composed.nix { inherit pkgs lib; };

          # Expose turnkey-prelude for CI builds
          packages.turnkey-prelude =
            let
              overlaidPkgs = import inputs.nixpkgs {
                inherit system;
                overlays = [
                  inputs.teller.overlays.default
                  inputs.toolbox.overlays.default
                ];
              };
              registry = overlaidPkgs.turnkeyRegistry;
              upstreamPrelude = inputs.teller.lib.resolveTool registry "buck2-prelude" {};
            in
            import ./nix/buck2/prelude.nix { inherit pkgs lib upstreamPrelude; };

          # Configure turnkey to use our local toolchain files
          # Each file creates a corresponding shell
          turnkey.toolchains = {
            enable = true;
            tellerLib = inputs.teller.lib;
            # Compose registries via overlays, as designed
            tellerRegistry =
              let
                overlaidPkgs = import inputs.nixpkgs {
                  inherit system;
                  overlays = [
                    inputs.teller.overlays.default   # Base: standard nixpkgs toolchains
                    inputs.toolbox.overlays.default   # Adds beads, jj, etc. (versions merge)
                  ];
                };
              in
              overlaidPkgs.turnkeyRegistry;
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
