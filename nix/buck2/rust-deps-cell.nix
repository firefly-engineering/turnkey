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

{ pkgs, lib, depsFile, featuresFile ? null }:

# Build tools needed for native code compilation (ring, etc.)
# Using stdenv.cc for the C compiler and binutils for ar
let buildTools = with pkgs; [ stdenv.cc perl ];
in

let
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

  # ==========================================================================
  # Build script fixups
  # ==========================================================================
  # Some crates have build scripts that generate files. We pre-generate these
  # in Nix to avoid needing to run build scripts at Buck2 build time.

  # Generate fixup commands for a specific crate
  # Returns empty string if no fixup needed
  getFixupCommands = key: dep:
    let
      crateName = dep.crateName;
      version = dep.version;
      patchVersion = lib.last (lib.splitString "." version);
      vendorPath = "vendor/${key}";

      # serde_core's private.rs - just the versioned module
      serdeCorePrivateRs = ''
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
      '';

      # serde's private.rs - versioned module PLUS the serde_core_private alias
      serdePrivateRs = ''
#[doc(hidden)]
pub mod __private${patchVersion} {
    #[doc(hidden)]
    pub use crate::private::*;
}
use serde_core::__private${patchVersion} as serde_core_private;
      '';
    in
    if crateName == "serde_core" then ''
      # Fixup: serde_core build script output
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'SERDE_CORE_PRIVATE'
${serdeCorePrivateRs}
SERDE_CORE_PRIVATE
    ''
    else if crateName == "serde" then ''
      # Fixup: serde build script output (includes serde_core_private alias)
      mkdir -p "$out/${vendorPath}/out_dir"
      cat > "$out/${vendorPath}/out_dir/private.rs" << 'SERDE_PRIVATE'
${serdePrivateRs}
SERDE_PRIVATE
    ''
    else if crateName == "ring" then ''
      # Fixup: ring native crypto library compilation
      # Ring's build.rs compiles C and assembly files into libring_core_*.a
      # We replicate this in Nix for Buck2 to link against
      echo "Building ring native crypto library..."
      RING_SRC="$out/${vendorPath}"
      RING_OUT="$out/${vendorPath}/out_dir"
      mkdir -p "$RING_OUT"

      # Symbol prefix to avoid conflicts (matches ring's build.rs)
      RING_PREFIX="ring_core_0_17_${patchVersion}_"

      # Generate prefix header for symbol namespacing
      # Ring expects this at ring_core_generated/prefix_symbols.h
      mkdir -p "$RING_OUT/ring_core_generated"
      cat > "$RING_OUT/ring_core_generated/prefix_symbols.h" << RING_PREFIX_HEADER
