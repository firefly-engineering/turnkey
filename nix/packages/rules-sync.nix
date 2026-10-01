# rules-sync Nix package
#
# Builds rules-sync, which keeps the deps of a project's rules.star files in
# step with their sources and prints what it did as a JSON report. tk runs
# it for `tk rules` and the rules sync before a buck2 command: tk's package
# (nix/packages/tk.nix) imports this file, and builds the store path of its
# binary into tk. That import is the one a Rust rules-sync repoints.
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "rules-sync";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/src/cmd/rules-sync")
      (root + "/src/go/pkg/rulesreport")
      (root + "/src/go/pkg/rulessync")
      (root + "/src/go/pkg/mapper")
      # mapper's tests import it; go mod vendor reads tests too
      (root + "/src/go/pkg/godeps")
      (root + "/src/go/pkg/goparse")
      (root + "/src/go/pkg/extraction")
      (root + "/src/go/pkg/starlark")
      (root + "/src/go/pkg/syncconfig")
      (root + "/src/go/pkg/conditional")
      (root + "/src/go/pkg/conditions")
      (root + "/src/go/pkg/cargocfg")
      (root + "/src/go/pkg/cargofeatures")
      (root + "/src/go/pkg/pep508")
    ];
  };
  subPackages = [ "src/cmd/rules-sync" ];

  vendorHash = "sha256-yJBhZBLYe5LRvDccN2gdIETa6J4H3G9FMzfwqPjFeuQ=";

  meta = {
    description = "Sync the deps of rules.star files with their sources, for tk";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rules-sync";
  };
}
