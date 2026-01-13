# E2E test runner package
#
# Provides the turnkey-e2e-runner script for running E2E tests.
# Used both locally and in CI.

{ pkgs, lib }:

let
  # Runtime dependencies for tests
  testDeps = with pkgs; [
    bash
    coreutils
    findutils
    git
    gnugrep
    gnused
    nix
  ];

in pkgs.writeShellApplication {
  name = "turnkey-e2e-runner";

  runtimeInputs = testDeps;

  text = ''
    # Find the e2e directory relative to the flake
    # When run via 'nix run', we need to locate the source
    if [[ -n "''${TURNKEY_E2E_DIR:-}" ]]; then
      E2E_DIR="$TURNKEY_E2E_DIR"
    elif [[ -d "./e2e" ]]; then
      E2E_DIR="./e2e"
    else
      echo "Error: Cannot find e2e directory" >&2
      echo "Run from the turnkey repo root, or set TURNKEY_E2E_DIR" >&2
      exit 1
    fi

    exec "$E2E_DIR/harness/runner.sh" "$@"
  '';

  meta = {
    description = "Turnkey E2E test runner";
    mainProgram = "turnkey-e2e-runner";
  };
}
