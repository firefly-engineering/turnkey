# pydeps-cell Nix package
#
# Builds the tool that writes the pydeps cell's rules.star files, with the
# dependencies between vendored Python packages evaluated per platform
# (src/go/pkg/pydepscell). The Python cell adapter runs it at merge time.
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "pydeps-cell";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/src/cmd/pydeps-cell")
      (root + "/src/go/pkg/pydepscell")
      (root + "/src/go/pkg/conditions")
      (root + "/src/go/pkg/pep508")
      (root + "/src/go/pkg/starlark")
    ];
  };
  subPackages = [ "src/cmd/pydeps-cell" ];

  vendorHash = "sha256-Vgqdy+jGLYByPiGY8z45+nSYo5YHpmlyHjmfAcYEyjU=";

  meta = {
    description = "Write the pydeps cell's rules.star files, with per-platform dependencies";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "pydeps-cell";
  };
}
