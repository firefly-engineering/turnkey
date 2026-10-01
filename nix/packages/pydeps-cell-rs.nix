# pydeps-cell-rs Nix package
#
# The Rust port of pydeps-cell (#212), built from the workspace projection
# like the other Rust tools. Its binary is named pydeps-cell, but until the
# switch it is a separate package, outside the shell and the Python cell
# adapter: `nix run .#parity -- pydeps-cell` compares it with the Go
# package.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/pydeps-cell-rs"
      "src/rust/conditions"
      "src/rust/deps-gen-kit"
      "src/rust/gostd"
      "src/rust/pep508"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "pydeps-cell-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "pydeps-cell-rs"
  ];
  cargoTestFlags = [
    "-p"
    "pydeps-cell-rs"
  ];

  meta = {
    description = "Write the pydeps cell's rules.star files, with per-platform dependencies (Rust port)";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "pydeps-cell";
  };
}
