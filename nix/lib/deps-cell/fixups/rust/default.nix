# Rust Fixups Registry
#
# Fixups for Rust crates that require special handling during build.
# Each fixup file handles a family of related crates.
#
# This module exports two types of fixups:
#   - buildScriptFixups: Shell commands to generate build.rs outputs
#   - rustcFlags: --cfg flags to pass to rustc
#
# Fixups are organized as:
#   - serde.nix: serde, serde_core (build script), serde_json (rustc flags)
#   - thiserror.nix: thiserror (build script)
#   - ring.nix: ring crypto library (native code compilation)
#   - rustix.nix: rustix platform flags (rustc flags)
#   - tree-sitter.nix: tree-sitter WASM stdlib symbols (build script)
#
# Build script fixups are functions: context -> string (shell commands)
# Context includes: { name, version, patchVersion, vendorPath, ... }
#
# Rustc flags are arrays: [ "--cfg" "flag" ... ]

{ pkgs, lib }:

let
  # Import individual fixup files
  serdeFixups = import ./serde.nix { inherit lib; };
  thiserrorFixups = import ./thiserror.nix { inherit lib; };
  ringFixups = import ./ring.nix { inherit lib; };
  rustixFixups = import ./rustix.nix { inherit lib; };
  treeSitterFixups = import ./tree-sitter.nix { inherit lib; };
in
rec {
  # ==========================================================================
  # Build Script Fixups
  # ==========================================================================
  #
  # These generate files that build.rs would normally create.
  # Keyed by crate name or name@version.

  buildScriptFixups =
    (serdeFixups.buildScriptFixups or {})
    // (thiserrorFixups.buildScriptFixups or {})
    // (ringFixups.buildScriptFixups or ringFixups)
    // (rustixFixups.buildScriptFixups or {})
    // (treeSitterFixups.buildScriptFixups or {});

  # ==========================================================================
  # Rustc Flags
  # ==========================================================================
  #
  # These are --cfg flags that build.rs would normally emit.
  # Keyed by crate name or name@version.

  rustcFlags =
    (serdeFixups.rustcFlags or {})
    // (thiserrorFixups.rustcFlags or {})
    // (ringFixups.rustcFlags or {})
    // (rustixFixups.rustcFlags or {})
    // (treeSitterFixups.rustcFlags or {});

  # ==========================================================================
  # Combined (for backward compatibility)
  # ==========================================================================
  #
  # The legacy API expected all fixups in a single attribute set.
  # This merges build script fixups for compatibility with existing code.

  # Legacy export: just the build script fixups (what old fixups/default.nix exported)
  __legacyBuildScriptFixups = buildScriptFixups;

  # Legacy export: just the rustc flags (what old rustc-flags/default.nix exported)
  __legacyRustcFlags = rustcFlags;
}
