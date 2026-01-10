# rustdeps-gen Nix package
#
# Builds the rustdeps-gen tool that generates rust-deps.toml from Cargo.lock.
# This tool is used to create declarative Rust dependency files for Buck2 integration.
#
# Uses buildGoModule (standard Nix pattern for Go tools).
# The vendorHash is for this tool's build process only - dependency cells
# use per-crate fetching as described in docs/dependency-management.md.
{ pkgs, lib }:

let
  fs = lib.fileset;
  root = ../..;
in
pkgs.buildGoModule {
  pname = "rustdeps-gen";
  version = "0.1.0";

  src = fs.toSource {
    inherit root;
    fileset = fs.unions [
      (root + "/go.mod")
      (root + "/go.sum")
      (root + "/cmd/rustdeps-gen")
      (root + "/go/pkg/rustdeps")
    ];
  };
  subPackages = [ "cmd/rustdeps-gen" ];

  vendorHash = "sha256-6JdnoCmu3KvG3pNbzMS2Xo0igMAcIZjpeA0S8a4MPWY=";

  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include prefetcher tools in PATH
  postInstall = ''
    wrapProgram $out/bin/rustdeps-gen \
      --prefix PATH : ${lib.makeBinPath [
        pkgs.nix
      ]}
  '';

  meta = {
    description = "Generate rust-deps.toml from Cargo.lock for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "rustdeps-gen";
  };
}
