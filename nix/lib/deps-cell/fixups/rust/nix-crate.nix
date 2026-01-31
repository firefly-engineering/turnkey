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
#
# For Linux targets, we need: linux, linux_android (but not bsd, apple_targets)

{ lib }:

{
  # ==========================================================================
  # Rustc Flags
  # ==========================================================================

  rustcFlags = {
    # nix crate platform detection flags for Linux x86_64
    # These are the cfg_aliases that build.rs would set
    nix = [
      "--cfg" "linux"
      "--cfg" "linux_android"
    ];
  };
}
