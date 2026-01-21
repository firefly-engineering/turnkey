# tk Nix package
#
# Builds the tk CLI - a transparent wrapper around buck2 that auto-syncs.
# This tool ensures generated files (rules.star files, dependency cells) are
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
      (root + "/src/cmd/tk")
      (root + "/src/go/pkg/localconfig")
      (root + "/src/go/pkg/syncconfig")
      (root + "/src/go/pkg/syncer")
      (root + "/src/go/pkg/staleness")
      (root + "/src/go/pkg/rules")
    ];
  };
  subPackages = [ "src/cmd/tk" ];

  vendorHash = "sha256-Vgqdy+jGLYByPiGY8z45+nSYo5YHpmlyHjmfAcYEyjU=";

  # buck2 is needed at build time to generate shell completions
  nativeBuildInputs = [ pkgs.buck2 pkgs.installShellFiles ];

  postInstall = ''
    # Generate and install shell completions
    installShellCompletion --cmd tk \
      --bash <($out/bin/tk completion bash) \
      --zsh <($out/bin/tk completion zsh) \
      --fish <($out/bin/tk completion fish)
  '';

  meta = {
    description = "Turnkey CLI wrapper for buck2 with auto-sync";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "tk";
  };
}
