# Serde build script fixups
#
# Serde and serde_core use build scripts to generate version-specific module
# aliases in their `private.rs` files. We pre-generate these to avoid needing
# to run build scripts at Buck2 build time.
#
# The generated files expose version-specific private modules that allow
# serde_derive to work correctly with the exact version being used.

{ lib }:

{
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
}
