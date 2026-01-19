# gobuckify - generates BUCK files for Go dependencies
#
# This fetches gobuckify from the upstream facebook/buck2 repository and
# applies a patch to use the `go` command directly instead of
# `buck2 run toolchains//:go[go]`. This allows it to run inside Nix
# derivations where Buck2 isn't available.
#
# Upstream: https://github.com/facebook/buck2/tree/main/prelude/go/tools/gobuckify
# License: MIT or Apache-2.0

{ pkgs, lib }:

let
  # Pin to a specific tag for reproducibility
  buck2Version = "2026-01-02";
  buck2Rev = "ebb0fba54f1f840d689a9c10f56aa7f56ae3f38d";
  buck2Hash = "sha256-3TK3t/b3wPfhRTzLtNEb4tz5ev7voM71AGxOc4vBPmk=";

  # Fetch just the gobuckify tool source
  src = pkgs.fetchFromGitHub {
    owner = "facebook";
    repo = "buck2";
    rev = buck2Rev;
    hash = buck2Hash;
    sparseCheckout = [ "prelude/go/tools/gobuckify" ];
  };

in
pkgs.buildGoModule {
  pname = "gobuckify";
  version = buck2Version;

  inherit src;
  sourceRoot = "${src.name}/prelude/go/tools/gobuckify";

  patches = [
    ../patches/gobuckify/use-go-directly.patch
    ../patches/gobuckify/fix-goroutine-closure.patch
    ../patches/gobuckify/fix-platform-select-syntax.patch
    ../patches/gobuckify/fix-cross-platform-analysis.patch
    ../patches/gobuckify/configurable-buildfile-name.patch
  ];

  # gobuckify doesn't have a go.mod in the repo (it's a Buck2 project)
  # We need to create one for buildGoModule to work
  preBuild = ''
    cat > go.mod << EOF
    module github.com/facebook/buck2/prelude/go/tools/gobuckify
    go 1.21
    EOF
  '';

  # No external dependencies - uses only Go standard library
  vendorHash = null;

  meta = with lib; {
    description = "Generate BUCK files for Go third-party dependencies";
    homepage = "https://github.com/facebook/buck2";
    license = with licenses; [ mit asl20 ];
    maintainers = [ ];
  };
}
