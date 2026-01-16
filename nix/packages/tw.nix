# tw Nix package
#
# Builds the tw CLI - a transparent wrapper for native language tools
# (go, cargo, uv) that auto-syncs when dependency files change.
#
# Usage:
#   tw go get github.com/foo/bar    # runs go get, syncs if go.mod changed
#   tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
#   tw uv add requests              # runs uv add, syncs if pyproject.toml changed
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "tw";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/cmd/tw")
      (root + "/go/pkg/syncconfig")
      (root + "/go/pkg/syncer")
      (root + "/go/pkg/staleness")
      (root + "/go/pkg/snapshot")
    ];
  };
  subPackages = [ "cmd/tw" ];

  vendorHash = "sha256-FfIQwd6lWWP787ZaHHXffPTbdJpYEpwDJoD0tDVwLOM=";

  meta = {
    description = "Turnkey wrapper for native language tools with auto-sync";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "tw";
  };
}
