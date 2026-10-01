# buckgen-rs Nix package
#
# The Rust port of buckgen (#213), built from the workspace projection like
# the other Rust tools. Its binary is named buckgen, but until the switch it
# is a separate package, outside the shell and the Go cell adapter:
# `nix run .#parity -- buckgen` compares it with the Go package.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/buckgen-rs"
      "src/rust/conditions"
      "src/rust/deps-gen-kit"
      "src/rust/goparse"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "buckgen-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "buckgen-rs"
  ];
  cargoTestFlags = [
    "-p"
    "buckgen-rs"
  ];

  meta = {
    description = "Generate rules.star files for Go dependencies in Buck2 (Rust port)";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "buckgen";
  };
}
