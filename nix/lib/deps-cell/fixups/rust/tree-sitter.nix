# Tree-sitter build script fixups (v3: include language grammars)
#
# Tree-sitter uses a build script to:
# 1. Copy stdlib-symbols.txt to OUT_DIR (for WASM support)
# 2. Compile the native C library (libtree_sitter.a)
#
# Language grammars (tree-sitter-rust, tree-sitter-python, etc.) also have
# native C parsers that need to be compiled.
#
# Reference: https://github.com/tree-sitter/tree-sitter/blob/master/lib/binding_rust/build.rs

{ lib }:

let
  # Helper to create a grammar fixup
  # Most grammars have parser.c in src/, some have scanner.c too
  mkGrammarFixup = { name, srcDir ? "src", hasScanner ? false, extraSrcs ? [] }: { vendorPath, ... }: ''
    echo "Building ${name} native library..."
    GRAM_SRC="$out/${vendorPath}/${srcDir}"
    GRAM_OUT="$out/${vendorPath}/out_dir"
    mkdir -p "$GRAM_OUT"

    GRAM_CFLAGS="-std=c11 -fvisibility=hidden -fPIC"
    GRAM_CFLAGS="$GRAM_CFLAGS -I$GRAM_SRC"

    # Compile parser.c
    GRAM_OBJS=()
    cc -c $GRAM_CFLAGS -o "$GRAM_OUT/parser.o" "$GRAM_SRC/parser.c"
    GRAM_OBJS+=("$GRAM_OUT/parser.o")

    ${if hasScanner then ''
    # Compile scanner.c
    if [ -f "$GRAM_SRC/scanner.c" ]; then
      cc -c $GRAM_CFLAGS -o "$GRAM_OUT/scanner.o" "$GRAM_SRC/scanner.c"
      GRAM_OBJS+=("$GRAM_OUT/scanner.o")
    fi
    '' else ""}

    ${lib.concatMapStringsSep "\n" (src: ''
    # Compile extra source: ${src}
    cc -c $GRAM_CFLAGS -o "$GRAM_OUT/$(basename ${src} .c).o" "$out/${vendorPath}/${src}"
    GRAM_OBJS+=("$GRAM_OUT/$(basename ${src} .c).o")
    '') extraSrcs}

    # Create static library
    ar rcs "$GRAM_OUT/lib${name}.a" "''${GRAM_OBJS[@]}"
    echo "Built ${name} native library: $GRAM_OUT/lib${name}.a"
  '';

  # Helper to create a grammar native library entry
  mkGrammarNativeLib = name: { ... }: {
    lib_name = name;
    static_lib_path = "out_dir/lib${name}.a";
    link_search_path = "out_dir";
  };

