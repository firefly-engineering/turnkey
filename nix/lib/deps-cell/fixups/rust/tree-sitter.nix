# Tree-sitter build script fixups
#
# Tree-sitter uses a build script to copy stdlib-symbols.txt to OUT_DIR.
# This file contains symbols used for WASM stdlib support.
#
# Reference: https://github.com/tree-sitter/tree-sitter/blob/master/lib/binding_rust/build.rs

{ lib }:

{
  # ==========================================================================
  # Build Script Fixups
  # ==========================================================================

  buildScriptFixups = {
    # tree-sitter fixup: copies stdlib-symbols.txt to out_dir
    tree-sitter = { vendorPath, ... }: ''
      # Fixup: tree-sitter build script output
      # The build.rs copies src/wasm/stdlib-symbols.txt to OUT_DIR
      mkdir -p "$out/${vendorPath}/out_dir"
      cp "$out/${vendorPath}/src/wasm/stdlib-symbols.txt" \
         "$out/${vendorPath}/out_dir/stdlib-symbols.txt"
    '';
  };
}
