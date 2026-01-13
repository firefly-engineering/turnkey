{ pkgs, lib ? pkgs.lib }:

# Default registry mapping toolchain names to nixpkgs derivations
# This is a simple initial implementation without version resolution
{
  # Build systems
  buck2 = pkgs.buck2;

  # Nix tooling
  nix = pkgs.nix;

  # Go dependency management (godeps-gen with prefetcher tools)
  godeps-gen = import ../packages/godeps-gen.nix { inherit pkgs lib; };

  # Python testing
  pytest = pkgs.python3Packages.pytest;
}
