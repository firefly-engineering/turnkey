# turnkey-test-runner Nix package
#
# Turnkey's buck2 test runner. Its protocol code is generated for one buck2
# release (nix/packages/test-runner-protocol.nix), so the package is built per
# buck2 version.
{
  pkgs,
  lib,
  buck2Version,
}:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  protocol = import ./test-runner-protocol.nix { inherit pkgs lib buck2Version; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "turnkey-test-runner";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [ "src/cmd/turnkey-test-runner" ];
  };

  cargoLock.lockFile = root + "/Cargo.lock";
  cargoBuildFlags = [
    "-p"
    "turnkey-test-runner"
  ];
  doCheck = false;

  TURNKEY_TEST_RUNNER_PROTOCOL = protocol;

  passthru = { inherit protocol; };

  meta = {
    description = "Turnkey's buck2 test runner, with test result caching";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "turnkey-test-runner";
  };
}
