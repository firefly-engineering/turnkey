# Cargo workspace utilities for Nix
#
# Provides helpers for building Rust packages from monorepo workspaces
# without needing to include all workspace members.
#
# Usage:
#   let
#     cargoLib = import ./cargo.nix { inherit pkgs lib; };
#   in
#   cargoLib.prunedCargoSource {
#     root = ./.;
#     members = [ "cmd/my-tool" "lib/my-lib" ];
#   }
#
{ pkgs, lib }:

let
  fs = lib.fileset;
  cargo-prune-workspace = import ../packages/cargo-prune-workspace.nix { inherit pkgs lib; };

  # Create a source tree with only specified workspace members in Cargo.toml
  #
  # Arguments:
  #   root: Path to the workspace root
  #   members: List of workspace members to keep (e.g., ["cmd/foo", "lib/bar"])
  #   lockFile: Path to Cargo.lock (defaults to root + "/Cargo.lock")
  #
  # Returns: A derivation containing the pruned source
  prunedCargoSource = {
    root,
    members,
    lockFile ? root + "/Cargo.lock",
  }:
    let
      # Build fileset for all member directories plus root files
      memberFilesets = map (m: root + "/${m}") members;
      fileset = fs.unions ([
        (root + "/Cargo.toml")
        lockFile
      ] ++ memberFilesets);

      # Create initial source with just the members we need
      initialSrc = fs.toSource {
        inherit root fileset;
      };

      # Comma-separated list of members for the CLI
      membersArg = lib.concatStringsSep "," members;
    in
    pkgs.runCommand "pruned-cargo-source" {
      nativeBuildInputs = [ cargo-prune-workspace ];
    } ''
      cp -r ${initialSrc} $out
      chmod -R u+w $out
      cargo-prune-workspace --manifest-path $out/Cargo.toml --members ${membersArg}
    '';

in
{
  inherit prunedCargoSource;
}
