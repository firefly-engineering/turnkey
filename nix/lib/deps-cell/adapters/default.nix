# Language Adapters for Dependency Cells
#
# Each adapter provides:
#   - mkXxxDepPackage: Build a single dependency package
#   - mkXxxDepsCell: Build a complete dependency cell
#   - hooks: Hook functions for per-dependency phases
#   - cellHooks: Hook functions for cell merge phase
#   - buildInputs: Build inputs for per-dependency builds
#   - cellBuildInputs: Build inputs for cell builds
#   - mergeCommands: Shell commands for cell merge phase

{ pkgs, lib }:

{
  go = import ./go.nix { inherit pkgs lib; };
  rust = import ./rust.nix { inherit pkgs lib; };
  python = import ./python.nix { inherit pkgs lib; };
  javascript = import ./javascript.nix { inherit pkgs lib; };
}
