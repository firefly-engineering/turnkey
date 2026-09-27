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
# 4. Archives into libring_core_<major>_<minor>_<patch>__.a
#
# It is written for ring 0.17: its source lists are 0.17's, so other
# versions are left unaccounted for. The platform is the one the cell is
# built on (ctx.platform); the library exists only for it.
#
# Reference: https://github.com/briansmith/ring/blob/main/build.rs

{ lib, ... }:

let
  symbols = import ./ring-symbols.nix { inherit lib; };

  # Generate #define lines for symbol renames (no prefix)
  renameDefines = lib.concatMapStringsSep "\n" (r: "#define ${r.old} ${r.new}") symbols.symbolRenames;

  # Generate #define lines for symbol renames with underscore (Apple/Mach-O)
  renameDefinesApple = lib.concatMapStringsSep "\n" (
    r: "#define _${r.old} _${r.new}"
  ) symbols.symbolRenames;

  # Generate #define lines for symbol prefixes
  # Uses \${RING_PREFIX} which becomes ${RING_PREFIX} in shell (escaped in double-quoted string)
  prefixDefines = lib.concatMapStringsSep "\n" (
    sym: "#define ${sym} \${RING_PREFIX}${sym}"
  ) symbols.symbolsToPrefix;

  # Generate #define lines for symbol prefixes with underscore (Apple/Mach-O)
  # On Apple platforms, Mach-O symbols have a leading underscore
  prefixDefinesApple = lib.concatMapStringsSep "\n" (
    sym: "#define _${sym} _\${RING_PREFIX}${sym}"
  ) symbols.symbolsToPrefix;

  # Helper to generate shell array from a Nix list
  mkSourcesArray = srcs: lib.concatMapStringsSep "\n" (src: "        ${src}") srcs;

  # ring's sources for a platform: its CPU's C and assembly, in the object
  # format of its OS (ELF on Linux, Mach-O on macOS)
  platformSources =
    platform:
    if platform.cpu == "arm64" then
      {
        cSources = symbols.cSourcesCommon ++ symbols.cSourcesAarch64;
        asmSources =
          if platform.os == "macos" then
            symbols.asmSourcesAarch64Apple
          else
            map (builtins.replaceStrings [ "-ios64.S" ] [ "-linux64.S" ]) symbols.asmSourcesAarch64Apple;
      }
    else
      {
        cSources = symbols.cSourcesCommon ++ symbols.cSourcesX86_64;
        asmSources =
          if platform.os == "macos" then
            map (builtins.replaceStrings [ "-elf.S" ] [ "-macosx.S" ]) symbols.asmSourcesX86_64Linux
          else
            symbols.asmSourcesX86_64Linux;
      };

  # ring's symbol prefix, from its version
  prefixOf = v: "ring_core_${v.major}_${v.minor}_${v.patch}__";

  # The build script for the platform the cell is built on
  mkRingFixup =
    ctx:
    let
      platSrcs = platformSources ctx.platform;
      cSourcesArray = mkSourcesArray platSrcs.cSources;
      asmSourcesArray = mkSourcesArray platSrcs.asmSources;
      system = ctx.platform.system;
    in
    ''
          # Fixup: ring native crypto library compilation
          # Ring's build.rs compiles C and assembly files into libring_core_*.a
          # We replicate this in Nix for Buck2 to link against
          echo "Building ring native crypto library (${system})..."
          RING_SRC="$CRATE_SRC"
          RING_OUT="$OUT_DIR"

          # Symbol prefix to avoid conflicts (matches ring's build.rs)
          # Note: The prefix ends with double underscore, matching what ring's Rust code expects
          RING_PREFIX="${prefixOf ctx.versionParts}"

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

          # Generate assembly prefix header
          # On Apple (Mach-O), assembly symbols have a leading underscore, so we need
          # both _symbol and symbol defines. ring's build.rs uses #if defined(__APPLE__)
          # to conditionally include the underscore variants.
          cat > "$RING_OUT/ring_core_generated/prefix_symbols_asm.h" << RING_ASM_PREFIX_HEADER
      #ifndef ring_core_generated_PREFIX_SYMBOLS_ASM_H
      #define ring_core_generated_PREFIX_SYMBOLS_ASM_H

      #if defined(__APPLE__)
      // Apple/Mach-O: underscore-prefixed symbol renames
      ${renameDefinesApple}

      // Apple/Mach-O: underscore-prefixed symbols
      ${prefixDefinesApple}
      #else
      // ELF: symbol renames (from SYMBOLS_TO_RENAME in build.rs)
      ${renameDefines}

      // ELF: all symbols from SYMBOLS_TO_PREFIX in build.rs
      ${prefixDefines}
      #endif

      #endif
      RING_ASM_PREFIX_HEADER

          # Compiler flags matching ring's build.rs
          # Include paths: ring's include dir AND out_dir (for generated headers)
          RING_CFLAGS="-fvisibility=hidden -std=c1x -pedantic -Wall -I$RING_SRC/include -I$RING_OUT"

          # C source files to compile (platform-specific)
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

          # Assembly files (platform-specific)
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

in
{
  rust.ring.versions = [
    {
      when = {
        atLeast = "0.17";
        below = "0.18";
      };
      buildScript.generate = mkRingFixup;
      nativeLibraries = [
        {
          name = ctx: prefixOf ctx.versionParts;
          staticLib = ctx: "out_dir/lib${prefixOf ctx.versionParts}.a";
        }
      ];
    }
  ];
}
