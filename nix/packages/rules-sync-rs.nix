# rules-sync-rs Nix package
#
# rules-sync's Rust port (#215), built from the workspace projection like
# the other Rust tools, while it is compared with the Go rules-sync
# (nix/parity/cases/rules-sync.toml). Outside the shell and tk; its binary
# is named rules-sync.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/rules-sync-rs"
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
  pname = "rules-sync-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "rules-sync-rs"
  ];
  cargoTestFlags = [
    "-p"
    "rules-sync-rs"
  ];

  meta = {
    description = "Sync the deps of rules.star files with their sources, for tk (Rust port)";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rules-sync";
  };
}
