# compute-unified-features Nix package
#
# Builds the compute-unified-features tool that computes unified features
# for all Rust crates in a vendor directory. This implements Cargo-style
# feature unification for Buck2 builds.
{ pkgs, lib }:

let
  root = ../..;

  # Source files needed for the package
  pythonSrc = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      ../../src/python/__init__.py
      ../../src/python/cfg
      ../../src/python/cargo
      ../../src/cmd/compute-unified-features
    ];
  };

in
pkgs.writeShellApplication {
  name = "compute-unified-features";

  runtimeInputs = [ pkgs.python3 ];

  text = ''
    export PYTHONPATH="${pythonSrc}/src"
    exec python3 "${pythonSrc}/src/cmd/compute-unified-features/__main__.py" "$@"
  '';

  meta = {
    description = "Compute unified features for Rust crates";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "compute-unified-features";
  };
}
