# buckgen Nix package
#
# Builds the tool that writes the rules.star files of a Go module in the Go
# deps cell (src/cmd/buckgen), from the module's own source: it parses the
# Go files directly, without the go command. Each module's derivation runs
# it (mkGoDepPackage, nix/lib/deps-cell/adapters/go.nix). Written in Rust
# (ported from Go, #213), built from the workspace projection like the
# other Rust tools.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/buckgen"
      "src/rust/conditions"
      "src/rust/deps-gen-kit"
      "src/rust/goparse"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "buckgen";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "buckgen"
  ];
  cargoTestFlags = [
    "-p"
    "buckgen"
  ];

  meta = {
    description = "Generate rules.star files for Go dependencies in Buck2";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "buckgen";
  };
}
