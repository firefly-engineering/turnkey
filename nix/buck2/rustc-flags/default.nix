# Default rustc flags registry
#
# This module exports all built-in rustc flags for crates whose build
# scripts generate cfg directives. These flags are passed to rustc
# during compilation to emulate what the build script would set.
#
# Keys can be:
# - Crate names (catch-all): "serde_json"
# - Version-specific: "rustix@0.39.0" (takes precedence)
#
# Users can override or extend via turnkey.toolchains.buck2.rustcFlagsRegistry

{ lib }:

let
  serdeFlags = import ./serde.nix { inherit lib; };
  rustixFlags = import ./rustix.nix { inherit lib; };
in
  # Merge all flag registries
  serdeFlags // rustixFlags
