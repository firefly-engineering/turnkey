# Serde build script fixups and rustc flags
#
# Serde and serde_core use build scripts to generate version-specific module
# aliases in their `private.rs` files. We pre-generate these to avoid needing
# to run build scripts at Buck2 build time.
#
# serde_json uses a build script to detect CPU architecture for optimized
# integer parsing. We provide the rustc --cfg flag for this.
#
# The generated files expose version-specific private modules that allow
# serde_derive to work correctly with the exact version being used.

{ lib }:

{
  # ==========================================================================
  # Build Script Fixups
  # ==========================================================================

  buildScriptFixups = {
    # serde_core fixup: generates out_dir/private.rs with version-specific module
    serde_core = { patchVersion, vendorPath, ... }: ''
      # Fixup: serde_core build script output
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'SERDE_CORE_PRIVATE'
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
SERDE_CORE_PRIVATE
    '';

    # serde fixup: generates out_dir/private.rs with version-specific module
    # Also includes alias to serde_core_private for serde_derive compatibility
    serde = { patchVersion, vendorPath, ... }: ''
      # Fixup: serde build script output (includes serde_core_private alias)
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'SERDE_PRIVATE'
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
use serde_core::__private${patchVersion} as serde_core_private;
SERDE_PRIVATE
    '';
  };

  # ==========================================================================
  # Rustc Flags
  # ==========================================================================

  rustcFlags = {
    # serde_json uses fast 64-bit arithmetic on x86_64
    # Reference: https://github.com/serde-rs/json/blob/master/build.rs
    serde_json = [ "--cfg" ''fast_arithmetic=\"64\"'' ];
  };
}
