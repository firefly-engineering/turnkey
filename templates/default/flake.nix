{
  description = "Buck2 project managed by turnkey";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devenv.url = "github:cachix/devenv";
    turnkey.url = "github:firefly-engineering/turnkey";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
        inputs.turnkey.flakeModules.turnkey
      ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem = { pkgs, config, ... }: {
        # Enable turnkey toolchain management
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;

          # The default registry from turnkey provides buck2, nix, godeps-gen.
          # Extend it with additional tools:
          registry = {
            # Base tools (from nixpkgs)
            buck2 = pkgs.buck2;
            nix = pkgs.nix;
            go = pkgs.go;

            # Required by Buck2 toolchains (go needs python + cxx)
            python = pkgs.python3;
            clang = pkgs.llvmPackages.clang;
            lld = pkgs.llvmPackages.lld;

            # Turnkey tools
            godeps-gen = inputs.turnkey.packages.${pkgs.system}.godeps-gen;
            tk = inputs.turnkey.packages.${pkgs.system}.tk;
          };

          # Enable Buck2 integration
          buck2 = {
            enable = true;
            prelude.strategy = "bundled";

            # Go dependencies (auto-generated from go.mod/go.sum)
            go = {
              enable = true;
              depsFile = ./go-deps.toml;
            };

            # Uncomment to enable other languages:
            # rust = {
            #   enable = true;
            #   depsFile = ./rust-deps.toml;
            # };
            # python = {
            #   enable = true;
            #   depsFile = ./python-deps.toml;
            # };
          };
        };
      };
    };
}
