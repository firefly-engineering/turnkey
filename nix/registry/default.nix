{ pkgs }:

# Default registry mapping toolchain names to nixpkgs derivations
# This is a simple initial implementation without version resolution
{
  # Build systems
  buck2 = pkgs.buck2;

  # Nix tooling
  nix = pkgs.nix;
}
