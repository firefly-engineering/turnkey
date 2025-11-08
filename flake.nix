{
  description = "Turnkey toolchain management for Nix flakes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devenv.url = "github:cachix/devenv";
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
        turnkey-devenv = ./nix/devenv/turnkey;
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
          system,
          ...
        }:
        {
          # Configure turnkey to use our local toolchain files
          # Each file creates a corresponding shell
          turnkey.toolchains = {
            enable = true;
            declarationFiles = {
              default = ./toolchain.toml; # Creates devShells.default with buck2 + nix
              ci = ./toolchain.ci.toml; # Creates devShells.ci with just nix
            };
          };
        };
    };
}
