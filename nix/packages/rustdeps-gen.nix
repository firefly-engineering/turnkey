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
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/rustdeps-gen"
      "src/rust/deps-gen-kit"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "rustdeps-gen";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  # Only build rustdeps-gen, not examples
  cargoBuildFlags = [
    "-p"
    "rustdeps-gen"
  ];
  cargoTestFlags = [
    "-p"
    "rustdeps-gen"
  ];
  # The ignored tests run cargo on a vendored fixture workspace: Buck2's
  # tests have no cargo, the build has
  checkFlags = [ "--include-ignored" ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include nix in PATH: deps-gen-kit prefetches with
  # nix-prefetch-url and nix hash
  postInstall = ''
    wrapProgram $out/bin/rustdeps-gen \
      --prefix PATH : ${lib.makeBinPath [ pkgs.nix ]}
  '';

  meta = {
    description = "Generate rust-deps.toml from Cargo.lock for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rustdeps-gen";
  };
}
