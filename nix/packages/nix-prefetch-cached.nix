# nix-prefetch-cached Nix package
#
# Caching wrapper around nix-prefetch-url that avoids redundant network fetches.
# Stores hashes in ~/.cache/turnkey/prefetch-cache.json (configurable via TURNKEY_CACHE_DIR).
#
# This tool is used by deps-gen tools (rustdeps-gen, pydeps-gen, godeps-gen) to
# speed up dependency resolution by caching previously fetched hashes.
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.rustPlatform.buildRustPackage {
  pname = "nix-prefetch-cached";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/Cargo.toml")
      (root + "/Cargo.lock")
      (root + "/cmd/nix-prefetch-cached")
      (root + "/rust/prefetch-cache")
      # Include other workspace members for Cargo workspace resolution
      (root + "/cmd/pydeps-gen/Cargo.toml")
      (root + "/cmd/rustdeps-gen/Cargo.toml")
      (root + "/examples/rust-hello/Cargo.toml")
      (root + "/examples/rust-hello-deps/Cargo.toml")
    ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Only build nix-prefetch-cached
  cargoBuildFlags = [ "-p" "nix-prefetch-cached" ];
  cargoTestFlags = [ "-p" "nix-prefetch-cached" ];

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
