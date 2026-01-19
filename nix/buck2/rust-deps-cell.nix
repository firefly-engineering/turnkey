# Rust dependencies cell builder
#
# Reads a rust-deps.toml file and builds a Buck2 cell containing
# all crate dependencies with rules.star files for rust_library targets.
#
# The TOML file format (supports multiple versions of same crate):
#   [deps."crate-name@1.0.0"]
#   name = "crate-name"
#   version = "1.0.0"
#   hash = "sha256-..."
#   features = ["feature1", "feature2"]  # optional
#
# This is now a thin wrapper around the deps-cell library.
#
# Feature unification:
# Features are unified across the dependency graph, matching Cargo's behavior.
# If any crate requires feature X on crate Y, crate Y is built with feature X.
#
# Manual overrides can be specified in an optional featuresFile (rust-features.toml).
#
# Build script fixups:
# Some crates have build scripts that generate files needed at compile time.
# We handle these by pre-generating the output in Nix via the fixups registry.

{ pkgs, lib, depsFile, featuresFile ? null, rustcFlagsRegistry ? {}, buildScriptFixups ? {} }:

let
  depsCell = import ../lib/deps-cell { inherit pkgs lib; };

  # Import tools for feature unification and rules.star generation
  genRustBuck = import ../packages/gen-rust-buck.nix { inherit pkgs lib; };
  computeUnifiedFeatures = import ../packages/compute-unified-features.nix { inherit pkgs lib; };

  # Import default registries from deps-cell fixups
  rustFixups = import ../lib/deps-cell/fixups/rust { inherit pkgs lib; };
  defaultRustcFlagsRegistry = rustFixups.rustcFlags;
  defaultBuildScriptFixups = rustFixups.buildScriptFixups;
in
depsCell.mkRustDepsCell {
  inherit depsFile featuresFile;
  inherit computeUnifiedFeatures genRustBuck;

  # Merge user-provided registries with defaults
  buildScriptFixups = defaultBuildScriptFixups // buildScriptFixups;
  rustcFlagsRegistry = defaultRustcFlagsRegistry // rustcFlagsRegistry;
}
