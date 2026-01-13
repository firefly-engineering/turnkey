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
      ../../python/__init__.py
      ../../python/cfg
      ../../python/cargo
      ../../cmd/compute-unified-features
    ];
  };

in
pkgs.writeShellApplication {
  name = "compute-unified-features";

  runtimeInputs = [ pkgs.python3 ];

  text = ''
    export PYTHONPATH="${pythonSrc}"
    exec python3 "${pythonSrc}/cmd/compute-unified-features/__main__.py" "$@"
  '';

  meta = {
    description = "Compute unified features for Rust crates";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "compute-unified-features";
  };
}
