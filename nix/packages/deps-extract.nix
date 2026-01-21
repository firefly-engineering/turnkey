# deps-extract Nix package
#
# Builds the deps-extract tool that extracts imports from source files using tree-sitter.
# Supports Python, Rust, TypeScript, and Solidity via Cargo features.
#
# Written in Rust for proper AST parsing using tree-sitter grammars.
{
  pkgs,
  lib,
  # Language features to enable (default: all)
  enablePython ? true,
  enableRust ? true,
  enableTypescript ? true,
  enableSolidity ? true,
}:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };

  # Build features list based on enabled languages
  features = lib.concatStringsSep "," (
    lib.optional enablePython "python"
    ++ lib.optional enableRust "rust"
    ++ lib.optional enableTypescript "typescript"
    ++ lib.optional enableSolidity "solidity"
  );
in
pkgs.rustPlatform.buildRustPackage {
  pname = "deps-extract";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "src/rust/deps-extract" ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  # Build with selected features only (no default features)
  cargoBuildFlags = [
    "-p" "deps-extract"
    "--no-default-features"
    "--features" features
  ];
  cargoTestFlags = [
    "-p" "deps-extract"
    "--no-default-features"
    "--features" features
  ];

  meta = {
    description = "Extract imports from source files using tree-sitter";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "deps-extract";
  };
}
