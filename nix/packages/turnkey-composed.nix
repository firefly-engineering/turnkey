# turnkey-composed Nix package
#
# Builds the FUSE composition daemon.
# On macOS, links against FUSE-T's libfuse3 (/usr/local/lib).
# Uses sandbox-paths to make the system FUSE-T visible during build.
{ pkgs, lib }:

let
  root = ../..;
  cargoLib = import ../lib/cargo.nix { inherit pkgs lib; };
  isDarwin = pkgs.stdenv.isDarwin;
in
pkgs.rustPlatform.buildRustPackage ({
  pname = "turnkey-composed";
  version = "0.1.0";

  src = cargoLib.prunedCargoSource {
    inherit root;
    members = [
      "src/cmd/turnkey-composed"
      "src/rust/composition"
      "src/rust/nix-eval"
    ];
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
  };

  cargoBuildFlags = [ "-p" "turnkey-composed" ];
  doCheck = false;

  meta = {
    description = "FUSE composition daemon for Turnkey";
    homepage = "https://github.com/firefly-engineering/turnkey";
    license = lib.licenses.mit;
    mainProgram = "turnkey-composed";
  };
} // lib.optionalAttrs isDarwin {
  # macOS doesn't have a Nix sandbox in the traditional sense.
  # The build has access to /usr/local/lib where FUSE-T installs libfuse3.
  # The build.rs and composition/build.rs set -L/usr/local/lib for the linker.
  # We also set LIBRARY_PATH as a fallback.
  LIBRARY_PATH = "/usr/local/lib";
})