#define RING_CORE_PREFIX $RING_PREFIX
#define CRYPTO_memcmp ''${RING_PREFIX}CRYPTO_memcmp
#define ChaCha20_ctr32 ''${RING_PREFIX}ChaCha20_ctr32
#define ChaCha20_ctr32_avx2 ''${RING_PREFIX}ChaCha20_ctr32_avx2
#define ChaCha20_ctr32_ssse3_4x ''${RING_PREFIX}ChaCha20_ctr32_ssse3_4x
#define ChaCha20_ctr32_nohw ''${RING_PREFIX}ChaCha20_ctr32_nohw
#define bn_mul_mont ''${RING_PREFIX}bn_mul_mont
#define bn_mul_mont_nohw ''${RING_PREFIX}bn_mul_mont_nohw
#define bn_mul4x_mont ''${RING_PREFIX}bn_mul4x_mont
#define bn_mulx4x_mont ''${RING_PREFIX}bn_mulx4x_mont
#define bn_sqr8x_mont ''${RING_PREFIX}bn_sqr8x_mont
#define bn_from_montgomery_in_place ''${RING_PREFIX}bn_from_montgomery_in_place
#define gcm_init_clmul ''${RING_PREFIX}gcm_init_clmul
#define gcm_ghash_clmul ''${RING_PREFIX}gcm_ghash_clmul
#define gcm_init_nohw ''${RING_PREFIX}gcm_init_nohw
#define gcm_ghash_nohw ''${RING_PREFIX}gcm_ghash_nohw
#define p256_point_mul ''${RING_PREFIX}p256_point_mul
#define p256_point_mul_base ''${RING_PREFIX}p256_point_mul_base
#define p256_point_mul_base_vartime ''${RING_PREFIX}p256_point_mul_base_vartime
#define p256_scalar_mul_mont ''${RING_PREFIX}p256_scalar_mul_mont
#define x25519_ge_scalarmult_base ''${RING_PREFIX}x25519_ge_scalarmult_base
#define x25519_ge_double_scalarmult_vartime ''${RING_PREFIX}x25519_ge_double_scalarmult_vartime
#define x25519_ge_frombytes_vartime ''${RING_PREFIX}x25519_ge_frombytes_vartime
#define x25519_scalar_mult_generic_masked ''${RING_PREFIX}x25519_scalar_mult_generic_masked
#define x25519_scalar_mult_adx ''${RING_PREFIX}x25519_scalar_mult_adx
#define x25519_public_from_private_generic_masked ''${RING_PREFIX}x25519_public_from_private_generic_masked
#define x25519_fe_invert ''${RING_PREFIX}x25519_fe_invert
#define x25519_fe_mul_ttt ''${RING_PREFIX}x25519_fe_mul_ttt
#define x25519_fe_neg ''${RING_PREFIX}x25519_fe_neg
#define x25519_fe_tobytes ''${RING_PREFIX}x25519_fe_tobytes
#define x25519_fe_isnegative ''${RING_PREFIX}x25519_fe_isnegative
#define x25519_sc_muladd ''${RING_PREFIX}x25519_sc_muladd
#define aes_hw_ctr32_encrypt_blocks ''${RING_PREFIX}aes_hw_ctr32_encrypt_blocks
#define aes_hw_set_encrypt_key ''${RING_PREFIX}aes_hw_set_encrypt_key
#define aes_nohw_ctr32_encrypt_blocks ''${RING_PREFIX}aes_nohw_ctr32_encrypt_blocks
#define aes_nohw_set_encrypt_key ''${RING_PREFIX}aes_nohw_set_encrypt_key
#define vpaes_ctr32_encrypt_blocks ''${RING_PREFIX}vpaes_ctr32_encrypt_blocks
#define vpaes_set_encrypt_key ''${RING_PREFIX}vpaes_set_encrypt_key
#define sha256_block_data_order ''${RING_PREFIX}sha256_block_data_order
#define sha256_block_data_order_hw ''${RING_PREFIX}sha256_block_data_order_hw
#define sha256_block_data_order_ssse3 ''${RING_PREFIX}sha256_block_data_order_ssse3
#define sha256_block_data_order_avx ''${RING_PREFIX}sha256_block_data_order_avx
#define sha256_block_data_order_avx2 ''${RING_PREFIX}sha256_block_data_order_avx2
#define sha512_block_data_order ''${RING_PREFIX}sha512_block_data_order
#define sha512_block_data_order_hw ''${RING_PREFIX}sha512_block_data_order_hw
#define sha512_block_data_order_avx ''${RING_PREFIX}sha512_block_data_order_avx
#define sha512_block_data_order_avx2 ''${RING_PREFIX}sha512_block_data_order_avx2
#define OPENSSL_ia32cap_P ''${RING_PREFIX}OPENSSL_ia32cap_P
#define OPENSSL_cpuid_setup ''${RING_PREFIX}OPENSSL_cpuid_setup
#define chacha20_poly1305_open ''${RING_PREFIX}chacha20_poly1305_open
#define chacha20_poly1305_seal ''${RING_PREFIX}chacha20_poly1305_seal
RING_PREFIX_HEADER

      # Generate assembly prefix header (same symbols but in assembly-compatible format)
      cat > "$RING_OUT/ring_core_generated/prefix_symbols_asm.h" << RING_ASM_PREFIX_HEADER
