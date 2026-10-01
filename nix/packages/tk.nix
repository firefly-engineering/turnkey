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
{
  pkgs,
  lib,
  # The pinned buck2 (turnkeyLib.pinnedBuck2Release system).buck2
  buck2,
}:

let
  fs = lib.fileset;
  root = ../..;
  # The rules-sync tk runs for rules sync: the one place it is chosen
  rulesSync = import ./rules-sync.nix { inherit pkgs lib; };
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
      (root + "/src/go/pkg/buck2args")
      (root + "/src/go/pkg/localconfig")
      (root + "/src/go/pkg/syncconfig")
      (root + "/src/go/pkg/conditions")
      (root + "/src/go/pkg/syncer")
      (root + "/src/go/pkg/staleness")
      # rules-sync's report, which tk reads
      (root + "/src/go/pkg/rulesreport")
      (root + "/src/go/pkg/cellfresh")
      (root + "/src/go/pkg/materialize")
      (root + "/src/go/pkg/testcache")
    ];
  };
  subPackages = [ "src/cmd/tk" ];

  vendorHash = "sha256-Lz9kCfY4vE6ytq1jzrX1qIRFx6EmBe/nxfhPc4sdGng=";

  # tk runs the rules-sync it was built with, never one found on PATH
  # (src/cmd/tk/rules.go)
  ldflags = [ "-X main.rulesSyncPath=${lib.getExe rulesSync}" ];

  # The rules-sync above, which flake.nix exposes as packages.rules-sync
  passthru = { inherit rulesSync; };

  # buck2 is needed at build time to generate shell completions: the pinned
  # one, so they describe the buck2 the shell runs
  nativeBuildInputs = [
    buck2
    pkgs.installShellFiles
  ];

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
