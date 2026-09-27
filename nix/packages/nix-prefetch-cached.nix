# nix-prefetch-cached Nix package
#
# Caching wrapper around nix-prefetch-url that avoids redundant network fetches.
# Stores hashes in ~/.cache/turnkey/prefetch-cache.json (configurable via TURNKEY_CACHE_DIR).
#
# godeps-gen prefetches through this tool; the Rust deps generators link the
# prefetch-cache crate it is built on (through deps-gen-kit) instead.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "nix-prefetch-cached";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [
      "src/cmd/nix-prefetch-cached"
      "src/rust/prefetch-cache"
    ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Only build nix-prefetch-cached
  cargoBuildFlags = [
    "-p"
    "nix-prefetch-cached"
  ];
  cargoTestFlags = [
    "-p"
    "nix-prefetch-cached"
  ];

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include nix in PATH for nix-prefetch-url and nix hash
  postInstall = ''
    wrapProgram $out/bin/nix-prefetch-cached \
      --prefix PATH : ${lib.makeBinPath [ pkgs.nix ]}
  '';

  meta = {
    description = "Caching wrapper around nix-prefetch-url for faster dependency resolution";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "nix-prefetch-cached";
  };
}
