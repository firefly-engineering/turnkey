# Go dependencies registry
#
# Maps Go import paths to their source and metadata.
# This registry can be extended or overridden by consumers.
#
# Structure:
#   "import/path" = {
#     version = "vX.Y.Z";           # Go module version
#     src = <derivation>;           # Source fetcher (fetchFromGitHub, etc.)
#     deps = [ "other/import" ];    # Optional: Go import paths this depends on
#     subpackages = [ "sub" ];      # Optional: subpackages to include
#   };

{ pkgs }:

{
  "github.com/google/uuid" = {
    version = "v1.6.0";
    src = pkgs.fetchFromGitHub {
      owner = "google";
      repo = "uuid";
      rev = "v1.6.0";
      sha256 = "sha256-VWl9sqUzdOuhW0KzQlv0gwwUQClYkmZwSydHG2sALYw=";
    };
    deps = [ ];
  };
}