in
{
  # ==========================================================================
  # Build Script Fixups
  # ==========================================================================

  buildScriptFixups = {
    # tree-sitter fixup: copies stdlib-symbols.txt and compiles native library
    tree-sitter = { vendorPath, ... }: ''
      # Fixup: tree-sitter build script outputs
      echo "Building tree-sitter native library..."
      TS_SRC="$out/${vendorPath}"
      TS_OUT="$out/${vendorPath}/out_dir"
      mkdir -p "$TS_OUT"

      # 1. Copy stdlib-symbols.txt (for WASM support)
      cp "$TS_SRC/src/wasm/stdlib-symbols.txt" "$TS_OUT/stdlib-symbols.txt"

      # 2. Compile the native C library
      # tree-sitter uses a unity build (lib.c includes all other .c files)
      # Compiler flags match build.rs
      TS_CFLAGS="-std=c11 -fvisibility=hidden -fPIC"
      TS_CFLAGS="$TS_CFLAGS -D_POSIX_C_SOURCE=200112L -D_DEFAULT_SOURCE"
      TS_CFLAGS="$TS_CFLAGS -I$TS_SRC/src -I$TS_SRC/src/wasm -I$TS_SRC/include"
      TS_CFLAGS="$TS_CFLAGS -Wno-unused-parameter -Wno-trigraphs -Wno-unused-but-set-variable"

      # Compile lib.c (unity build)
      cc -c $TS_CFLAGS -o "$TS_OUT/lib.o" "$TS_SRC/src/lib.c"

      # Create static library
      ar rcs "$TS_OUT/libtree_sitter.a" "$TS_OUT/lib.o"
      echo "Built tree-sitter native library: $TS_OUT/libtree_sitter.a"
    '';

    # Language grammar fixups
    tree-sitter-go = mkGrammarFixup {
      name = "tree_sitter_go";
    };

    tree-sitter-rust = mkGrammarFixup {
      name = "tree_sitter_rust";
      hasScanner = true;
    };

    tree-sitter-python = mkGrammarFixup {
      name = "tree_sitter_python";
      hasScanner = true;
    };

    tree-sitter-solidity = mkGrammarFixup {
      name = "tree_sitter_solidity";
    };

    tree-sitter-starlark = mkGrammarFixup {
      name = "tree_sitter_starlark";
      hasScanner = true;
    };

    # tree-sitter-typescript has two grammars: typescript and tsx
    tree-sitter-typescript = { vendorPath, ... }: ''
      echo "Building tree-sitter-typescript native libraries..."
      TS_BASE="$out/${vendorPath}"
      TS_OUT="$out/${vendorPath}/out_dir"
      mkdir -p "$TS_OUT"

      GRAM_CFLAGS="-std=c11 -fvisibility=hidden -fPIC"

      # Build TypeScript grammar
      TS_TS_SRC="$TS_BASE/typescript/src"
      GRAM_OBJS=()
      cc -c $GRAM_CFLAGS -I"$TS_TS_SRC" -o "$TS_OUT/ts_parser.o" "$TS_TS_SRC/parser.c"
      GRAM_OBJS+=("$TS_OUT/ts_parser.o")
      if [ -f "$TS_TS_SRC/scanner.c" ]; then
        cc -c $GRAM_CFLAGS -I"$TS_TS_SRC" -o "$TS_OUT/ts_scanner.o" "$TS_TS_SRC/scanner.c"
        GRAM_OBJS+=("$TS_OUT/ts_scanner.o")
      fi
      ar rcs "$TS_OUT/libtree_sitter_typescript.a" "''${GRAM_OBJS[@]}"
      echo "Built tree_sitter_typescript: $TS_OUT/libtree_sitter_typescript.a"

      # Build TSX grammar
      TS_TSX_SRC="$TS_BASE/tsx/src"
      TSX_OBJS=()
      cc -c $GRAM_CFLAGS -I"$TS_TSX_SRC" -o "$TS_OUT/tsx_parser.o" "$TS_TSX_SRC/parser.c"
      TSX_OBJS+=("$TS_OUT/tsx_parser.o")
      if [ -f "$TS_TSX_SRC/scanner.c" ]; then
        cc -c $GRAM_CFLAGS -I"$TS_TSX_SRC" -o "$TS_OUT/tsx_scanner.o" "$TS_TSX_SRC/scanner.c"
        TSX_OBJS+=("$TS_OUT/tsx_scanner.o")
      fi
      ar rcs "$TS_OUT/libtree_sitter_tsx.a" "''${TSX_OBJS[@]}"
      echo "Built tree_sitter_tsx: $TS_OUT/libtree_sitter_tsx.a"
    '';
  };

  # ==========================================================================
  # Native Libraries
  # ==========================================================================

  nativeLibraries = {
    # tree-sitter's native C library
    tree-sitter = { ... }: {
      lib_name = "tree_sitter";
      static_lib_path = "out_dir/libtree_sitter.a";
      link_search_path = "out_dir";
    };

    # Language grammars
    tree-sitter-go = mkGrammarNativeLib "tree_sitter_go";
    tree-sitter-rust = mkGrammarNativeLib "tree_sitter_rust";
    tree-sitter-python = mkGrammarNativeLib "tree_sitter_python";
    tree-sitter-solidity = mkGrammarNativeLib "tree_sitter_solidity";
    tree-sitter-starlark = mkGrammarNativeLib "tree_sitter_starlark";
    tree-sitter-typescript = { ... }: {
      lib_name = "tree_sitter_typescript";
      static_lib_path = "out_dir/libtree_sitter_typescript.a";
      link_search_path = "out_dir";
    };
  };
}
