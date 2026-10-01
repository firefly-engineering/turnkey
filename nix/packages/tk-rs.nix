# tk-rs Nix package: tk's Rust port (#216), until it switches
#
# A separate package, outside the shell and never on PATH as tk: the parity
# harness (nix/parity) runs it against the Go tk (nix/packages/tk.nix). Its
# binary is named tk. Built from the root Cargo workspace like rustdeps-gen,
# with the shell completions tk.nix installs. Rules sync is the rules-syncer
# library, linked in: there is no rules-sync binary to bake in.
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
      "src/cmd/tk-rs"
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
  pname = "tk-rs";
  version = "0.1.0";

  inherit (projection) src;

  cargoLock.lockFileContents = projection.lock;

  cargoBuildFlags = [
    "-p"
    "tk-rs"
  ];
  cargoTestFlags = [
    "-p"
    "tk-rs"
  ];

  # buck2 is needed at build time to generate shell completions: the pinned
  # one, so they describe the buck2 the shell runs
  nativeBuildInputs = [
    buck2
    pkgs.installShellFiles
  ];

  postInstall = ''
    installShellCompletion --cmd tk \
      --bash <($out/bin/tk completion bash) \
      --zsh <($out/bin/tk completion zsh) \
      --fish <($out/bin/tk completion fish)
  '';

  meta = {
    description = "Turnkey CLI wrapper for buck2 with auto-sync (Rust port)";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "tk";
  };
}
