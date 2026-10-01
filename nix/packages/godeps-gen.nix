# godeps-gen Nix package
#
# Builds the godeps-gen tool that generates go-deps.toml from a Go module or
# workspace, for Buck2 integration. Written in Rust (ported from Go, #211),
# built from the workspace projection like the other Rust tools.
#
# Wrapped to put the prefetcher on PATH. go, which resolves the workspace
# (go list -m all), is the shell's: the toolchain the project builds with,
# as rustdeps-gen's cargo is.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  nix-prefetch-cached = import ./nix-prefetch-cached.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/godeps-gen"
      "src/rust/deps-gen-kit"
      "src/rust/gomod"
      "src/rust/gostd"
      "src/rust/prefetch-cache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "godeps-gen";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "godeps-gen"
  ];
  cargoTestFlags = [
    "-p"
    "godeps-gen"
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
    description = "Generate go-deps.toml from go.mod and go.sum for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "godeps-gen";
  };
}
