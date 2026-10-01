# rules-sync Nix package
#
# Builds rules-sync, which keeps the deps of a project's rules.star files in
# step with their sources and prints what it did as a JSON report
# (src/cmd/rules-sync). tk runs it for `tk rules` and the rules sync before
# a buck2 command: tk's package (nix/packages/tk.nix) imports this file, and
# builds the store path of its binary into tk. Written in Rust (ported from
# Go, #215), built from the workspace projection like the other Rust tools.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/rules-sync"
      "src/rust/conditions"
      "src/rust/deps-extract"
      "src/rust/deps-gen-kit"
      "src/rust/gomod"
      "src/rust/goparse"
      "src/rust/gostd"
      "src/rust/pep508"
      "src/rust/prefetch-cache"
      "src/rust/project-sync"
      "src/rust/rules-star"
      "src/rust/rules-syncer"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "rules-sync";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "rules-sync"
  ];
  cargoTestFlags = [
    "-p"
    "rules-sync"
  ];

  meta = {
    description = "Sync the deps of rules.star files with their sources, for tk";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rules-sync";
  };
}
