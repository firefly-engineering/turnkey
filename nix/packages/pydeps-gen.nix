# pydeps-gen Nix package
#
# Builds the pydeps-gen tool that generates python-deps.toml from pyproject.toml
# or requirements.txt. This tool is used to create declarative Python dependency
# files for Buck2 integration.
#
# Written in Rust for consistent tooling with rustdeps-gen.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  nix-prefetch-cached = import ./nix-prefetch-cached.nix { inherit pkgs lib; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "pydeps-gen";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "cmd/pydeps-gen" ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Only build pydeps-gen, not examples
  cargoBuildFlags = [ "-p" "pydeps-gen" ];
  cargoTestFlags = [ "-p" "pydeps-gen" ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include nix and nix-prefetch-cached in PATH for prefetching
  postInstall = ''
    wrapProgram $out/bin/pydeps-gen \
      --prefix PATH : ${lib.makeBinPath [ pkgs.nix nix-prefetch-cached ]}
  '';

  meta = {
    description = "Generate python-deps.toml from pyproject.toml or requirements.txt for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "pydeps-gen";
  };
}
