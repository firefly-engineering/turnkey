# Default versioned registry mapping toolchain names to packages
# This provides all standard toolchains that turnkey supports out of the box.
#
# Structure:
#   <toolchain> = {
#     versions = { "<version>" = <derivation>; ... };
#     default = "<version>";
#   };
#
# Users can extend via registryExtensions or custom overlays using mkRegistryOverlay.
{
  pkgs,
  lib ? pkgs.lib,
}:

let
  # Import custom packages (user-facing tools only)
  jrsonnet = import ../packages/jrsonnet.nix { inherit pkgs lib; };
  tk = import ../packages/tk.nix { inherit pkgs lib; };
  tw = import ../packages/tw.nix { inherit pkgs lib; };

  # Helper for single-version entries (most common case for now)
  single = pkg: {
    versions = {
      "default" = pkg;
    };
    default = "default";
  };

in
{
  # ==========================================================================
  # Build systems
  # ==========================================================================
  buck2 = single pkgs.buck2;

  # ==========================================================================
  # Nix tooling
  # ==========================================================================
  nix = single pkgs.nix;

  # ==========================================================================
  # Go toolchain
  # ==========================================================================
  go = single pkgs.go;
  golangci-lint = single pkgs.golangci-lint;
  gopls = single pkgs.gopls;

  # ==========================================================================
  # Rust toolchain
  # ==========================================================================
  rust = single pkgs.rustc;
  cargo = single pkgs.cargo;
  clippy = single pkgs.clippy;
  rustfmt = single pkgs.rustfmt;
  rust-analyzer = single pkgs.rust-analyzer;
  cargo-edit = single pkgs.cargo-edit; # Provides cargo add/rm/upgrade
  reindeer = single pkgs.reindeer;

  # ==========================================================================
  # Python toolchain
  # ==========================================================================
  python = single pkgs.python3;
  uv = single pkgs.uv; # Python package manager for lock file generation
  ruff = single pkgs.ruff; # Python linter and formatter
  pytest = single pkgs.python3Packages.pytest;

  # ==========================================================================
  # C/C++ toolchain
  # ==========================================================================
  cxx = single pkgs.stdenv.cc;
  # Use clangUseLLVM which has lld integration for -fuse-ld=lld to work
  clang = single pkgs.llvmPackages.clangUseLLVM;
  lld = single pkgs.lld;

  # ==========================================================================
  # JavaScript/TypeScript toolchain
  # ==========================================================================
  nodejs = single pkgs.nodejs;
  typescript = single pkgs.nodePackages.typescript;
  biome = single pkgs.biome; # Fast linter and formatter for JS/TS/JSON

  # ==========================================================================
  # Solidity toolchain
  # ==========================================================================
  # The 'solidity' entry is for the Buck2 toolchain - it provides solc in PATH
  # Use 'solc' and 'foundry' directly if you need specific tools
  solidity = single pkgs.solc; # Buck2 toolchain entry (provides solc)
  solc = single pkgs.solc; # Solidity compiler
  foundry = single pkgs.foundry; # Ethereum dev toolkit (forge, cast, anvil)

  # ==========================================================================
  # Data templating
  # ==========================================================================
  jsonnet = single jrsonnet; # Rust implementation (fastest)

  # ==========================================================================
  # Documentation tooling
  # ==========================================================================
  mdbook = single pkgs.mdbook;

  # ==========================================================================
  # Turnkey CLI tools
  # ==========================================================================
  tk = single tk;
  tw = single tw;
}
