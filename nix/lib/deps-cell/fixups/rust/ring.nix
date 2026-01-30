# Ring build script fixup
#
# Ring requires compiling native C and assembly crypto code. Its build.rs
# normally does this, but we pre-compile in Nix to avoid needing build
# script execution at Buck2 build time.
#
# The fixup:
# 1. Generates prefix header files for symbol namespacing
# 2. Compiles C sources with proper flags
# 3. Assembles pregenerated .S files
# 4. Archives into libring_core_0_17_<patch>__.a
#
# Reference: https://github.com/briansmith/ring/blob/main/build.rs

{ lib }:

let
  symbols = import ./ring-symbols.nix { inherit lib; };

  # Generate #define lines for symbol renames (no prefix)
  renameDefines = lib.concatMapStringsSep "\n" (r:
    "#define ${r.old} ${r.new}"
  ) symbols.symbolRenames;

  # Generate #define lines for symbol prefixes
  # Uses \${RING_PREFIX} which becomes ${RING_PREFIX} in shell (escaped in double-quoted string)
  prefixDefines = lib.concatMapStringsSep "\n" (sym:
    "#define ${sym} \${RING_PREFIX}${sym}"
  ) symbols.symbolsToPrefix;

  # Generate C source array for shell
  cSourcesArray = lib.concatMapStringsSep "\n" (src:
    "        ${src}"
  ) symbols.cSources;

  # Generate ASM source array for shell
  asmSourcesArray = lib.concatMapStringsSep "\n" (src:
    "        ${src}"
  ) symbols.asmSources;

in
{
  # Ring native crypto library compilation fixup
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
    RING_PREFIX="ring_core_0_17_${patchVersion}__"

    # Generate prefix header for symbol namespacing
    # Ring expects this at ring_core_generated/prefix_symbols.h
    mkdir -p "$RING_OUT/ring_core_generated"
    cat > "$RING_OUT/ring_core_generated/prefix_symbols.h" << RING_PREFIX_HEADER
#ifndef ring_core_generated_PREFIX_SYMBOLS_H
#define ring_core_generated_PREFIX_SYMBOLS_H

// Symbol renames (from SYMBOLS_TO_RENAME in build.rs)
${renameDefines}

// All symbols from SYMBOLS_TO_PREFIX in build.rs
${prefixDefines}

#endif
RING_PREFIX_HEADER

    # Generate assembly prefix header (same symbols, for .S files)
    cat > "$RING_OUT/ring_core_generated/prefix_symbols_asm.h" << RING_ASM_PREFIX_HEADER
#ifndef ring_core_generated_PREFIX_SYMBOLS_ASM_H
#define ring_core_generated_PREFIX_SYMBOLS_ASM_H

// Symbol renames (from SYMBOLS_TO_RENAME in build.rs)
${renameDefines}

// All symbols from SYMBOLS_TO_PREFIX in build.rs
${prefixDefines}

#endif
RING_ASM_PREFIX_HEADER

    # Compiler flags matching ring's build.rs
    # Include paths: ring's include dir AND out_dir (for generated headers)
    RING_CFLAGS="-fvisibility=hidden -std=c1x -pedantic -Wall -I$RING_SRC/include -I$RING_OUT"

    # C source files to compile (x86_64-linux)
    RING_C_SRCS=(
${cSourcesArray}
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
${asmSourcesArray}
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

  # ==========================================================================
  # Native Libraries
  # ==========================================================================

  nativeLibraries = {
    # ring's native crypto library
    ring = { patchVersion, ... }: {
      lib_name = "ring_core_0_17_${patchVersion}__";
      static_lib_path = "out_dir/libring_core_0_17_${patchVersion}__.a";
      link_search_path = "out_dir";
    };
  };
}
