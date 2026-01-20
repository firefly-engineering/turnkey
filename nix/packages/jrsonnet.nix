# jrsonnet Nix package
#
# Builds jrsonnet - a fast Rust implementation of Jsonnet.
# https://github.com/CertainLach/jrsonnet
#
# We package this ourselves because nixpkgs has an outdated version (0.4.2)
# that doesn't compile with modern Rust.
{ pkgs, lib }:

let
  version = "0.5.0-pre96-test";
in
pkgs.rustPlatform.buildRustPackage {
  pname = "jrsonnet";
  inherit version;

  src = pkgs.fetchFromGitHub {
    owner = "CertainLach";
    repo = "jrsonnet";
    rev = "v${version}";
    hash = "sha256-dm62UkL8lbvU3Ftjj6K5ziZGuHpFyLUzyTg9x/+no54=";
  };

  cargoLock = {
    lockFile = pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/CertainLach/jrsonnet/v${version}/Cargo.lock";
      hash = "sha256-7CKKXh5d6s4AXQtc9ojikqfJl1AfuvRYYtcztRigGhI=";
    };
    allowBuiltinFetchGit = true;
  };

  # Only build the main jrsonnet binary
  cargoBuildFlags = [ "-p" "jrsonnet" ];
  cargoTestFlags = [ "-p" "jrsonnet" ];

  # Skip tests that require network or have issues in sandbox
  doCheck = false;

  # Create a 'jsonnet' symlink for compatibility with tools expecting that name
  postInstall = ''
    ln -s $out/bin/jrsonnet $out/bin/jsonnet
  '';

  meta = {
    description = "Rust implementation of Jsonnet language";
    homepage = "https://github.com/CertainLach/jrsonnet";
    license = lib.licenses.mit;
    mainProgram = "jrsonnet";
  };
}
