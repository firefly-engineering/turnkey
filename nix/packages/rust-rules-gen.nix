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
in
pkgs.rustPlatform.buildRustPackage {
  pname = "rust-rules-gen";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "src/cmd/rust-rules-gen" ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  cargoBuildFlags = [
    "-p"
    "rust-rules-gen"
  ];
  cargoTestFlags = [
    "-p"
    "rust-rules-gen"
  ];

  # The select() keys follow the conditions module's shared test cases
  TURNKEY_SPLIT_VECTORS = ../../src/go/pkg/conditions/testdata/split-vectors.json;

  meta = {
    description = "Generate a vendored Rust crate's rules.star from its package slice";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rust-rules-gen";
  };
}
