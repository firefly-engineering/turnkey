# Rust Fixups Registry
#
# Fixups for Rust crates that require special handling during build.
# Each fixup file handles a family of related crates.
#
# This module exports three types of fixups:
#   - buildScriptFixups: Shell commands to generate build.rs outputs
#   - rustcFlags: --cfg flags to pass to rustc
#   - nativeLibraries: Info about pre-compiled native libraries
#
# Fixup files are auto-discovered from this directory. To add a new fixup:
#   1. Create a new .nix file (e.g., mycrate.nix)
#   2. Export any of: buildScriptFixups, rustcFlags, nativeLibraries
#   3. That's it - no need to modify this file!
#
# Build script fixups are functions: context -> string (shell commands)
# Context includes: { name, version, patchVersion, vendorPath, ... }
#
# Rustc flags are arrays: [ "--cfg" "flag" ... ]
#
# Native libraries are functions: context -> { lib_name, static_lib_path, link_search_path? }

{ pkgs, lib }:

let
  # Auto-discover all fixup files in this directory
  # Excludes: default.nix (this file), *-symbols.nix (helper files)
  fixupDir = ./.;
  allFiles = builtins.attrNames (builtins.readDir fixupDir);

  isFixupFile = name:
    lib.hasSuffix ".nix" name
    && name != "default.nix"
    && !lib.hasSuffix "-symbols.nix" name;

  fixupFiles = builtins.filter isFixupFile allFiles;

  # Import each fixup file
  importFixup = filename:
    import (fixupDir + "/${filename}") { inherit lib; };

  allFixups = map importFixup fixupFiles;

  # Merge a specific attribute from all fixups
  # For ring.nix backward compatibility: if fixup has no buildScriptFixups attr,
  # treat the whole fixup as buildScriptFixups (legacy format)
  mergeAttr = attrName: fixups:
    lib.foldl' (acc: fixup:
      acc // (
        if attrName == "buildScriptFixups" && !(fixup ? buildScriptFixups) && !(fixup ? rustcFlags) && !(fixup ? nativeLibraries)
        then fixup  # Legacy format: whole file is buildScriptFixups
        else (fixup.${attrName} or {})
      )
    ) {} fixups;

in
rec {
  # ==========================================================================
  # Build Script Fixups
  # ==========================================================================
  #
  # These generate files that build.rs would normally create.
  # Keyed by crate name or name@version.

  buildScriptFixups = mergeAttr "buildScriptFixups" allFixups;

  # ==========================================================================
  # Rustc Flags
  # ==========================================================================
  #
  # These are --cfg flags that build.rs would normally emit.
  # Keyed by crate name or name@version.

  rustcFlags = mergeAttr "rustcFlags" allFixups;

  # ==========================================================================
  # Native Libraries
  # ==========================================================================
  #
  # Info about pre-compiled native libraries that need to be linked.
  # Keyed by crate name or name@version.
  # Each entry is a function: context -> { lib_name, static_lib_path, link_search_path? }

  nativeLibraries = mergeAttr "nativeLibraries" allFixups;

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
