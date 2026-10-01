# tw Nix package
#
# Builds the tw CLI - a transparent wrapper for native language tools
# (go, cargo, uv) that auto-syncs when dependency files change.
#
# Usage:
#   tw go get github.com/foo/bar    # runs go get, syncs if go.mod changed
#   tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
#   tw uv add requests              # runs uv add, syncs if pyproject.toml changed
#
# Written in Rust, built from the root Cargo workspace like rustdeps-gen.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/tw"
      "src/rust/conditions"
      "src/rust/deps-gen-kit"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
      "src/rust/project-sync"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "tw";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "tw"
  ];
  # project-sync's tests too: they cover the sync tw runs
  cargoTestFlags = [
    "-p"
    "tw"
    "-p"
    "project-sync"
  ];

  meta = {
    description = "Turnkey wrapper for native language tools with auto-sync";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "tw";
  };
}
