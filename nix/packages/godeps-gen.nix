# godeps-gen Nix package
#
# Builds the godeps-gen tool that generates go-deps.toml from go.mod/go.sum.
# This tool is used to create declarative Go dependency files for Buck2 integration.
#
# Uses buildGoModule (standard Nix pattern for Go tools).
# The vendorHash is for this tool's build process only - dependency cells
# use per-module fetching as described in docs/dependency-management.md.
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "godeps-gen";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/cmd/godeps-gen")
      (root + "/go/pkg/godeps")
    ];
  };
  subPackages = [ "cmd/godeps-gen" ];

  vendorHash = "sha256-qzjcuSUg5mPONQZnxz1kltrEhwtkwljCTEssULMAa78=";

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include prefetcher tools in PATH
  postInstall = ''
    wrapProgram $out/bin/godeps-gen \
      --prefix PATH : ${lib.makeBinPath [
        pkgs.nix-prefetch-github
        pkgs.nix
      ]}
  '';

  meta = {
    description = "Generate go-deps.toml from go.mod and go.sum for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "godeps-gen";
  };
}
