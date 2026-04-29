# turnkey-composed Nix package
#
# Builds the FUSE composition daemon.
#
# On macOS, links against macFUSE's libfuse3 at
# `/usr/local/lib/libfuse3.4.dylib`. The FSKit dispatch is selected at mount
# time via `-o backend=fskit` (see src/rust/composition/src/fuse/fuse_t/backend.rs);
# without that option, macFUSE falls back to the kext-based mount_macfuse
# helper which is blocked by syspolicyd on Apple Silicon Tahoe.
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
  # macFUSE installs libfuse3 to /usr/local/lib regardless of arch.
  # composition/build.rs sets the same path via cargo:rustc-link-search.
  LIBRARY_PATH = "/usr/local/lib";
})
