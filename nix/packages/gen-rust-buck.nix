# gen-rust-buck Nix package
#
# Builds the gen-rust-buck tool that generates rules.star files for Rust crates.
# This tool is used by rust-deps-cell.nix to create Buck2 build files
# for vendored Rust dependencies.
{ pkgs, lib }:

let
  root = ../..;

  # Source files needed for the package
  pythonSrc = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      ../../python/__init__.py
      ../../python/cfg
      ../../python/cargo
      ../../python/buck
      ../../cmd/gen-rust-buck
    ];
  };

in
pkgs.writeShellApplication {
  name = "gen-rust-buck";

  runtimeInputs = [ pkgs.python3 ];

  text = ''
    export PYTHONPATH="${pythonSrc}"
    exec python3 "${pythonSrc}/cmd/gen-rust-buck/__main__.py" "$@"
  '';

  meta = {
    description = "Generate rules.star files for Rust crates";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "gen-rust-buck";
  };
}
