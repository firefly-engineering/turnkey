# Rust Fixups Registry
#
# Fixups for Rust crates that require special handling during build.
# Each fixup file handles a family of related crates.
#
# Fixups are organized as:
#   - serde.nix: serde, serde_core, serde_json, serde_derive
#   - ring.nix: ring crypto library (native code)
#   - rustix.nix: rustix platform flags
#
# Each fixup is a function: context -> string (shell commands)
# Context includes: { name, version, patchVersion, vendorPath, ... }

{ pkgs, lib }:

let
  # Import individual fixup files
  # TODO: Migrate from nix/buck2/fixups/ once adapters are ready
  # serdeFixups = import ./serde.nix { inherit pkgs lib; };
  # ringFixups = import ./ring.nix { inherit pkgs lib; };
  # rustixFixups = import ./rustix.nix { inherit pkgs lib; };
in
{
  # Placeholder - will be populated when migrating from nix/buck2/fixups/
  # See nix/buck2/fixups/default.nix for current implementation
}
