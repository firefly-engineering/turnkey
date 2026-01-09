# godeps-gen Nix package
#
# Builds the godeps-gen tool that generates go-deps.toml from go.mod/go.sum.
# This tool is used to create declarative Go dependency files for Buck2 integration.
#
# Uses the monorepo pattern: src points to repo root, subPackages selects the tool.
# Wraps the binary with prefetcher tools (nix-prefetch-github, nix) in PATH.
{ pkgs, lib }:

let
  # Read vendorHash from go-deps.toml (data-driven, not hardcoded)
  depsToml = builtins.fromTOML (builtins.readFile ../../go-deps.toml);
  vendorHash = depsToml.meta.vendorHash or (throw ''
    go-deps.toml is missing [meta] vendorHash field.
    Add the following to go-deps.toml:

    [meta]
    vendorHash = "sha256-XXXX..."

    To get the hash, run: nix build .#godeps-gen 2>&1 | grep 'got:'
  '');
in
pkgs.buildGoModule {
  pname = "godeps-gen";
  version = "0.1.0";

  # Monorepo: use repo root as source, select subpackage
  src = ../..;
  subPackages = [ "cmd/godeps-gen" ];

  # Hash from go-deps.toml [meta] section
  inherit vendorHash;

  # For wrapping the binary with prefetcher tools
  nativeBuildInputs = [ pkgs.makeWrapper ];

  # Wrap the binary to include prefetcher tools in PATH
  # - nix-prefetch-github: for GitHub, golang.org/x, gopkg.in, go.uber.org modules
  # - nix: for nix-prefetch-url (GoProxy fallback) and nix hash to-sri
  postInstall = ''
    wrapProgram $out/bin/godeps-gen \
      --prefix PATH : ${lib.makeBinPath [
        pkgs.nix-prefetch-github
        pkgs.nix
      ]}
  '';

  meta = {
    description = "Generate go-deps.toml from go.mod and go.sum for Buck2 integration";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    maintainers = [ ];
    mainProgram = "godeps-gen";
  };
}
