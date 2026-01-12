# Rust dependencies cell builder
#
# Reads a rust-deps.toml file and builds a Buck2 cell containing
# all crate dependencies with BUCK files for rust_library targets.
#
# The TOML file format (supports multiple versions of same crate):
#   [deps."crate-name@1.0.0"]
#   name = "crate-name"
#   version = "1.0.0"
#   hash = "sha256-..."
#   features = ["feature1", "feature2"]  # optional
#
# This allows downstream repos to declare deps in pure data files.
#
# Feature unification:
# Features are unified across the dependency graph, matching Cargo's behavior.
# If any crate requires feature X on crate Y, crate Y is built with feature X.
#
# Manual overrides can be specified in an optional featuresFile (rust-features.toml).
#
# Build script fixups:
# Some crates have build scripts that generate files needed at compile time.
# We handle these by pre-generating the output in Nix.

{ pkgs, lib, depsFile, featuresFile ? null, rustcFlagsRegistry ? {}, buildScriptFixups ? {} }:

# Build tools needed for native code compilation (ring, etc.)
# Using stdenv.cc for the C compiler and binutils for ar
let buildTools = with pkgs; [ stdenv.cc perl ];
in

let
  # ==========================================================================
  # Default registries
  # ==========================================================================

  # Default rustc flags for crates whose build scripts generate cfg directives
  defaultRustcFlagsRegistry = {
    serde_json = ["--cfg" ''fast_arithmetic=\"64\"''];
    rustix = ["--cfg" "libc" "--cfg" "linux_like" "--cfg" "linux_kernel"];
  };

  # Merge user-provided rustc flags with defaults (user takes precedence)
  mergedRustcFlagsRegistry = defaultRustcFlagsRegistry // rustcFlagsRegistry;

  # Default build script fixups
  # Each fixup is a function that receives { crateName, version, patchVersion, key, vendorPath }
  # and returns shell commands to execute
  defaultBuildScriptFixups = {
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

    # Ring requires compiling native crypto library
    # The fixup uses RING_PREFIX to namespace symbols by version
    ring = { patchVersion, vendorPath, ... }: ''
      # Fixup: ring native crypto library compilation
      # Ring's build.rs compiles C and assembly files into libring_core_*.a
      # We replicate this in Nix for Buck2 to link against
      echo "Building ring native crypto library..."
      RING_SRC="$out/${vendorPath}"
      RING_OUT="$out/${vendorPath}/out_dir"
      mkdir -p "$RING_OUT"

      # Symbol prefix to avoid conflicts (matches ring's build.rs)
      # Note: The prefix ends with double underscore, matching what ring's Rust code expects
      RING_PREFIX="ring_core_0_17_''${patchVersion}__"

      # Generate prefix header for symbol namespacing
      # Ring expects this at ring_core_generated/prefix_symbols.h
      # This list matches SYMBOLS_TO_PREFIX and SYMBOLS_TO_RENAME from ring's build.rs
      mkdir -p "$RING_OUT/ring_core_generated"
      cat > "$RING_OUT/ring_core_generated/prefix_symbols.h" << RING_PREFIX_HEADER
#ifndef ring_core_generated_PREFIX_SYMBOLS_H
#define ring_core_generated_PREFIX_SYMBOLS_H

// Symbol renames (from SYMBOLS_TO_RENAME in build.rs)
#define ecp_nistz256_point_double p256_point_double
#define ecp_nistz256_point_add p256_point_add
#define ecp_nistz256_point_add_affine p256_point_add_affine
#define ecp_nistz256_ord_mul_mont p256_scalar_mul_mont
#define ecp_nistz256_ord_sqr_mont p256_scalar_sqr_rep_mont
#define ecp_nistz256_mul_mont p256_mul_mont
#define ecp_nistz256_sqr_mont p256_sqr_mont

// All symbols from SYMBOLS_TO_PREFIX in build.rs
#define adx_bmi2_available ''${RING_PREFIX}adx_bmi2_available
#define avx2_available ''${RING_PREFIX}avx2_available
#define CRYPTO_memcmp ''${RING_PREFIX}CRYPTO_memcmp
#define CRYPTO_poly1305_finish ''${RING_PREFIX}CRYPTO_poly1305_finish
#define CRYPTO_poly1305_finish_neon ''${RING_PREFIX}CRYPTO_poly1305_finish_neon
#define CRYPTO_poly1305_init ''${RING_PREFIX}CRYPTO_poly1305_init
#define CRYPTO_poly1305_init_neon ''${RING_PREFIX}CRYPTO_poly1305_init_neon
#define CRYPTO_poly1305_update ''${RING_PREFIX}CRYPTO_poly1305_update
#define CRYPTO_poly1305_update_neon ''${RING_PREFIX}CRYPTO_poly1305_update_neon
#define ChaCha20_ctr32 ''${RING_PREFIX}ChaCha20_ctr32
#define ChaCha20_ctr32_avx2 ''${RING_PREFIX}ChaCha20_ctr32_avx2
#define ChaCha20_ctr32_neon ''${RING_PREFIX}ChaCha20_ctr32_neon
#define ChaCha20_ctr32_nohw ''${RING_PREFIX}ChaCha20_ctr32_nohw
#define ChaCha20_ctr32_ssse3 ''${RING_PREFIX}ChaCha20_ctr32_ssse3
#define ChaCha20_ctr32_ssse3_4x ''${RING_PREFIX}ChaCha20_ctr32_ssse3_4x
#define LIMB_is_zero ''${RING_PREFIX}LIMB_is_zero
#define LIMBS_add_mod ''${RING_PREFIX}LIMBS_add_mod
#define LIMBS_are_zero ''${RING_PREFIX}LIMBS_are_zero
#define LIMBS_equal ''${RING_PREFIX}LIMBS_equal
#define LIMBS_less_than ''${RING_PREFIX}LIMBS_less_than
#define LIMBS_reduce_once ''${RING_PREFIX}LIMBS_reduce_once
#define LIMBS_select_512_32 ''${RING_PREFIX}LIMBS_select_512_32
#define LIMBS_shl_mod ''${RING_PREFIX}LIMBS_shl_mod
#define LIMBS_sub_mod ''${RING_PREFIX}LIMBS_sub_mod
#define LIMBS_window5_split_window ''${RING_PREFIX}LIMBS_window5_split_window
#define LIMBS_window5_unsplit_window ''${RING_PREFIX}LIMBS_window5_unsplit_window
#define LIMB_shr ''${RING_PREFIX}LIMB_shr
#define OPENSSL_cpuid_setup ''${RING_PREFIX}OPENSSL_cpuid_setup
#define aes_gcm_dec_kernel ''${RING_PREFIX}aes_gcm_dec_kernel
#define aes_gcm_dec_update_vaes_avx2 ''${RING_PREFIX}aes_gcm_dec_update_vaes_avx2
#define aes_gcm_enc_kernel ''${RING_PREFIX}aes_gcm_enc_kernel
#define aes_gcm_enc_update_vaes_avx2 ''${RING_PREFIX}aes_gcm_enc_update_vaes_avx2
#define aes_hw_ctr32_encrypt_blocks ''${RING_PREFIX}aes_hw_ctr32_encrypt_blocks
#define aes_hw_set_encrypt_key ''${RING_PREFIX}aes_hw_set_encrypt_key
#define aes_hw_set_encrypt_key_alt ''${RING_PREFIX}aes_hw_set_encrypt_key_alt
#define aes_hw_set_encrypt_key_base ''${RING_PREFIX}aes_hw_set_encrypt_key_base
#define aes_nohw_ctr32_encrypt_blocks ''${RING_PREFIX}aes_nohw_ctr32_encrypt_blocks
#define aes_nohw_encrypt ''${RING_PREFIX}aes_nohw_encrypt
#define aes_nohw_set_encrypt_key ''${RING_PREFIX}aes_nohw_set_encrypt_key
#define aesni_gcm_decrypt ''${RING_PREFIX}aesni_gcm_decrypt
#define aesni_gcm_encrypt ''${RING_PREFIX}aesni_gcm_encrypt
#define bn_from_montgomery_in_place ''${RING_PREFIX}bn_from_montgomery_in_place
#define bn_gather5 ''${RING_PREFIX}bn_gather5
#define bn_mul_mont ''${RING_PREFIX}bn_mul_mont
#define bn_mul_mont_nohw ''${RING_PREFIX}bn_mul_mont_nohw
#define bn_mul4x_mont ''${RING_PREFIX}bn_mul4x_mont
#define bn_mulx4x_mont ''${RING_PREFIX}bn_mulx4x_mont
#define bn_mul8x_mont_neon ''${RING_PREFIX}bn_mul8x_mont_neon
#define bn_mul4x_mont_gather5 ''${RING_PREFIX}bn_mul4x_mont_gather5
#define bn_mulx4x_mont_gather5 ''${RING_PREFIX}bn_mulx4x_mont_gather5
#define bn_neg_inv_mod_r_u64 ''${RING_PREFIX}bn_neg_inv_mod_r_u64
#define bn_power5_nohw ''${RING_PREFIX}bn_power5_nohw
#define bn_powerx5 ''${RING_PREFIX}bn_powerx5
#define bn_scatter5 ''${RING_PREFIX}bn_scatter5
#define bn_sqr8x_internal ''${RING_PREFIX}bn_sqr8x_internal
#define bn_sqr8x_mont ''${RING_PREFIX}bn_sqr8x_mont
#define bn_sqrx8x_internal ''${RING_PREFIX}bn_sqrx8x_internal
#define bsaes_ctr32_encrypt_blocks ''${RING_PREFIX}bsaes_ctr32_encrypt_blocks
#define bssl_constant_time_test_conditional_memcpy ''${RING_PREFIX}bssl_constant_time_test_conditional_memcpy
#define bssl_constant_time_test_conditional_memxor ''${RING_PREFIX}bssl_constant_time_test_conditional_memxor
#define bssl_constant_time_test_main ''${RING_PREFIX}bssl_constant_time_test_main
#define chacha20_poly1305_open ''${RING_PREFIX}chacha20_poly1305_open
#define chacha20_poly1305_open_avx2 ''${RING_PREFIX}chacha20_poly1305_open_avx2
#define chacha20_poly1305_open_sse41 ''${RING_PREFIX}chacha20_poly1305_open_sse41
#define chacha20_poly1305_seal ''${RING_PREFIX}chacha20_poly1305_seal
#define chacha20_poly1305_seal_avx2 ''${RING_PREFIX}chacha20_poly1305_seal_avx2
#define chacha20_poly1305_seal_sse41 ''${RING_PREFIX}chacha20_poly1305_seal_sse41
#define ecp_nistz256_mul_mont_adx ''${RING_PREFIX}ecp_nistz256_mul_mont_adx
#define ecp_nistz256_mul_mont_nohw ''${RING_PREFIX}ecp_nistz256_mul_mont_nohw
#define ecp_nistz256_ord_mul_mont_adx ''${RING_PREFIX}ecp_nistz256_ord_mul_mont_adx
#define ecp_nistz256_ord_mul_mont_nohw ''${RING_PREFIX}ecp_nistz256_ord_mul_mont_nohw
#define ecp_nistz256_ord_sqr_mont_adx ''${RING_PREFIX}ecp_nistz256_ord_sqr_mont_adx
#define ecp_nistz256_ord_sqr_mont_nohw ''${RING_PREFIX}ecp_nistz256_ord_sqr_mont_nohw
#define ecp_nistz256_point_add_adx ''${RING_PREFIX}ecp_nistz256_point_add_adx
#define ecp_nistz256_point_add_nohw ''${RING_PREFIX}ecp_nistz256_point_add_nohw
#define ecp_nistz256_point_add_affine_adx ''${RING_PREFIX}ecp_nistz256_point_add_affine_adx
#define ecp_nistz256_point_add_affine_nohw ''${RING_PREFIX}ecp_nistz256_point_add_affine_nohw
#define ecp_nistz256_point_double_adx ''${RING_PREFIX}ecp_nistz256_point_double_adx
#define ecp_nistz256_point_double_nohw ''${RING_PREFIX}ecp_nistz256_point_double_nohw
#define ecp_nistz256_select_w5_avx2 ''${RING_PREFIX}ecp_nistz256_select_w5_avx2
#define ecp_nistz256_select_w5_nohw ''${RING_PREFIX}ecp_nistz256_select_w5_nohw
#define ecp_nistz256_select_w7_avx2 ''${RING_PREFIX}ecp_nistz256_select_w7_avx2
#define ecp_nistz256_select_w7_nohw ''${RING_PREFIX}ecp_nistz256_select_w7_nohw
#define ecp_nistz256_sqr_mont_adx ''${RING_PREFIX}ecp_nistz256_sqr_mont_adx
#define ecp_nistz256_sqr_mont_nohw ''${RING_PREFIX}ecp_nistz256_sqr_mont_nohw
#define fiat_curve25519_adx_mul ''${RING_PREFIX}fiat_curve25519_adx_mul
#define fiat_curve25519_adx_square ''${RING_PREFIX}fiat_curve25519_adx_square
#define gcm_ghash_avx ''${RING_PREFIX}gcm_ghash_avx
#define gcm_ghash_clmul ''${RING_PREFIX}gcm_ghash_clmul
#define gcm_ghash_neon ''${RING_PREFIX}gcm_ghash_neon
#define gcm_ghash_vpclmulqdq_avx2_1 ''${RING_PREFIX}gcm_ghash_vpclmulqdq_avx2_1
#define gcm_gmult_clmul ''${RING_PREFIX}gcm_gmult_clmul
#define gcm_gmult_neon ''${RING_PREFIX}gcm_gmult_neon
#define gcm_init_avx ''${RING_PREFIX}gcm_init_avx
#define gcm_init_clmul ''${RING_PREFIX}gcm_init_clmul
#define gcm_init_neon ''${RING_PREFIX}gcm_init_neon
#define gcm_init_nohw ''${RING_PREFIX}gcm_init_nohw
#define gcm_ghash_nohw ''${RING_PREFIX}gcm_ghash_nohw
#define gcm_init_vpclmulqdq_avx2 ''${RING_PREFIX}gcm_init_vpclmulqdq_avx2
#define k25519Precomp ''${RING_PREFIX}k25519Precomp
#define limbs_mul_add_limb ''${RING_PREFIX}limbs_mul_add_limb
#define little_endian_bytes_from_scalar ''${RING_PREFIX}little_endian_bytes_from_scalar
#define ecp_nistz256_neg ''${RING_PREFIX}ecp_nistz256_neg
#define ecp_nistz256_select_w5 ''${RING_PREFIX}ecp_nistz256_select_w5
#define ecp_nistz256_select_w7 ''${RING_PREFIX}ecp_nistz256_select_w7
#define neon_available ''${RING_PREFIX}neon_available
#define p256_mul_mont ''${RING_PREFIX}p256_mul_mont
#define p256_point_add ''${RING_PREFIX}p256_point_add
#define p256_point_add_affine ''${RING_PREFIX}p256_point_add_affine
#define p256_point_double ''${RING_PREFIX}p256_point_double
#define p256_point_mul ''${RING_PREFIX}p256_point_mul
#define p256_point_mul_base ''${RING_PREFIX}p256_point_mul_base
#define p256_point_mul_base_vartime ''${RING_PREFIX}p256_point_mul_base_vartime
#define p256_scalar_mul_mont ''${RING_PREFIX}p256_scalar_mul_mont
#define p256_scalar_sqr_rep_mont ''${RING_PREFIX}p256_scalar_sqr_rep_mont
#define p256_sqr_mont ''${RING_PREFIX}p256_sqr_mont
#define p384_elem_div_by_2 ''${RING_PREFIX}p384_elem_div_by_2
#define p384_elem_mul_mont ''${RING_PREFIX}p384_elem_mul_mont
#define p384_elem_neg ''${RING_PREFIX}p384_elem_neg
#define p384_elem_sub ''${RING_PREFIX}p384_elem_sub
#define p384_point_add ''${RING_PREFIX}p384_point_add
#define p384_point_double ''${RING_PREFIX}p384_point_double
#define p384_point_mul ''${RING_PREFIX}p384_point_mul
#define p384_scalar_mul_mont ''${RING_PREFIX}p384_scalar_mul_mont
#define openssl_poly1305_neon2_addmulmod ''${RING_PREFIX}openssl_poly1305_neon2_addmulmod
#define openssl_poly1305_neon2_blocks ''${RING_PREFIX}openssl_poly1305_neon2_blocks
#define sha256_block_data_order ''${RING_PREFIX}sha256_block_data_order
#define sha256_block_data_order_avx ''${RING_PREFIX}sha256_block_data_order_avx
#define sha256_block_data_order_ssse3 ''${RING_PREFIX}sha256_block_data_order_ssse3
#define sha256_block_data_order_hw ''${RING_PREFIX}sha256_block_data_order_hw
#define sha256_block_data_order_neon ''${RING_PREFIX}sha256_block_data_order_neon
#define sha256_block_data_order_nohw ''${RING_PREFIX}sha256_block_data_order_nohw
#define sha256_block_data_order_avx2 ''${RING_PREFIX}sha256_block_data_order_avx2
#define sha512_block_data_order ''${RING_PREFIX}sha512_block_data_order
#define sha512_block_data_order_avx ''${RING_PREFIX}sha512_block_data_order_avx
#define sha512_block_data_order_hw ''${RING_PREFIX}sha512_block_data_order_hw
#define sha512_block_data_order_neon ''${RING_PREFIX}sha512_block_data_order_neon
#define sha512_block_data_order_nohw ''${RING_PREFIX}sha512_block_data_order_nohw
#define sha512_block_data_order_avx2 ''${RING_PREFIX}sha512_block_data_order_avx2
#define vpaes_ctr32_encrypt_blocks ''${RING_PREFIX}vpaes_ctr32_encrypt_blocks
#define vpaes_encrypt ''${RING_PREFIX}vpaes_encrypt
#define vpaes_encrypt_key_to_bsaes ''${RING_PREFIX}vpaes_encrypt_key_to_bsaes
#define vpaes_set_encrypt_key ''${RING_PREFIX}vpaes_set_encrypt_key
#define x25519_NEON ''${RING_PREFIX}x25519_NEON
#define x25519_fe_invert ''${RING_PREFIX}x25519_fe_invert
#define x25519_fe_isnegative ''${RING_PREFIX}x25519_fe_isnegative
#define x25519_fe_mul_ttt ''${RING_PREFIX}x25519_fe_mul_ttt
#define x25519_fe_neg ''${RING_PREFIX}x25519_fe_neg
#define x25519_fe_tobytes ''${RING_PREFIX}x25519_fe_tobytes
#define x25519_ge_double_scalarmult_vartime ''${RING_PREFIX}x25519_ge_double_scalarmult_vartime
#define x25519_ge_frombytes_vartime ''${RING_PREFIX}x25519_ge_frombytes_vartime
#define x25519_ge_scalarmult_base ''${RING_PREFIX}x25519_ge_scalarmult_base
#define x25519_ge_scalarmult_base_adx ''${RING_PREFIX}x25519_ge_scalarmult_base_adx
#define x25519_public_from_private_generic_masked ''${RING_PREFIX}x25519_public_from_private_generic_masked
#define x25519_sc_mask ''${RING_PREFIX}x25519_sc_mask
#define x25519_sc_muladd ''${RING_PREFIX}x25519_sc_muladd
#define x25519_sc_reduce ''${RING_PREFIX}x25519_sc_reduce
#define x25519_scalar_mult_adx ''${RING_PREFIX}x25519_scalar_mult_adx
#define x25519_scalar_mult_generic_masked ''${RING_PREFIX}x25519_scalar_mult_generic_masked
#define OPENSSL_ia32cap_P ''${RING_PREFIX}OPENSSL_ia32cap_P

#endif
RING_PREFIX_HEADER

      # Generate assembly prefix header (same symbols as above, for .S files)
      # Uses the same symbol list but in assembly-compatible format
      cat > "$RING_OUT/ring_core_generated/prefix_symbols_asm.h" << RING_ASM_PREFIX_HEADER
#ifndef ring_core_generated_PREFIX_SYMBOLS_ASM_H
#define ring_core_generated_PREFIX_SYMBOLS_ASM_H

// Symbol renames (from SYMBOLS_TO_RENAME in build.rs)
#define ecp_nistz256_point_double p256_point_double
#define ecp_nistz256_point_add p256_point_add
#define ecp_nistz256_point_add_affine p256_point_add_affine
#define ecp_nistz256_ord_mul_mont p256_scalar_mul_mont
#define ecp_nistz256_ord_sqr_mont p256_scalar_sqr_rep_mont
#define ecp_nistz256_mul_mont p256_mul_mont
#define ecp_nistz256_sqr_mont p256_sqr_mont

// All symbols from SYMBOLS_TO_PREFIX in build.rs
#define adx_bmi2_available ''${RING_PREFIX}adx_bmi2_available
#define avx2_available ''${RING_PREFIX}avx2_available
#define CRYPTO_memcmp ''${RING_PREFIX}CRYPTO_memcmp
#define CRYPTO_poly1305_finish ''${RING_PREFIX}CRYPTO_poly1305_finish
#define CRYPTO_poly1305_finish_neon ''${RING_PREFIX}CRYPTO_poly1305_finish_neon
#define CRYPTO_poly1305_init ''${RING_PREFIX}CRYPTO_poly1305_init
#define CRYPTO_poly1305_init_neon ''${RING_PREFIX}CRYPTO_poly1305_init_neon
#define CRYPTO_poly1305_update ''${RING_PREFIX}CRYPTO_poly1305_update
#define CRYPTO_poly1305_update_neon ''${RING_PREFIX}CRYPTO_poly1305_update_neon
#define ChaCha20_ctr32 ''${RING_PREFIX}ChaCha20_ctr32
#define ChaCha20_ctr32_avx2 ''${RING_PREFIX}ChaCha20_ctr32_avx2
#define ChaCha20_ctr32_neon ''${RING_PREFIX}ChaCha20_ctr32_neon
#define ChaCha20_ctr32_nohw ''${RING_PREFIX}ChaCha20_ctr32_nohw
#define ChaCha20_ctr32_ssse3 ''${RING_PREFIX}ChaCha20_ctr32_ssse3
#define ChaCha20_ctr32_ssse3_4x ''${RING_PREFIX}ChaCha20_ctr32_ssse3_4x
#define LIMB_is_zero ''${RING_PREFIX}LIMB_is_zero
#define LIMBS_add_mod ''${RING_PREFIX}LIMBS_add_mod
#define LIMBS_are_zero ''${RING_PREFIX}LIMBS_are_zero
#define LIMBS_equal ''${RING_PREFIX}LIMBS_equal
#define LIMBS_less_than ''${RING_PREFIX}LIMBS_less_than
#define LIMBS_reduce_once ''${RING_PREFIX}LIMBS_reduce_once
#define LIMBS_select_512_32 ''${RING_PREFIX}LIMBS_select_512_32
#define LIMBS_shl_mod ''${RING_PREFIX}LIMBS_shl_mod
#define LIMBS_sub_mod ''${RING_PREFIX}LIMBS_sub_mod
#define LIMBS_window5_split_window ''${RING_PREFIX}LIMBS_window5_split_window
#define LIMBS_window5_unsplit_window ''${RING_PREFIX}LIMBS_window5_unsplit_window
#define LIMB_shr ''${RING_PREFIX}LIMB_shr
#define OPENSSL_cpuid_setup ''${RING_PREFIX}OPENSSL_cpuid_setup
#define aes_gcm_dec_kernel ''${RING_PREFIX}aes_gcm_dec_kernel
#define aes_gcm_dec_update_vaes_avx2 ''${RING_PREFIX}aes_gcm_dec_update_vaes_avx2
#define aes_gcm_enc_kernel ''${RING_PREFIX}aes_gcm_enc_kernel
#define aes_gcm_enc_update_vaes_avx2 ''${RING_PREFIX}aes_gcm_enc_update_vaes_avx2
#define aes_hw_ctr32_encrypt_blocks ''${RING_PREFIX}aes_hw_ctr32_encrypt_blocks
#define aes_hw_set_encrypt_key ''${RING_PREFIX}aes_hw_set_encrypt_key
#define aes_hw_set_encrypt_key_alt ''${RING_PREFIX}aes_hw_set_encrypt_key_alt
#define aes_hw_set_encrypt_key_base ''${RING_PREFIX}aes_hw_set_encrypt_key_base
#define aes_nohw_ctr32_encrypt_blocks ''${RING_PREFIX}aes_nohw_ctr32_encrypt_blocks
#define aes_nohw_encrypt ''${RING_PREFIX}aes_nohw_encrypt
#define aes_nohw_set_encrypt_key ''${RING_PREFIX}aes_nohw_set_encrypt_key
#define aesni_gcm_decrypt ''${RING_PREFIX}aesni_gcm_decrypt
#define aesni_gcm_encrypt ''${RING_PREFIX}aesni_gcm_encrypt
#define bn_from_montgomery_in_place ''${RING_PREFIX}bn_from_montgomery_in_place
#define bn_gather5 ''${RING_PREFIX}bn_gather5
#define bn_mul_mont ''${RING_PREFIX}bn_mul_mont
#define bn_mul_mont_nohw ''${RING_PREFIX}bn_mul_mont_nohw
#define bn_mul4x_mont ''${RING_PREFIX}bn_mul4x_mont
#define bn_mulx4x_mont ''${RING_PREFIX}bn_mulx4x_mont
#define bn_mul8x_mont_neon ''${RING_PREFIX}bn_mul8x_mont_neon
#define bn_mul4x_mont_gather5 ''${RING_PREFIX}bn_mul4x_mont_gather5
#define bn_mulx4x_mont_gather5 ''${RING_PREFIX}bn_mulx4x_mont_gather5
#define bn_neg_inv_mod_r_u64 ''${RING_PREFIX}bn_neg_inv_mod_r_u64
#define bn_power5_nohw ''${RING_PREFIX}bn_power5_nohw
#define bn_powerx5 ''${RING_PREFIX}bn_powerx5
#define bn_scatter5 ''${RING_PREFIX}bn_scatter5
#define bn_sqr8x_internal ''${RING_PREFIX}bn_sqr8x_internal
#define bn_sqr8x_mont ''${RING_PREFIX}bn_sqr8x_mont
#define bn_sqrx8x_internal ''${RING_PREFIX}bn_sqrx8x_internal
#define bsaes_ctr32_encrypt_blocks ''${RING_PREFIX}bsaes_ctr32_encrypt_blocks
#define bssl_constant_time_test_conditional_memcpy ''${RING_PREFIX}bssl_constant_time_test_conditional_memcpy
#define bssl_constant_time_test_conditional_memxor ''${RING_PREFIX}bssl_constant_time_test_conditional_memxor
#define bssl_constant_time_test_main ''${RING_PREFIX}bssl_constant_time_test_main
#define chacha20_poly1305_open ''${RING_PREFIX}chacha20_poly1305_open
#define chacha20_poly1305_open_avx2 ''${RING_PREFIX}chacha20_poly1305_open_avx2
#define chacha20_poly1305_open_sse41 ''${RING_PREFIX}chacha20_poly1305_open_sse41
#define chacha20_poly1305_seal ''${RING_PREFIX}chacha20_poly1305_seal
#define chacha20_poly1305_seal_avx2 ''${RING_PREFIX}chacha20_poly1305_seal_avx2
#define chacha20_poly1305_seal_sse41 ''${RING_PREFIX}chacha20_poly1305_seal_sse41
#define ecp_nistz256_mul_mont_adx ''${RING_PREFIX}ecp_nistz256_mul_mont_adx
#define ecp_nistz256_mul_mont_nohw ''${RING_PREFIX}ecp_nistz256_mul_mont_nohw
#define ecp_nistz256_ord_mul_mont_adx ''${RING_PREFIX}ecp_nistz256_ord_mul_mont_adx
#define ecp_nistz256_ord_mul_mont_nohw ''${RING_PREFIX}ecp_nistz256_ord_mul_mont_nohw
#define ecp_nistz256_ord_sqr_mont_adx ''${RING_PREFIX}ecp_nistz256_ord_sqr_mont_adx
#define ecp_nistz256_ord_sqr_mont_nohw ''${RING_PREFIX}ecp_nistz256_ord_sqr_mont_nohw
#define ecp_nistz256_point_add_adx ''${RING_PREFIX}ecp_nistz256_point_add_adx
#define ecp_nistz256_point_add_nohw ''${RING_PREFIX}ecp_nistz256_point_add_nohw
#define ecp_nistz256_point_add_affine_adx ''${RING_PREFIX}ecp_nistz256_point_add_affine_adx
#define ecp_nistz256_point_add_affine_nohw ''${RING_PREFIX}ecp_nistz256_point_add_affine_nohw
#define ecp_nistz256_point_double_adx ''${RING_PREFIX}ecp_nistz256_point_double_adx
#define ecp_nistz256_point_double_nohw ''${RING_PREFIX}ecp_nistz256_point_double_nohw
#define ecp_nistz256_select_w5_avx2 ''${RING_PREFIX}ecp_nistz256_select_w5_avx2
#define ecp_nistz256_select_w5_nohw ''${RING_PREFIX}ecp_nistz256_select_w5_nohw
#define ecp_nistz256_select_w7_avx2 ''${RING_PREFIX}ecp_nistz256_select_w7_avx2
#define ecp_nistz256_select_w7_nohw ''${RING_PREFIX}ecp_nistz256_select_w7_nohw
#define ecp_nistz256_sqr_mont_adx ''${RING_PREFIX}ecp_nistz256_sqr_mont_adx
#define ecp_nistz256_sqr_mont_nohw ''${RING_PREFIX}ecp_nistz256_sqr_mont_nohw
#define fiat_curve25519_adx_mul ''${RING_PREFIX}fiat_curve25519_adx_mul
#define fiat_curve25519_adx_square ''${RING_PREFIX}fiat_curve25519_adx_square
#define gcm_ghash_avx ''${RING_PREFIX}gcm_ghash_avx
#define gcm_ghash_clmul ''${RING_PREFIX}gcm_ghash_clmul
#define gcm_ghash_neon ''${RING_PREFIX}gcm_ghash_neon
#define gcm_ghash_vpclmulqdq_avx2_1 ''${RING_PREFIX}gcm_ghash_vpclmulqdq_avx2_1
#define gcm_gmult_clmul ''${RING_PREFIX}gcm_gmult_clmul
#define gcm_gmult_neon ''${RING_PREFIX}gcm_gmult_neon
#define gcm_init_avx ''${RING_PREFIX}gcm_init_avx
#define gcm_init_clmul ''${RING_PREFIX}gcm_init_clmul
#define gcm_init_neon ''${RING_PREFIX}gcm_init_neon
#define gcm_init_nohw ''${RING_PREFIX}gcm_init_nohw
#define gcm_ghash_nohw ''${RING_PREFIX}gcm_ghash_nohw
#define gcm_init_vpclmulqdq_avx2 ''${RING_PREFIX}gcm_init_vpclmulqdq_avx2
#define k25519Precomp ''${RING_PREFIX}k25519Precomp
#define limbs_mul_add_limb ''${RING_PREFIX}limbs_mul_add_limb
#define little_endian_bytes_from_scalar ''${RING_PREFIX}little_endian_bytes_from_scalar
#define ecp_nistz256_neg ''${RING_PREFIX}ecp_nistz256_neg
#define ecp_nistz256_select_w5 ''${RING_PREFIX}ecp_nistz256_select_w5
#define ecp_nistz256_select_w7 ''${RING_PREFIX}ecp_nistz256_select_w7
#define neon_available ''${RING_PREFIX}neon_available
#define p256_mul_mont ''${RING_PREFIX}p256_mul_mont
#define p256_point_add ''${RING_PREFIX}p256_point_add
#define p256_point_add_affine ''${RING_PREFIX}p256_point_add_affine
#define p256_point_double ''${RING_PREFIX}p256_point_double
#define p256_point_mul ''${RING_PREFIX}p256_point_mul
#define p256_point_mul_base ''${RING_PREFIX}p256_point_mul_base
#define p256_point_mul_base_vartime ''${RING_PREFIX}p256_point_mul_base_vartime
#define p256_scalar_mul_mont ''${RING_PREFIX}p256_scalar_mul_mont
#define p256_scalar_sqr_rep_mont ''${RING_PREFIX}p256_scalar_sqr_rep_mont
#define p256_sqr_mont ''${RING_PREFIX}p256_sqr_mont
#define p384_elem_div_by_2 ''${RING_PREFIX}p384_elem_div_by_2
#define p384_elem_mul_mont ''${RING_PREFIX}p384_elem_mul_mont
#define p384_elem_neg ''${RING_PREFIX}p384_elem_neg
#define p384_elem_sub ''${RING_PREFIX}p384_elem_sub
#define p384_point_add ''${RING_PREFIX}p384_point_add
#define p384_point_double ''${RING_PREFIX}p384_point_double
#define p384_point_mul ''${RING_PREFIX}p384_point_mul
#define p384_scalar_mul_mont ''${RING_PREFIX}p384_scalar_mul_mont
#define openssl_poly1305_neon2_addmulmod ''${RING_PREFIX}openssl_poly1305_neon2_addmulmod
#define openssl_poly1305_neon2_blocks ''${RING_PREFIX}openssl_poly1305_neon2_blocks
#define sha256_block_data_order ''${RING_PREFIX}sha256_block_data_order
#define sha256_block_data_order_avx ''${RING_PREFIX}sha256_block_data_order_avx
#define sha256_block_data_order_ssse3 ''${RING_PREFIX}sha256_block_data_order_ssse3
#define sha256_block_data_order_hw ''${RING_PREFIX}sha256_block_data_order_hw
#define sha256_block_data_order_neon ''${RING_PREFIX}sha256_block_data_order_neon
#define sha256_block_data_order_nohw ''${RING_PREFIX}sha256_block_data_order_nohw
#define sha256_block_data_order_avx2 ''${RING_PREFIX}sha256_block_data_order_avx2
#define sha512_block_data_order ''${RING_PREFIX}sha512_block_data_order
#define sha512_block_data_order_avx ''${RING_PREFIX}sha512_block_data_order_avx
#define sha512_block_data_order_hw ''${RING_PREFIX}sha512_block_data_order_hw
#define sha512_block_data_order_neon ''${RING_PREFIX}sha512_block_data_order_neon
#define sha512_block_data_order_nohw ''${RING_PREFIX}sha512_block_data_order_nohw
#define sha512_block_data_order_avx2 ''${RING_PREFIX}sha512_block_data_order_avx2
#define vpaes_ctr32_encrypt_blocks ''${RING_PREFIX}vpaes_ctr32_encrypt_blocks
#define vpaes_encrypt ''${RING_PREFIX}vpaes_encrypt
#define vpaes_encrypt_key_to_bsaes ''${RING_PREFIX}vpaes_encrypt_key_to_bsaes
#define vpaes_set_encrypt_key ''${RING_PREFIX}vpaes_set_encrypt_key
#define x25519_NEON ''${RING_PREFIX}x25519_NEON
#define x25519_fe_invert ''${RING_PREFIX}x25519_fe_invert
#define x25519_fe_isnegative ''${RING_PREFIX}x25519_fe_isnegative
#define x25519_fe_mul_ttt ''${RING_PREFIX}x25519_fe_mul_ttt
#define x25519_fe_neg ''${RING_PREFIX}x25519_fe_neg
#define x25519_fe_tobytes ''${RING_PREFIX}x25519_fe_tobytes
#define x25519_ge_double_scalarmult_vartime ''${RING_PREFIX}x25519_ge_double_scalarmult_vartime
#define x25519_ge_frombytes_vartime ''${RING_PREFIX}x25519_ge_frombytes_vartime
#define x25519_ge_scalarmult_base ''${RING_PREFIX}x25519_ge_scalarmult_base
#define x25519_ge_scalarmult_base_adx ''${RING_PREFIX}x25519_ge_scalarmult_base_adx
#define x25519_public_from_private_generic_masked ''${RING_PREFIX}x25519_public_from_private_generic_masked
#define x25519_sc_mask ''${RING_PREFIX}x25519_sc_mask
#define x25519_sc_muladd ''${RING_PREFIX}x25519_sc_muladd
#define x25519_sc_reduce ''${RING_PREFIX}x25519_sc_reduce
#define x25519_scalar_mult_adx ''${RING_PREFIX}x25519_scalar_mult_adx
#define x25519_scalar_mult_generic_masked ''${RING_PREFIX}x25519_scalar_mult_generic_masked
#define OPENSSL_ia32cap_P ''${RING_PREFIX}OPENSSL_ia32cap_P

#endif
RING_ASM_PREFIX_HEADER

      # Compiler flags matching ring's build.rs
      # Include paths: ring's include dir AND out_dir (for generated headers)
      RING_CFLAGS="-fvisibility=hidden -std=c1x -pedantic -Wall -I$RING_SRC/include -I$RING_OUT"

      # C source files to compile (x86_64-linux)
      RING_C_SRCS=(
        crypto/curve25519/curve25519.c
        crypto/fipsmodule/aes/aes_nohw.c
        crypto/fipsmodule/bn/montgomery.c
        crypto/fipsmodule/bn/montgomery_inv.c
        crypto/fipsmodule/ec/ecp_nistz.c
        crypto/fipsmodule/ec/gfp_p256.c
        crypto/fipsmodule/ec/gfp_p384.c
        crypto/fipsmodule/ec/p256.c
        crypto/fipsmodule/ec/p256-nistz.c
        crypto/limbs/limbs.c
        crypto/mem.c
        crypto/poly1305/poly1305.c
        crypto/crypto.c
        crypto/cpu_intel.c
        crypto/curve25519/curve25519_64_adx.c
      )

      # Compile C files
      RING_OBJS=()
      for src in "''${RING_C_SRCS[@]}"; do
        if [ -f "$RING_SRC/$src" ]; then
          obj="$RING_OUT/$(basename $src .c).o"
          cc -c $RING_CFLAGS -o "$obj" "$RING_SRC/$src"
          RING_OBJS+=("$obj")
        fi
      done

      # Assembly files from pregenerated/ directory (x86_64 ELF format)
      RING_ASM_SRCS=(
        pregenerated/chacha-x86_64-elf.S
        pregenerated/aesni-gcm-x86_64-elf.S
        pregenerated/aesni-x86_64-elf.S
        pregenerated/ghash-x86_64-elf.S
        pregenerated/vpaes-x86_64-elf.S
        pregenerated/x86_64-mont-elf.S
        pregenerated/x86_64-mont5-elf.S
        pregenerated/p256-x86_64-asm-elf.S
        pregenerated/sha256-x86_64-elf.S
        pregenerated/sha512-x86_64-elf.S
        pregenerated/chacha20_poly1305_x86_64-elf.S
        pregenerated/aes-gcm-avx2-x86_64-elf.S
        third_party/fiat/asm/fiat_curve25519_adx_mul.S
        third_party/fiat/asm/fiat_curve25519_adx_square.S
      )

      # Assemble .S files (also need include paths for ring-core headers)
      for src in "''${RING_ASM_SRCS[@]}"; do
        if [ -f "$RING_SRC/$src" ]; then
          obj="$RING_OUT/$(basename $src .S).o"
          cc -c -I$RING_SRC/include -I$RING_OUT -o "$obj" "$RING_SRC/$src"
          RING_OBJS+=("$obj")
        fi
      done

      # Create static library
      ar rcs "$RING_OUT/lib''${RING_PREFIX%.}.a" "''${RING_OBJS[@]}"
      echo "Built ring native library: $RING_OUT/lib''${RING_PREFIX%.}.a"
    '';
  };

  # Merge user-provided build script fixups with defaults (user takes precedence)
  mergedBuildScriptFixups = defaultBuildScriptFixups // buildScriptFixups;

  # Import semver utilities
  semver = import ../lib/semver.nix { inherit lib; };

  # Parse the TOML file
  depsToml = builtins.fromTOML (builtins.readFile depsFile);

  # Convert TOML deps to registry format
  # Key is "name@version", value contains name, version, hash
  registry = lib.mapAttrs (key: dep: {
    # Use explicit name field, fallback to parsing key for backwards compat
    crateName = dep.name or (lib.head (lib.splitString "@" key));
    inherit (dep) version;
    features = dep.features or [];
    src = fetchCrate (dep.name or (lib.head (lib.splitString "@" key))) dep;
  }) (depsToml.deps or {});

  # Fetch crate from crates.io
  fetchCrate = crateName: dep:
    pkgs.fetchzip {
      url = "https://crates.io/api/v1/crates/${crateName}/${dep.version}/download";
      sha256 = dep.hash;
      extension = "tar.gz";
    };

  # Scripts for BUCK file generation
  genBuckScript = ./gen-rust-buck.py;
  computeFeaturesScript = ./compute-unified-features.py;

  # JSON list of all available crate names for dependency resolution
  # Includes both versioned keys (e.g., "getrandom@0.2.17") and unversioned names
  # This allows version-aware dependency resolution
  availableCratesJson = builtins.toJSON (
    (lib.attrNames cratesByName) ++  # unversioned names for symlinks
    (lib.attrNames registry)         # versioned keys for precise matching
  );

  # JSON registry of rustc flags for crates with build script cfg directives
  # Passed to gen-rust-buck.py for BUCK file generation
  rustcFlagsRegistryJson = builtins.toJSON mergedRustcFlagsRegistry;

  # ==========================================================================
  # Build script fixups
  # ==========================================================================
  # Some crates have build scripts that generate files. We pre-generate these
  # in Nix to avoid needing to run build scripts at Buck2 build time.

  # Generate fixup commands for a specific crate
  # Looks up in mergedBuildScriptFixups (version-specific key first, then crate name)
  # Returns empty string if no fixup needed
  getFixupCommands = key: dep:
    let
      crateName = dep.crateName;
      version = dep.version;
      patchVersion = lib.last (lib.splitString "." version);
      vendorPath = "vendor/${key}";

      # Context passed to fixup functions
      fixupContext = { inherit crateName version patchVersion key vendorPath; };

      # Look up fixup: try versioned key first, then crate name
      fixup = mergedBuildScriptFixups.${key} or mergedBuildScriptFixups.${crateName} or null;

      # If fixup is a function, call it with context; otherwise use as-is
      resolvedFixup =
        if fixup == null then null
        else if builtins.isFunction fixup then fixup fixupContext
        else fixup;
    in
    if resolvedFixup != null then resolvedFixup
    else "";

  # Check if a crate needs build script fixups
  # Uses mergedBuildScriptFixups keys (supports version-specific and catch-all)
  needsFixup = crateName:
    lib.hasAttr crateName mergedBuildScriptFixups;

  # JSON map of crates that need OUT_DIR set (for gen-rust-buck.py)
  # Derived from the merged fixups registry keys
  fixupCratesJson = builtins.toJSON (lib.attrNames mergedBuildScriptFixups);

  # ==========================================================================
  # Crate setup (Phase 1: copy sources and apply fixups)
  # ==========================================================================

  # Generate shell commands to set up one crate's sources (no BUCK file yet)
  setupCrateSources = key: dep:
    let
      vendorPath = "vendor/${key}";
      fixupCmds = getFixupCommands key dep;
    in
    ''
      # Set up ${key}
      mkdir -p $out/${vendorPath}
      cp -r ${dep.src}/* $out/${vendorPath}/
      chmod -R u+w $out/${vendorPath}

      # Apply fixups (if any)
      ${fixupCmds}
    '';

  # All source setup commands
  allSourceSetupCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList setupCrateSources registry
  );

  # ==========================================================================
  # BUCK file generation (Phase 2: after feature unification)
  # ==========================================================================

  # Generate BUCK file for one crate using unified features
  generateBuckFile = key: dep:
    let
      vendorPath = "vendor/${key}";
    in
    ''
      # Generate BUCK file for ${key}
      ${pkgs.python3}/bin/python3 ${genBuckScript} \
        "$out/${vendorPath}" \
        '${availableCratesJson}' \
        '${fixupCratesJson}' \
        "$UNIFIED_FEATURES" \
        '${rustcFlagsRegistryJson}' \
        > "$out/${vendorPath}/BUCK"
    '';

  # All BUCK generation commands
  allBuckGenCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList generateBuckFile registry
  );

  # ==========================================================================
  # Symlink generation with proper version selection
  # ==========================================================================

  # Group crates by unversioned name to create symlinks
  # This allows users to reference crates without version suffix
  cratesByName = lib.foldlAttrs (acc: key: dep:
    let
      name = dep.crateName;
      existing = acc.${name} or [];
    in
    acc // { ${name} = existing ++ [{ inherit key; version = dep.version; }]; }
  ) {} registry;

  # Generate symlink commands for unversioned references
  # When multiple versions exist, sort by semver and pick the greatest
  symlinkCommands = lib.concatStringsSep "\n" (
    lib.mapAttrsToList (name: versions:
      let
        # Sort versions by semver descending (greatest first)
        sorted = lib.sort semver.sortDesc versions;
        # Pick the greatest version
        target = (lib.head sorted).key;
      in
      ''
        # Symlink ${name} -> ${target}
        ln -s "${target}" "$out/vendor/${name}"
      ''
    ) cratesByName
  );

  # Optional features file handling
  featuresFileArg = if featuresFile != null && builtins.pathExists featuresFile
    then "${featuresFile}"
    else "";

in
pkgs.runCommand "rust-deps-cell" {
  nativeBuildInputs = buildTools;
} ''
  mkdir -p $out/vendor

  # ==========================================================================
  # Phase 1: Set up all crate sources and apply fixups
  # ==========================================================================
  ${allSourceSetupCommands}

  # ==========================================================================
  # Phase 2: Compute unified features across all crates
  # ==========================================================================
  echo "Computing unified features..."
  UNIFIED_FEATURES=$(${pkgs.python3}/bin/python3 ${computeFeaturesScript} \
    "$out/vendor" \
    ${featuresFileArg})

  # ==========================================================================
  # Phase 3: Generate BUCK files with unified features
  # ==========================================================================
  echo "Generating BUCK files with unified features..."
  ${allBuckGenCommands}

  # ==========================================================================
  # Phase 4: Create symlinks for unversioned crate references
  # ==========================================================================
  # Users can reference "rustdeps//vendor/itoa:itoa" instead of "rustdeps//vendor/itoa@1.0.17:itoa"
  ${symlinkCommands}

  # Generate cell .buckconfig
  cat > $out/.buckconfig << 'BUCKCONFIG'
  [cells]
      rustdeps = .
      prelude = prelude

  [buildfile]
      name = BUCK
  BUCKCONFIG

  echo "Generated rustdeps cell with ${toString (lib.length (lib.attrNames registry))} crates (with feature unification)"
''
