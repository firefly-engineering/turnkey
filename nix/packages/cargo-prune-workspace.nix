# cargo-prune-workspace Nix package
#
# Builds the cargo-prune-workspace tool that prunes Cargo.toml workspace
# members to a whitelist. Used by prunedCargoSource to create minimal
# source trees for Nix builds.
#
# Written in Go to avoid the chicken-and-egg problem of needing this tool
# to build Rust packages (including itself if it were in Rust).
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "cargo-prune-workspace";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/cmd/cargo-prune-workspace")
    ];
  };
  subPackages = [ "cmd/cargo-prune-workspace" ];

  vendorHash = "sha256-6JdnoCmu3KvG3pNbzMS2Xo0igMAcIZjpeA0S8a4MPWY=";

  meta = {
    description = "Prune Cargo.toml workspace members to a whitelist";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "cargo-prune-workspace";
  };
}
