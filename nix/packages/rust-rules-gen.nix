# rust-rules-gen Nix package
#
# Builds rust-rules-gen, which generates one vendored Rust crate's rules.star
# from its package slice, inside the crate's own derivation in the Rust deps
# cell (nix/lib/deps-cell/adapters/rust.nix). Compiled, so the one process
# each crate's derivation starts is cheap.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [ "src/cmd/rust-rules-gen" ];
    # The conditions module's shared test cases, which the crate's testdata/
    # links to
    extraFiles = [ "src/testdata/split-vectors.json" ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "rust-rules-gen";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "rust-rules-gen"
  ];
  cargoTestFlags = [
    "-p"
    "rust-rules-gen"
  ];

  meta = {
    description = "Generate a vendored Rust crate's rules.star from its package slice";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rust-rules-gen";
  };
}
