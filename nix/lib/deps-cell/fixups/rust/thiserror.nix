# thiserror build script fixup
#
# thiserror uses a build script to generate version-specific module
# aliases in `private.rs`. We pre-generate these to avoid needing
# to run build scripts at Buck2 build time.
#
# The generated file exposes a version-specific private module that
# allows thiserror_impl to work correctly with the exact version.
#
# Reference: https://github.com/dtolnay/thiserror/blob/main/build.rs

{ lib }:

{
  # ==========================================================================
  # Build Script Fixups
  # ==========================================================================

  buildScriptFixups = {
    # thiserror fixup: generates out_dir/private.rs with version-specific module
    thiserror = { patchVersion, vendorPath, ... }: ''
      # Fixup: thiserror build script output
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'THISERROR_PRIVATE'
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
THISERROR_PRIVATE
    '';
  };
}
