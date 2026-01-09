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

      perSystem = { pkgs, ... }: {
        # Enable turnkey toolchain management
        turnkey = {
          enable = true;
          declarationFile = ./toolchain.toml;
        };

        # Configure Buck2 integration
        devenv.shells.default = {
          turnkey.buck2 = {
            enable = true;
            # Prelude strategy: bundled (default), git, nix, or path
            # prelude.strategy = "bundled";
          };
        };
      };
    };
}
