# tw-rs Nix package
#
# The Rust port of tw (#214), built from the root Cargo workspace like
# rustdeps-gen. Until tw switches to it, it is compared with the Go tw by
# `nix run .#parity -- tw` (nix/parity/README.md) and stays out of the
# shell: its binary is named tw, so it must never be on PATH beside the
# Go one.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/tw-rs"
      "src/rust/conditions"
      "src/rust/deps-gen-kit"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
      "src/rust/project-sync"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "tw-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "tw-rs"
  ];
  # project-sync's tests too: they cover the sync tw runs
  cargoTestFlags = [
    "-p"
    "tw-rs"
    "-p"
    "project-sync"
  ];

  meta = {
    description = "Turnkey wrapper for native language tools with auto-sync (Rust port)";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "tw";
  };
}
