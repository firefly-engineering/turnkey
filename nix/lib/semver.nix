# Semver utilities for Nix
#
# Provides semantic versioning parsing and comparison functions.
# Follows semver 2.0.0 spec: https://semver.org/
#
# Usage:
#   let
#     semver = import ./semver.nix { inherit lib; };
#   in
#   semver.compare "1.2.3" "1.2.4"  # returns -1
#
{ lib }:

let
  # Parse semver string "1.2.3" or "1.2.3-pre" into { major, minor, patch, prerelease }
  parse = version:
    let
      # Split off prerelease suffix if present (e.g., "1.0.0-alpha" -> ["1.0.0" "alpha"])
      preParts = lib.splitString "-" version;
      versionPart = lib.head preParts;
      prerelease = if lib.length preParts > 1 then lib.elemAt preParts 1 else null;

      # Split version into major.minor.patch
      parts = lib.splitString "." versionPart;
      major = lib.toInt (lib.elemAt parts 0);
      minor = if lib.length parts > 1 then lib.toInt (lib.elemAt parts 1) else 0;
      patch = if lib.length parts > 2 then lib.toInt (lib.elemAt parts 2) else 0;
    in
    { inherit major minor patch prerelease; };

  # Compare two parsed semver versions
  # Returns: -1 if a < b, 0 if a == b, 1 if a > b
  compareParsed = a: b:
    if a.major != b.major then
      (if a.major > b.major then 1 else -1)
    else if a.minor != b.minor then
      (if a.minor > b.minor then 1 else -1)
    else if a.patch != b.patch then
      (if a.patch > b.patch then 1 else -1)
    else
      # Prerelease versions are less than release versions
      # "1.0.0-alpha" < "1.0.0"
      if a.prerelease == null && b.prerelease != null then 1
      else if a.prerelease != null && b.prerelease == null then -1
      else if a.prerelease == null && b.prerelease == null then 0
      else builtins.compareVersions a.prerelease b.prerelease;

  # Compare two version strings directly
  # Returns: -1 if a < b, 0 if a == b, 1 if a > b
  compare = a: b:
    compareParsed (parse a) (parse b);

  # Sort comparator for lib.sort (returns true if a should come before b)
  # Ascending order: smaller versions first
  sortAsc = a: b:
    compare a.version b.version < 0;

  # Sort comparator for lib.sort (returns true if a should come before b)
  # Descending order: greater versions first
  sortDesc = a: b:
    compare a.version b.version > 0;

in
{
  inherit parse compareParsed compare sortAsc sortDesc;
}
