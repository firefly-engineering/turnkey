# check-rust-edition-rs Nix package
#
# Builds the check-rust-edition-rs tool that verifies Rust edition
# consistency between Cargo.toml and rules.star files.
#
# Uses tree-sitter for proper Starlark AST parsing instead of regex.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/check-rust-edition-rs"
      "src/rust/starlark-parse"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "check-rust-edition-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "check-rust-edition-rs"
  ];
  cargoTestFlags = [
    "-p"
    "check-rust-edition-rs"
  ];

  meta = {
    description = "Check Rust edition consistency between Cargo.toml and rules.star";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "check-rust-edition-rs";
  };
}
