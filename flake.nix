{
  description = "Turnkey toolchain management for Nix flakes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devenv.url = "github:cachix/devenv";

    # Required by devenv for container support (even if unused)
    nix2container = {
      url = "github:nlewo/nix2container";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    mk-shell-bin.url = "github:rrbutani/nix-mk-shell-bin";

    # Beads - distributed git-backed graph issue tracker for AI agents
    beads = {
      url = "github:steveyegge/beads/v0.46.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Beads Viewer - visualization tool for beads graphs
    beads_viewer = {
      url = "github:Dicklesworthstone/beads_viewer/v0.12.1";
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
          packages.gobuckify = import ./nix/packages/gobuckify.nix { inherit pkgs lib; };
          packages.cargo-prune-workspace = import ./nix/packages/cargo-prune-workspace.nix { inherit pkgs lib; };
          packages.tk = import ./nix/packages/tk.nix { inherit pkgs lib; };
          packages.tw = import ./nix/packages/tw.nix { inherit pkgs lib; };
          packages.e2e-runner = import ./nix/packages/e2e-runner.nix { inherit pkgs lib; };
          packages.turnkey-prelude = import ./nix/buck2/prelude.nix { inherit pkgs lib; };

          # Configure turnkey to use our local toolchain files
          # Each file creates a corresponding shell
          turnkey.toolchains = {
            enable = true;
            declarationFiles = {
              default = ./toolchain.toml; # Creates devShells.default with buck2 + nix + beads + go
            };
            # Extend the default registry with packages from flake inputs
            registry = {
              buck2 = pkgs.buck2;
              nix = pkgs.nix;
              beads = inputs.beads.packages.${system}.default;
              beads_viewer = inputs.beads_viewer.packages.${system}.default;
              jj = inputs.jj.packages.${system}.default;
              # Language toolchains for Buck2 integration
              go = pkgs.go;
              rust = pkgs.rustc;
              cargo = pkgs.cargo;
              reindeer = pkgs.reindeer;
              python = pkgs.python3;
              uv = pkgs.uv;  # Python package manager for lock file generation
              cxx = pkgs.stdenv.cc;
              # Use clangUseLLVM which has lld integration for -fuse-ld=lld to work
              clang = pkgs.llvmPackages.clangUseLLVM;
              lld = pkgs.lld;
              # JavaScript/TypeScript (no Buck2 toolchain, but available in shell for genrule)
              nodejs = pkgs.nodejs;
              typescript = pkgs.nodePackages.typescript;
              # Internal tools
              godeps-gen = config.packages.godeps-gen;
              pydeps-gen = config.packages.pydeps-gen;
              rustdeps-gen = config.packages.rustdeps-gen;
              gobuckify = config.packages.gobuckify;
              tk = config.packages.tk;
              tw = config.packages.tw;
              # Note: go, cargo, uv are automatically wrapped by flake-parts module
              # when wrapNativeTools = true (the default)
              # Python testing
              pytest = pkgs.python3Packages.pytest;
              # Documentation tools
              mdbook = pkgs.mdbook;
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
            };
          };
        };
    };
}