#define SYMBOL_PREFIX ring_core_0_17_${patchVersion}_
#define CRYPTO_memcmp ring_core_0_17_${patchVersion}_CRYPTO_memcmp
#define ChaCha20_ctr32 ring_core_0_17_${patchVersion}_ChaCha20_ctr32
#define ChaCha20_ctr32_avx2 ring_core_0_17_${patchVersion}_ChaCha20_ctr32_avx2
#define ChaCha20_ctr32_ssse3_4x ring_core_0_17_${patchVersion}_ChaCha20_ctr32_ssse3_4x
#define ChaCha20_ctr32_nohw ring_core_0_17_${patchVersion}_ChaCha20_ctr32_nohw
#define bn_mul_mont ring_core_0_17_${patchVersion}_bn_mul_mont
#define bn_mul_mont_nohw ring_core_0_17_${patchVersion}_bn_mul_mont_nohw
#define bn_mul4x_mont ring_core_0_17_${patchVersion}_bn_mul4x_mont
#define bn_mulx4x_mont ring_core_0_17_${patchVersion}_bn_mulx4x_mont
#define bn_sqr8x_mont ring_core_0_17_${patchVersion}_bn_sqr8x_mont
#define bn_from_montgomery_in_place ring_core_0_17_${patchVersion}_bn_from_montgomery_in_place
#define gcm_init_clmul ring_core_0_17_${patchVersion}_gcm_init_clmul
#define gcm_ghash_clmul ring_core_0_17_${patchVersion}_gcm_ghash_clmul
#define gcm_init_nohw ring_core_0_17_${patchVersion}_gcm_init_nohw
#define gcm_ghash_nohw ring_core_0_17_${patchVersion}_gcm_ghash_nohw
#define p256_point_mul ring_core_0_17_${patchVersion}_p256_point_mul
#define p256_point_mul_base ring_core_0_17_${patchVersion}_p256_point_mul_base
#define p256_point_mul_base_vartime ring_core_0_17_${patchVersion}_p256_point_mul_base_vartime
#define p256_scalar_mul_mont ring_core_0_17_${patchVersion}_p256_scalar_mul_mont
#define x25519_ge_scalarmult_base ring_core_0_17_${patchVersion}_x25519_ge_scalarmult_base
#define x25519_ge_double_scalarmult_vartime ring_core_0_17_${patchVersion}_x25519_ge_double_scalarmult_vartime
#define x25519_ge_frombytes_vartime ring_core_0_17_${patchVersion}_x25519_ge_frombytes_vartime
#define x25519_scalar_mult_generic_masked ring_core_0_17_${patchVersion}_x25519_scalar_mult_generic_masked
#define x25519_scalar_mult_adx ring_core_0_17_${patchVersion}_x25519_scalar_mult_adx
#define x25519_public_from_private_generic_masked ring_core_0_17_${patchVersion}_x25519_public_from_private_generic_masked
#define x25519_fe_invert ring_core_0_17_${patchVersion}_x25519_fe_invert
#define x25519_fe_mul_ttt ring_core_0_17_${patchVersion}_x25519_fe_mul_ttt
#define x25519_fe_neg ring_core_0_17_${patchVersion}_x25519_fe_neg
#define x25519_fe_tobytes ring_core_0_17_${patchVersion}_x25519_fe_tobytes
#define x25519_fe_isnegative ring_core_0_17_${patchVersion}_x25519_fe_isnegative
#define x25519_sc_muladd ring_core_0_17_${patchVersion}_x25519_sc_muladd
#define aes_hw_ctr32_encrypt_blocks ring_core_0_17_${patchVersion}_aes_hw_ctr32_encrypt_blocks
#define aes_hw_set_encrypt_key ring_core_0_17_${patchVersion}_aes_hw_set_encrypt_key
#define aes_nohw_ctr32_encrypt_blocks ring_core_0_17_${patchVersion}_aes_nohw_ctr32_encrypt_blocks
#define aes_nohw_set_encrypt_key ring_core_0_17_${patchVersion}_aes_nohw_set_encrypt_key
#define vpaes_ctr32_encrypt_blocks ring_core_0_17_${patchVersion}_vpaes_ctr32_encrypt_blocks
#define vpaes_set_encrypt_key ring_core_0_17_${patchVersion}_vpaes_set_encrypt_key
#define sha256_block_data_order ring_core_0_17_${patchVersion}_sha256_block_data_order
#define sha256_block_data_order_hw ring_core_0_17_${patchVersion}_sha256_block_data_order_hw
#define sha256_block_data_order_ssse3 ring_core_0_17_${patchVersion}_sha256_block_data_order_ssse3
#define sha256_block_data_order_avx ring_core_0_17_${patchVersion}_sha256_block_data_order_avx
#define sha256_block_data_order_avx2 ring_core_0_17_${patchVersion}_sha256_block_data_order_avx2
#define sha512_block_data_order ring_core_0_17_${patchVersion}_sha512_block_data_order
#define sha512_block_data_order_hw ring_core_0_17_${patchVersion}_sha512_block_data_order_hw
#define sha512_block_data_order_avx ring_core_0_17_${patchVersion}_sha512_block_data_order_avx
#define sha512_block_data_order_avx2 ring_core_0_17_${patchVersion}_sha512_block_data_order_avx2
#define OPENSSL_ia32cap_P ring_core_0_17_${patchVersion}_OPENSSL_ia32cap_P
#define OPENSSL_cpuid_setup ring_core_0_17_${patchVersion}_OPENSSL_cpuid_setup
#define chacha20_poly1305_open ring_core_0_17_${patchVersion}_chacha20_poly1305_open
#define chacha20_poly1305_seal ring_core_0_17_${patchVersion}_chacha20_poly1305_seal
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
    ''
    else "";

  # Check if a crate needs build script fixups
  needsFixup = crateName:
    crateName == "serde_core" || crateName == "serde" || crateName == "ring";

  # JSON map of crates that need OUT_DIR set (for gen-rust-buck.py)
  fixupCratesJson = builtins.toJSON (lib.filter needsFixup (lib.attrNames cratesByName));

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
