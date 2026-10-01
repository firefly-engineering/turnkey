# Crates whose build script turnkey's builds need nothing from: turnkey
# builds them without running it, as Buck2 never runs build.rs. Most only
# probe the rustc version or target, emit cfgs for features turnkey doesn't
# use, or exist for targets it doesn't build for (Windows, wasm). blake3's
# picks a SIMD implementation (without it, the portable one is built), and
# heapless's gates APIs postcard doesn't use; starlark_syntax (rules-star)
# brings both, through pagable.
#
# A crate that turns out to need its build script's output gets a real
# fixup in its own family instead.
{ lib, ... }:

{
  rust =
    lib.genAttrs
      [
        "ahash"
        "allocative"
        "anyhow"
        "atomic-polyfill"
        "blake3"
        "crc32fast"
        "crossbeam-deque"
        "crossbeam-epoch"
        "crossbeam-utils"
        "erased-serde"
        "generic-array"
        "getrandom"
        "heapless"
        "httparse"
        "iana-time-zone-haiku"
        "icu_normalizer_data"
        "icu_properties_data"
        "libc"
        "logos-codegen"
        "memoffset"
        "num-traits"
        "objc2"
        "parking_lot_core"
        "portable-atomic"
        "portable-atomic-util"
        "prettyplease"
        "proc-macro2"
        "quote"
        "rayon-core"
        "rustls"
        "rustversion"
        "starlark_map"
        "syn"
        "tree-sitter-language"
        "typeid"
        "typenum"
        "wasm-bindgen"
        "wasm-bindgen-shared"
        "winapi"
        "winapi-i686-pc-windows-gnu"
        "winapi-x86_64-pc-windows-gnu"
        "windows_aarch64_gnullvm"
        "windows_aarch64_msvc"
        "windows_i686_gnu"
        "windows_i686_gnullvm"
        "windows_i686_msvc"
        "windows_x86_64_gnu"
        "windows_x86_64_gnullvm"
        "windows_x86_64_msvc"
        "wit-bindgen"
        "zerocopy"
        "zmij"
      ]
      (_: {
        buildScript.skip = true;
      });
}
