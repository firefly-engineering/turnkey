# jsdeps-gen Nix package
#
# Builds the jsdeps-gen tool that generates js-deps.toml from pnpm-lock.yaml.
# This tool is used to create declarative JavaScript dependency files for Buck2 integration.
#
# Written in Rust for proper YAML parsing of pnpm lockfiles.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "jsdeps-gen";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "src/cmd/jsdeps-gen" ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Only build jsdeps-gen, not other workspace members
  cargoBuildFlags = [ "-p" "jsdeps-gen" ];
  cargoTestFlags = [ "-p" "jsdeps-gen" ];

  meta = {
    description = "Generate js-deps.toml from pnpm-lock.yaml for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "jsdeps-gen";
  };
}
