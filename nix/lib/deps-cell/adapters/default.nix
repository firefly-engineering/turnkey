# Language Adapters for Dependency Cells
#
# Each adapter provides:
#   - mkXxxDepPackage: Build a single dependency package
#   - mkXxxDepsCell: Build a complete dependency cell (using generic builder)
#   - hooks: Hook functions for per-dependency phases
#   - cellHooks: Hook functions for cell merge phase
#   - buildInputs: Build inputs for per-dependency builds
#   - cellBuildInputs: Build inputs for cell builds

{ pkgs, lib, genericBuilder }:

{
  go = import ./go.nix { inherit pkgs lib genericBuilder; };
  rust = import ./rust.nix { inherit pkgs lib genericBuilder; };
  python = import ./python.nix { inherit pkgs lib genericBuilder; };
  javascript = import ./javascript.nix { inherit pkgs lib genericBuilder; };
}
