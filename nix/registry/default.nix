{ pkgs, lib ? pkgs.lib }:

# Default registry mapping toolchain names to nixpkgs derivations
# This provides all standard toolchains that turnkey supports out of the box.
# Users can extend this via registryExtensions without duplicating these entries.
{
  # ==========================================================================
  # Build systems
  # ==========================================================================
  buck2 = pkgs.buck2;

  # ==========================================================================
  # Nix tooling
  # ==========================================================================
  nix = pkgs.nix;

  # ==========================================================================
  # Go toolchain
  # ==========================================================================
  go = pkgs.go;
  golangci-lint = pkgs.golangci-lint;
  godeps-gen = import ../packages/godeps-gen.nix { inherit pkgs lib; };
  gobuckify = import ../packages/gobuckify.nix { inherit pkgs lib; };

  # ==========================================================================
  # Rust toolchain
  # ==========================================================================
  rust = pkgs.rustc;
  cargo = pkgs.cargo;
  clippy = pkgs.clippy;
  rustfmt = pkgs.rustfmt;
  rust-analyzer = pkgs.rust-analyzer;
  cargo-edit = pkgs.cargo-edit;  # Provides cargo add/rm/upgrade
  reindeer = pkgs.reindeer;
  rustdeps-gen = import ../packages/rustdeps-gen.nix { inherit pkgs lib; };

  # ==========================================================================
  # Python toolchain
  # ==========================================================================
  python = pkgs.python3;
  uv = pkgs.uv;  # Python package manager for lock file generation
  ruff = pkgs.ruff;  # Python linter and formatter
  pydeps-gen = import ../packages/pydeps-gen.nix { inherit pkgs lib; };
  pytest = pkgs.python3Packages.pytest;

  # ==========================================================================
  # C/C++ toolchain
  # ==========================================================================
  cxx = pkgs.stdenv.cc;
  # Use clangUseLLVM which has lld integration for -fuse-ld=lld to work
  clang = pkgs.llvmPackages.clangUseLLVM;
  lld = pkgs.lld;

  # ==========================================================================
  # JavaScript/TypeScript toolchain
  # ==========================================================================
  nodejs = pkgs.nodejs;
  typescript = pkgs.nodePackages.typescript;
  jsdeps-gen = import ../packages/jsdeps-gen.nix { inherit pkgs lib; };

  # ==========================================================================
  # Documentation tooling
  # ==========================================================================
  mdbook = pkgs.mdbook;

  # ==========================================================================
  # Turnkey CLI tools
  # ==========================================================================
  tk = import ../packages/tk.nix { inherit pkgs lib; };
  tw = import ../packages/tw.nix { inherit pkgs lib; };
}
