# Language Adapters for Dependency Cells
#
# Each adapter takes { pkgs, lib, genericBuilder } and provides:
#   - mkXxxDepPackage: Build a single dependency package
#   - mkXxxDepsCell: Build a complete dependency cell: its dep packages,
#     handed to genericBuilder.genericMkDepsCell (../default.nix) with the
#     language's vendor layout, merge commands and root rules.star
#   - buildInputs: Build inputs for per-dependency builds
#   - cellBuildInputs: Build inputs for cell builds

{ pkgs, lib, genericBuilder }:

{
  go = import ./go.nix { inherit pkgs lib genericBuilder; };
  rust = import ./rust.nix { inherit pkgs lib genericBuilder; };
  python = import ./python.nix { inherit pkgs lib genericBuilder; };
  javascript = import ./javascript.nix { inherit pkgs lib genericBuilder; };
  solidity = import ./solidity.nix { inherit pkgs lib genericBuilder; };
}
