# godeps-gen Nix package
#
# Builds the godeps-gen tool that generates go-deps.toml from go.mod/go.sum.
# This tool is used to create declarative Go dependency files for Buck2 integration.
#
# Uses the monorepo pattern: src points to repo root, subPackages selects the tool.
{ pkgs, lib }:

pkgs.buildGoModule {
  pname = "godeps-gen";
  version = "0.1.0";

  # Monorepo: use repo root as source, select subpackage
  src = ../..;
  subPackages = [ "tools/godeps-gen" ];

  # Hash of vendored dependencies (golang.org/x/mod)
  # To update: run `nix build` and copy the expected hash from error
  vendorHash = "sha256-n9TLT4c8V+I+uEg4MMUJvW451wpIYIfro2sZRtFe9ig=";

  meta = {
    description = "Generate go-deps.toml from go.mod and go.sum for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    maintainers = [ ];
    mainProgram = "godeps-gen";
  };
}
