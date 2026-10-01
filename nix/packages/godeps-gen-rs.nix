# godeps-gen-rs Nix package
#
# The Rust port of godeps-gen (#211), built from the workspace projection
# like the other Rust tools. Its binary is named godeps-gen, but until the
# switch it is a separate package, outside the shell and never on PATH:
# `nix run .#parity -- godeps-gen` compares it with the Go package.
#
# Wrapped as the Go package is: the prefetcher on PATH, and the shell's go,
# which resolves the workspace (go list -m all).
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  nix-prefetch-cached = import ./nix-prefetch-cached.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/godeps-gen-rs"
      "src/rust/deps-gen-kit"
      "src/rust/gomod"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
    ];
    # The godeps fixtures the crate's testdata/ links to
    extraFiles = [ "src/testdata/godeps" ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "godeps-gen-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "godeps-gen-rs"
  ];
  cargoTestFlags = [
    "-p"
    "godeps-gen-rs"
  ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  postInstall = ''
    wrapProgram $out/bin/godeps-gen \
      --prefix PATH : ${
        lib.makeBinPath [
          pkgs.nix
          nix-prefetch-cached
        ]
      }
  '';

  meta = {
    description = "Generate go-deps.toml from go.mod and go.sum for Buck2 integration (Rust port)";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "godeps-gen";
  };
}
