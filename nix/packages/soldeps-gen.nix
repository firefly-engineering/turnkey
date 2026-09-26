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
  nix-prefetch-cached = import ./nix-prefetch-cached.nix { inherit pkgs lib; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "soldeps-gen";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "src/cmd/soldeps-gen" ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Only build soldeps-gen, not other workspace members
  cargoBuildFlags = [ "-p" "soldeps-gen" ];
  cargoTestFlags = [ "-p" "soldeps-gen" ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary with what --prefetch runs: git for ls-remote, and
  # nix-prefetch-cached (plus the nix it calls) for hashes
  postInstall = ''
    wrapProgram $out/bin/soldeps-gen \
      --prefix PATH : ${lib.makeBinPath [ pkgs.git pkgs.nix nix-prefetch-cached ]}
  '';

  meta = {
    description = "Generate solidity-deps.toml from foundry.toml and package.json for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "soldeps-gen";
  };
}
