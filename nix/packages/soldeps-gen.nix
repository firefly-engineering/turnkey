# soldeps-gen Nix package
#
# Builds the soldeps-gen tool that generates solidity-deps.toml from foundry.toml
# and package.json. This tool is used to create declarative Solidity dependency
# files for Buck2 integration.
#
# Written in Rust for proper TOML and JSON parsing.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/soldeps-gen"
      "src/rust/deps-gen-kit"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "soldeps-gen";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  # Only build soldeps-gen, not other workspace members
  cargoBuildFlags = [
    "-p"
    "soldeps-gen"
  ];
  cargoTestFlags = [
    "-p"
    "soldeps-gen"
  ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary with what prefetching runs: git for ls-remote, and nix
  # (nix-prefetch-url and nix hash, through deps-gen-kit) for hashes
  postInstall = ''
    wrapProgram $out/bin/soldeps-gen \
      --prefix PATH : ${
        lib.makeBinPath [
          pkgs.git
          pkgs.nix
        ]
      }
  '';

  meta = {
    description = "Generate solidity-deps.toml from foundry.toml and package.json for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "soldeps-gen";
  };
}
