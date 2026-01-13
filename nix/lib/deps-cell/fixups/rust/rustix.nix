# Rustix rustc flags
#
# rustix's build script detects the target platform and sets cfg flags
# for conditional compilation of platform-specific code.
#
# Reference: https://github.com/bytecodealliance/rustix/blob/main/build.rs

{ lib }:

{
  # ==========================================================================
  # Rustc Flags
  # ==========================================================================

  rustcFlags = {
    # rustix platform detection flags for Linux x86_64
    rustix = [
      "--cfg" "libc"
      "--cfg" "linux_like"
      "--cfg" "linux_kernel"
    ];
  };
}
