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
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;

          # The default registry — provided by the turnkey flake-parts
          # module — ships with toolbox: buck2, nix, go, python, clang,
          # rust, uv, and more (all version-pinned). 'tk' is included as
          # a built-in extension. Add project-specific tools here:
          #
          # registryExtensions = {
          #   my-tool = {
          #     versions = { "default" = inputs.my-tool.packages.${pkgs.system}.default; };
          #     default = "default";
          #   };
          # };

          # Enable Buck2 integration. Each language flag wires the
          # corresponding deps-gen tool into PATH and registers the deps
          # cell with Buck2 — no need to list it in toolchain.toml.
          buck2 = {
            enable = true;
            prelude.strategy = "bundled";

            go = {
              enable = true;
              depsFile = ./go-deps.toml;
            };

            # rust = {
            #   enable = true;
            #   depsFile = ./rust-deps.toml;
            # };
            # python = {
            #   enable = true;
            #   depsFile = ./python-deps.toml;
            #   lockFile = ./pylock.toml;
            # };
          };
        };
      };
    };
}
