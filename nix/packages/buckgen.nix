# buckgen Nix package
#
# Builds the buckgen tool that generates rules.star files for Go dependencies.
# This parses Go source files directly without requiring importable code
# (no go.mod/main.go scaffolding needed).
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "buckgen";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/src/cmd/buckgen")
      (root + "/src/go/pkg/buckgen")
      (root + "/src/go/pkg/goparse")
      (root + "/src/go/pkg/conditional")
      (root + "/src/go/pkg/conditions")
      (root + "/src/go/pkg/starlark")
    ];
  };
  subPackages = [ "src/cmd/buckgen" ];

  # Use lib.fakeHash initially to get the correct hash:
  vendorHash = "sha256-4sLFkUmpdMTHbYaW01MELSokNtuCA1t+4dCghQYRHcI=";

  meta = {
    description = "Generate rules.star files for Go dependencies in Buck2";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "buckgen";
  };
}
