# fuser crate rustc flags
#
# The fuser crate uses build.rs to detect the mount implementation:
# - Without "libfuse" feature on Linux: uses pure-rust implementation
# - With "libfuse" feature: probes for libfuse2/libfuse3 via pkg-config
#
# Since Buck2 doesn't run build.rs, we need to set this manually.
# We use the pure-rust implementation on Linux (no external libfuse dependency).
#
# Reference: https://github.com/cberner/fuser/blob/main/build.rs

{ lib }:

{
  # ==========================================================================
  # Rustc Flags
  # ==========================================================================

  rustcFlags = {
    # fuser mount implementation flag for Linux (pure-rust, no libfuse)
    # Use separate arguments to avoid Buck2 parsing issues with combined format
    fuser = [ "--cfg" ''fuser_mount_impl="pure-rust"'' ];
  };
}
