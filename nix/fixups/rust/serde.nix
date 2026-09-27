# serde, serde_core and serde_json.
#
# serde and serde_core's build scripts write out_dir/private.rs, exposing a
# private module named after the crate's patch version, which serde_derive's
# expansions refer to. serde_json's build script sets fast_arithmetic="64" on
# 64-bit targets, which every platform turnkey builds for is.
#
# References: https://github.com/serde-rs/serde/blob/master/serde/build.rs,
# https://github.com/serde-rs/json/blob/master/build.rs
{ lib, ... }:

let
  # out_dir/private.rs, plus the lines `extra` gives for the crate
  privateModule = extra: ctx: ''
    cat > "$OUT_DIR/private.rs" << 'SERDE_PRIVATE'
    #[doc(hidden)]
    pub mod __private${ctx.versionParts.patch} {
        #[doc(hidden)]
        pub use crate::private::*;
    }
    ${lib.concatMapStrings (line: line + "\n") (extra ctx)}SERDE_PRIVATE
  '';
in
{
  rust.serde_core.buildScript.generate = privateModule (_: [ ]);

  # serde also aliases serde_core's private module, for serde_derive
  rust.serde.buildScript.generate = privateModule (ctx: [
    "use serde_core::__private${ctx.versionParts.patch} as serde_core_private;"
  ]);

  rust.serde_json.rustcFlags = [
    "--cfg"
    ''fast_arithmetic="64"''
  ];
}
