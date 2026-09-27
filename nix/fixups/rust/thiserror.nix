# thiserror's build script writes out_dir/private.rs, exposing a private
# module named after the crate's patch version, which thiserror_impl's
# expansions refer to.
#
# Reference: https://github.com/dtolnay/thiserror/blob/master/build.rs
{ ... }:

{
  rust.thiserror.buildScript.generate = ctx: ''
    cat > "$OUT_DIR/private.rs" << 'THISERROR_PRIVATE'
    #[doc(hidden)]
    pub mod __private${ctx.versionParts.patch} {
        #[doc(hidden)]
        pub use crate::private::*;
    }
    THISERROR_PRIVATE
  '';
}
