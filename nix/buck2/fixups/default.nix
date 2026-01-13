# Default build script fixups
#
# This module exports all built-in fixups for crates that require
# pre-generated build script outputs.
#
# Fixups are functions that receive a context containing:
#   - crateName: the crate's name (e.g., "ring")
#   - version: full version string (e.g., "0.17.14")
#   - patchVersion: last component of version (e.g., "14")
#   - key: full key with version (e.g., "ring@0.17.14")
#   - vendorPath: path to crate in vendor dir (e.g., "vendor/ring@0.17.14")
#
# And return shell commands to generate the required files.

{ lib }:

let
  serdeFixups = import ./serde.nix { inherit lib; };
  ringFixups = import ./ring.nix { inherit lib; };
in
  # Merge all fixup registries
  serdeFixups // ringFixups
