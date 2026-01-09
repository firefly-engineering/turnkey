# tk Nix package
#
# Builds the tk CLI - a transparent wrapper around buck2 that auto-syncs.
# This tool ensures generated files (BUCK files, dependency cells) are
# up-to-date before running buck2 commands that read the build graph.
#
# Usage:
#   tk build //some:target     # syncs first, then runs buck2 build
#   tk sync                    # explicit sync
#   tk check                   # check staleness (for CI)
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "tk";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/cmd/tk")
      (root + "/go/pkg/syncconfig")
      (root + "/go/pkg/syncer")
      (root + "/go/pkg/staleness")
    ];
  };
  subPackages = [ "cmd/tk" ];

  vendorHash = "sha256-6JdnoCmu3KvG3pNbzMS2Xo0igMAcIZjpeA0S8a4MPWY=";

  meta = {
    description = "Turnkey CLI wrapper for buck2 with auto-sync";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "tk";
  };
}
