# Rust protocol code for turnkey-test-runner.
#
# Generated at build time from pinned upstream sources, never hand-copied:
# - buck2's test-runner protocol (test.proto and the data.proto / error.proto
#   / host_sharing.proto it imports), from buck2's source at the revision of
#   the pinned buck2 release (nix/buck2/buck2-source.nix);
# - the Remote Execution API, from bazelbuild/remote-apis, with the
#   googleapis revision remote-apis itself pins.
#
# The result is a directory of generated .rs files. Buck2 builds the runner
# with OUT_DIR pointing at it (as the rustdeps cell does for vendored crates'
# build scripts), and Cargo builds copy it into OUT_DIR from build.rs.
{
  pkgs,
  lib,
}:

let
  buck2Source = import ../buck2/buck2-source.nix { inherit pkgs lib; };
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  root = ../..;

  buck2Protos = buck2Source.protos;

  remoteApis = pkgs.fetchFromGitHub {
    owner = "bazelbuild";
    repo = "remote-apis";
    rev = "adbf4a27c86fbea4a37637a6cbcacef372406fe7";
    sparseCheckout = [
      "build/bazel/remote/execution/v2"
      "build/bazel/semver"
    ];
    hash = "sha256-KoO+3ABXvApRnysEfjRfgbLK3tCRmDat6htB0uvmOZs=";
  };

  # The googleapis revision remote-apis pins in its MODULE.bazel
  # (googleapis 0.0.0-20260130-c0fcb356).
  googleapis = pkgs.fetchFromGitHub {
    owner = "googleapis";
    repo = "googleapis";
    rev = "c0fcb35628690e9eb15dcefae41c651c67cd050b";
    sparseCheckout = [
      "google/api"
      "google/longrunning"
      "google/rpc"
    ];
    hash = "sha256-nBsu/mhnV4IJy+9g6kAPaikzJhbU/e+Dthg6FZvnn08=";
  };

  # Include roots: buck2/ holds buck2's protos flat (they import each other
  # by bare name), reapi/ holds build/bazel/... and google/...
  protos = pkgs.runCommand "test-runner-protos-buck2-${buck2Source.version}" { } ''
    mkdir -p $out/buck2 $out/reapi/build/bazel $out/reapi/google
    cp ${buck2Protos}/app/buck2_test_proto/test.proto $out/buck2/
    cp ${buck2Protos}/app/buck2_data/data.proto $out/buck2/
    cp ${buck2Protos}/app/buck2_data/error.proto $out/buck2/
    cp ${buck2Protos}/app/buck2_host_sharing_proto/host_sharing.proto $out/buck2/
    cp -r ${remoteApis}/build/bazel/remote ${remoteApis}/build/bazel/semver $out/reapi/build/bazel/
    cp -r ${googleapis}/google/api ${googleapis}/google/longrunning ${googleapis}/google/rpc $out/reapi/google/
  '';

  codegen = pkgs.rustPlatform.buildRustPackage {
    pname = "test-runner-codegen";
    version = "0.1.0";
    src = cargoLib.prunedCargoSource {
      inherit root;
      members = [ "src/cmd/test-runner-codegen" ];
    };
    cargoLock.lockFile = root + "/Cargo.lock";
    cargoBuildFlags = [
      "-p"
      "test-runner-codegen"
    ];
    doCheck = false;
  };
in
pkgs.runCommand "test-runner-protocol-buck2-${buck2Source.version}"
  {
    passthru = { inherit protos codegen; };
  }
  ''
    mkdir -p $out
    ${codegen}/bin/test-runner-codegen ${protos} $out
  ''
