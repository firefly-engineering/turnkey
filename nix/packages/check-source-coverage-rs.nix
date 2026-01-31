# check-source-coverage-rs Nix package
#
# Builds the check-source-coverage-rs tool that verifies all source files
# are covered by Buck2 targets in rules.star files.
#
# Uses tree-sitter for proper Starlark AST parsing instead of regex.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "check-source-coverage-rs";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [
      "src/cmd/check-source-coverage-rs"
      "src/rust/starlark-parse"
    ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  cargoBuildFlags = [ "-p" "check-source-coverage-rs" ];
  cargoTestFlags = [ "-p" "check-source-coverage-rs" ];

  meta = {
    description = "Check that all source files are covered by Buck2 targets";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "check-source-coverage-rs";
  };
}
