# Rustix rustc flags
#
# rustix's build script detects the target platform and sets cfg flags
# for conditional compilation of platform-specific code.
#
# Reference: https://github.com/bytecodealliance/rustix/blob/main/build.rs
#
# On non-Linux platforms, rustix always uses the libc backend (os != "linux").
# The build.rs also sets OS-family aliases (apple, bsd, linux_like, etc.)
# and kernel flags (linux_kernel) based on the target OS.

{ lib }:

{
  # ==========================================================================
  # Rustc Flags
  # ==========================================================================

  rustcFlags = {
    # Platform-specific flags: dict with linux/macos keys
    # generates select() in Buck2 BUCK files
    rustix = {
      linux = [
        "--cfg" "libc"
        "--cfg" "linux_like"
        "--cfg" "linux_kernel"
      ];
      macos = [
        "--cfg" "libc"
        "--cfg" "apple"
        "--cfg" "bsd"
      ];
    };
  };
}
