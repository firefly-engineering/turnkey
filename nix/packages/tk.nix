# tk Nix package
#
# Builds the tk CLI - a transparent wrapper around buck2 that auto-syncs.
# This tool ensures generated files (deps files, rules.star files, deps
# cells) are up-to-date before running buck2 commands that read the build
# graph.
#
# Usage:
#   tk build //some:target     # syncs first, then runs buck2 build
#   tk sync                    # explicit sync
#   tk check                   # check staleness (for CI)
#
# Written in Rust (ported from Go, #216), built from the root Cargo
# workspace like rustdeps-gen. Rules sync is the rules-syncer library,
# linked in.
{
  pkgs,
  lib,
  # The pinned buck2 (turnkeyLib.pinnedBuck2Release system).buck2, whose
  # completions tk's are made from
  buck2,
}:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  projection = cargoLib.workspaceProjection {
    inherit root;
    members = [
      "src/cmd/tk"
      "src/rust/buck2-args"
      "src/rust/conditions"
      "src/rust/deps-cells"
      "src/rust/deps-extract"
      "src/rust/deps-gen-kit"
      "src/rust/gomod"
      "src/rust/goparse"
      "src/rust/gostd"
      "src/rust/pep508"
      "src/rust/prefetch-cache"
      "src/rust/project-sync"
      "src/rust/rules-star"
      "src/rust/rules-syncer"
      "src/rust/testcache"
    ];
  };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "tk";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "tk"
  ];
  cargoTestFlags = [
    "-p"
    "tk"
  ];

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
