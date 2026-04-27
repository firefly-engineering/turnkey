# nix crate rustc flags
#
# The nix crate uses cfg_aliases in build.rs to set up platform-specific
# conditional compilation flags. Since Buck2 doesn't run build.rs, we
# need to set these flags manually.
#
# Reference: https://github.com/nix-rust/nix/blob/master/build.rs
#
# The build.rs sets up these aliases:
#   - linux: target_os = "linux"
#   - linux_android: any(android, linux)
#   - bsd: any(freebsd, dragonfly, netbsd, openbsd, apple_targets)
#   - apple_targets: any(ios, macos, watchos, tvos, visionos)
#   - etc.

{ lib }:

{
  # ==========================================================================
  # Rustc Flags
  # ==========================================================================

  rustcFlags = {
    # Platform-specific flags: dict with linux/macos keys
    nix = {
      linux = [
        "--cfg" "linux"
        "--cfg" "linux_android"
        # Cap lints to warn: the nix crate uses #![deny(unused)] which breaks
        # with newer rustc versions that detect more dead code (e.g., GetCString)
        "--cap-lints" "warn"
      ];
      macos = [
        "--cfg" "apple_targets"
        "--cfg" "bsd"
        "--cap-lints" "warn"
      ];
    };
  };
}
