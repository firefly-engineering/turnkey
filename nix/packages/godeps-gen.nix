# godeps-gen Nix package
#
# Builds the godeps-gen tool that generates go-deps.toml from go.mod/go.sum.
# This tool is used to create declarative Go dependency files for Buck2 integration.
{ pkgs, lib }:

pkgs.buildGoModule {
  pname = "godeps-gen";
  version = "0.1.0";

  src = ../../tools/godeps-gen;

  # godeps-gen only uses Go standard library, no external dependencies
  vendorHash = null;

  meta = {
    description = "Generate go-deps.toml from go.mod and go.sum for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    maintainers = [ ];
    mainProgram = "godeps-gen";
  };
}
