# buck2 source revisions, keyed by buck2 release version.
#
# Turnkey ships the prebuilt buck2 release binary (from toolbox). Anything
# that mirrors one of buck2's wire formats, such as the test runner's protocol
# code, is generated from buck2's own source at the revision that binary was
# built from. This table is the single place that maps a release to that
# revision. A release is added only after turnkey's test runner passes its
# parity suite against it: with that release in the dev shell, run
# `python3 src/cmd/check-test-runner-parity/__main__.py` and require it to
# report every scenario as matching.
{ pkgs, lib }:

let
  releases = {
    "2026-09-15" = {
      rev = "6507dd157a6f81a810c48583edf1758dd0c337c5";
      protosHash = "sha256-xaGN1+FuT8DDiKI/Ww8cGrozx0Shvdyj9deH/ZycAdg=";
    };
    "2026-07-01" = {
      rev = "c88d791e34884e58617b92d5b98c7f71faee823c";
      protosHash = "sha256-XgPm5HNvqUNbKdWLSQ6YE7Bsbl17jIjzYTJYX34jh8s=";
    };
    "2026-04-15" = {
      rev = "7600cb80070a88b88be67aa5d20d6a93cffa0223";
      protosHash = "sha256-sWZTQ+79JI6bhxqlLjimAEq2bR/m9b8JGn/9Nvgpwqc=";
    };
  };

  # Split a store path's name ("<hash>-buck2-2026-04-15") into { name; version; }.
  parseStoreName =
    path:
    let
      # Only the name is wanted, not a dependency on the path it came from.
      base = builtins.unsafeDiscardStringContext (baseNameOf (toString path));
    in
    # Store path names are "<32-char hash>-<name>"
    builtins.parseDrvName (builtins.substring 33 (builtins.stringLength base) base);
in
{
  inherit releases;

  # The buck2 release version of a buck2 package, or of a meta-package
  # (e.g. toolbox's buck2-toolchain) that bundles one.
  versionOf =
    pkg:
    if (pkg.pname or null) == "buck2" then
      pkg.version
    else
      let
        buck2Paths = builtins.filter (p: (parseStoreName p).name == "buck2") (pkg.paths or [ ]);
      in
      if buck2Paths == [ ] then null else (parseStoreName (builtins.head buck2Paths)).version;

  # The buck2-prelude version to pair with a buck2 release: the newest one
  # that is not newer than the release. Both are named by release date
  # (YYYY-MM-DD), so they compare as strings. A prelude newer than the binary
  # may call builtins the binary doesn't have. `preludeVersions` are the
  # registry's available prelude versions; with no release known, or none old
  # enough, the registry's default is used.
  matchingPreludeVersion =
    {
      preludeVersions,
      default,
    }:
    buck2Version:
    let
      candidates = builtins.filter (v: buck2Version != null && v <= buck2Version) preludeVersions;
    in
    if candidates == [ ] then default else lib.last (lib.sort (a: b: a < b) candidates);

  # Whether turnkey knows the source revision of this buck2 release.
  isSupported = version: version != null && releases ? ${version};

  # buck2's test-runner protocol files at the revision of the given release.
  protosFor =
    version:
    let
      release =
        releases.${version} or (throw "turnkey: no buck2 source revision known for buck2 ${version}");
    in
    pkgs.fetchFromGitHub {
      owner = "facebook";
      repo = "buck2";
      inherit (release) rev;
      sparseCheckout = [
        "app/buck2_test_proto"
        "app/buck2_data"
        "app/buck2_host_sharing_proto"
      ];
      hash = release.protosHash;
    };
}
