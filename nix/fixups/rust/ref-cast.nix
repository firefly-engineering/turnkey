# ref-cast's build script writes out_dir/private.rs, exposing a private
# module named after the crate's patch version, which ref-cast-impl's
# expansions refer to, as thiserror's does (thiserror.nix). Its other
# outputs are cfgs for compilers older than turnkey's.
#
# Reference: https://github.com/dtolnay/ref-cast/blob/master/build.rs
{ ... }:

{
  rust.ref-cast.buildScript.generate = ctx: ''
    cat > "$OUT_DIR/private.rs" << 'REF_CAST_PRIVATE'
    #[doc(hidden)]
    pub mod __private${ctx.versionParts.patch} {
        #[doc(hidden)]
        pub use crate::private::*;
    }
    REF_CAST_PRIVATE
  '';
}
