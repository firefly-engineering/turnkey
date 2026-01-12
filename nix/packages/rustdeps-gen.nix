# rustdeps-gen Nix package
#
# Builds the rustdeps-gen tool that generates rust-deps.toml from Cargo.lock.
# This tool is used to create declarative Rust dependency files for Buck2 integration.
#
# Written in Rust for proper Cargo.lock parsing using the cargo-lock crate.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  nix-prefetch-cached = import ./nix-prefetch-cached.nix { inherit pkgs lib; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "rustdeps-gen";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "cmd/rustdeps-gen" ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Only build rustdeps-gen, not examples
  cargoBuildFlags = [ "-p" "rustdeps-gen" ];
  cargoTestFlags = [ "-p" "rustdeps-gen" ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include nix and nix-prefetch-cached in PATH for prefetching
  postInstall = ''
    wrapProgram $out/bin/rustdeps-gen \
      --prefix PATH : ${lib.makeBinPath [ pkgs.nix nix-prefetch-cached ]}
  '';

  meta = {
    description = "Generate rust-deps.toml from Cargo.lock for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rustdeps-gen";
  };
}
