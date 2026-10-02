# pydeps-cell Nix package
#
# Builds the tool that writes a vendored Python distribution's rules.star,
# with its dependencies evaluated per platform (src/cmd/pydeps-cell). The
# Python cell adapter runs it in each distribution's own derivation.
# Written in Rust (ported from Go, #212), built from the workspace
# projection like the other Rust tools.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/pydeps-cell"
      "src/rust/conditions"
      "src/rust/deps-gen-kit"
      "src/rust/gostd"
      "src/rust/pep508"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "pydeps-cell";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "pydeps-cell"
  ];
  cargoTestFlags = [
    "-p"
    "pydeps-cell"
  ];

  meta = {
    description = "Write a vendored Python distribution's rules.star, with per-platform dependencies";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "pydeps-cell";
  };
}
