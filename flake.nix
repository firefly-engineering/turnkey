{
  description = "Turnkey toolchain management for Nix flakes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/e4bae1bd10c9c57b2cf517953ab70060a828ee6f";
    flake-parts.url = "github:hercules-ci/flake-parts/80daad04eddbbf5a4d883996a73f3f542fa437ac";
    devenv.url = "github:cachix/devenv/9bfc4a64c3a798ed8fa6cee3a519a9eac5e73cb5";

    # Required by devenv for container support (even if unused)
    nix2container = {
      url = "github:nlewo/nix2container/66f4b8a47e92aa744ec43acbb5e9185078983909";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    mk-shell-bin.url = "github:rrbutani/nix-mk-shell-bin/ff5d8bd4d68a347be5042e2f16caee391cd75887";

    # Beads - distributed git-backed graph issue tracker for AI agents
    beads = {
      url = "github:steveyegge/beads/v0.47.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Beads Viewer - visualization tool for beads graphs
    beads_viewer = {
      url = "github:Dicklesworthstone/beads_viewer/v0.13.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Jujutsu - Git-compatible VCS
    jj = {
      url = "github:jj-vcs/jj/v0.37.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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

      # Export turnkey library functions (mkRegistryOverlay, mkMetaPackage, resolveTool, etc.)
      # These require pkgs, so they're provided per-system
      flake.lib = builtins.listToAttrs (
        map
          (
            system:
            let
              pkgs = nixpkgs.legacyPackages.${system};
              lib = pkgs.lib;
            in
            {
              name = system;
              value = import ./nix/lib {
                inherit pkgs lib;
                currentTime = self.lastModified or 0;
              };
            }
          )
          [
            "x86_64-linux"
            "aarch64-linux"
            "x86_64-darwin"
            "aarch64-darwin"
          ]
      );

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
          packages.turnkey-prelude = import ./nix/buck2/prelude.nix { inherit pkgs lib; };

          # Configure turnkey to use our local toolchain files
          # Each file creates a corresponding shell
          turnkey.toolchains = {
            enable = true;
            declarationFiles = {
              default = ./toolchain.toml; # Creates devShells.default with buck2 + nix + beads + go
            };
            # Extend the default registry with packages from flake inputs
            # (standard toolchains like buck2, nix, go, tk, etc. come from default registry)
            # Each entry needs versioned format: { versions = {...}; default = "..."; }
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
                beads = single inputs.beads.packages.${system}.default;
                beads_viewer = single inputs.beads_viewer.packages.${system}.default;
                jj = single inputs.jj.packages.${system}.default;
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
              };
            };
          };
        };
    };
}
