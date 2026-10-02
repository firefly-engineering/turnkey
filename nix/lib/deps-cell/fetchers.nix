# Unified fetcher functions for dependency cells
#
# Provides a dispatch mechanism to fetch sources from different origins:
#   - github: GitHub repositories
#   - git: Generic git repositories (for Foundry deps, etc.)
#   - cratesio: Rust crates from crates.io
#   - pypi: Python packages' pure wheels from PyPI
#   - goproxy: Go modules from proxy.golang.org
#   - url/npm: Direct URL download (for npm tarballs, etc.)
#   - zip: Unpacked archive download (for prefetched git archives, etc.)

{ pkgs, lib }:

rec {
  # Main fetch dispatcher
  # Takes a fetch specification and returns a fetched source derivation
  fetch =
    fetchSpec:
    if fetchSpec.type == "github" then
      fetchGitHub fetchSpec
    else if fetchSpec.type == "git" then
      fetchGit fetchSpec
    else if fetchSpec.type == "cratesio" then
      fetchCratesIO fetchSpec
    else if fetchSpec.type == "pypi" then
      fetchPyPI fetchSpec
    else if fetchSpec.type == "goproxy" then
      fetchGoProxy fetchSpec
    else if fetchSpec.type == "url" || fetchSpec.type == "npm" then
      fetchUrl fetchSpec
    else if fetchSpec.type == "zip" then
      fetchZip fetchSpec
    else
      throw "Unknown fetch type: ${fetchSpec.type}";

  # Fetch from GitHub
  # fetchSpec: { type, owner, repo, rev, sha256, ?sparseCheckout }
  fetchGitHub =
    fetchSpec:
    pkgs.fetchFromGitHub {
      inherit (fetchSpec) owner repo rev;
      sha256 = fetchSpec.sha256;
      sparseCheckout = fetchSpec.sparseCheckout or [ ];
    };

  # Fetch from a generic git repository
  # fetchSpec: { type, url, rev, hash, ?submodules }
  # Used for Foundry/Solidity dependencies that reference git repos
  fetchGit =
    fetchSpec:
    builtins.fetchGit {
      inherit (fetchSpec) url rev;
      allRefs = true;
      submodules = fetchSpec.submodules or false;
    };

  # Fetch from crates.io
  # fetchSpec: { type, crateName, version, sha256 }
  fetchCratesIO =
    fetchSpec:
    pkgs.fetchzip {
      url = "https://crates.io/api/v1/crates/${fetchSpec.crateName}/${fetchSpec.version}/download";
      sha256 = fetchSpec.sha256;
      extension = "tar.gz";
    };

  # Fetch a pure wheel from PyPI, unpacked (ADR 0013)
  # fetchSpec: { type, url, sha256 }
  # A .whl is a zip that unpack doesn't know by its extension, so it is
  # named one. Its root is the installed layout, several entries side by
  # side (the packages and their *.dist-info), so it is kept as it is:
  # the hash is pydeps-gen's nix-prefetch-url --unpack of the same URL.
  fetchPyPI =
    fetchSpec:
    pkgs.fetchzip {
      inherit (fetchSpec) url;
      sha256 = fetchSpec.sha256;
      extension = "zip";
      stripRoot = false;
    };

  # Fetch from a URL (for npm packages and other tarballs)
  # fetchSpec: { type, url, hash }
  # The hash should be an SRI hash (sha512-...)
  fetchUrl =
    fetchSpec:
    pkgs.fetchurl {
      inherit (fetchSpec) url;
      hash = fetchSpec.hash;
    };

  # Fetch and unpack an archive
  # fetchSpec: { type, url, hash }
  # The hash is the SRI hash of the unpacked contents (nix-prefetch-url --unpack)
  fetchZip =
    fetchSpec:
    pkgs.fetchzip {
      inherit (fetchSpec) url hash;
    };

  # Fetch from Go module proxy
  # fetchSpec: { type, modulePath, version, sha256 }
  # Note: Go module zips have a single root directory "modulePath@version/"
  # so we let fetchzip strip it (the default behavior) to get clean paths.
  fetchGoProxy =
    fetchSpec:
    pkgs.fetchzip {
      url = goProxyZipUrl fetchSpec;
      sha256 = fetchSpec.sha256;
      # stripRoot = true by default, which strips the "modulePath@version/" root
    };

  # The proxy.golang.org zip of a module version. The module proxy protocol
  # case-escapes both the module path and the version: an uppercase letter
  # becomes "!" and its lowercase (https://go.dev/ref/mod#goproxy-protocol).
  # godeps-gen hashes the same URL, built as golang.org/x/mod/module builds
  # it (src/cmd/godeps-gen/src/prefetch.rs), so the two must agree.
  goProxyZipUrl =
    { modulePath, version, ... }:
    let
      escape =
        s:
        lib.concatMapStrings (c: if c != lib.toLower c then "!${lib.toLower c}" else c) (
          lib.stringToCharacters s
        );
    in
    "https://proxy.golang.org/${escape modulePath}/@v/${escape version}.zip";

  # Helper to create a fetch spec for GitHub
  mkGitHubSpec =
    {
      owner,
      repo,
      rev,
      sha256,
      sparseCheckout ? [ ],
    }:
    {
      type = "github";
      inherit
        owner
        repo
        rev
        sha256
        sparseCheckout
        ;
    };

  # Helper to create a fetch spec for generic git repos (Foundry deps, etc.)
  mkGitSpec =
    {
      url,
      rev,
      hash ? null,
      submodules ? false,
    }:
    {
      type = "git";
      inherit url rev submodules;
    }
    // lib.optionalAttrs (hash != null) { inherit hash; };

  # Helper to create a fetch spec for crates.io
  mkCratesIOSpec =
    {
      crateName,
      version,
      sha256,
    }:
    {
      type = "cratesio";
      inherit crateName version sha256;
    };

  # Helper to create a fetch spec for PyPI
  mkPyPISpec =
    { url, sha256 }:
    {
      type = "pypi";
      inherit url sha256;
    };

  # Helper to create a fetch spec for Go proxy
  mkGoProxySpec =
    {
      modulePath,
      version,
      sha256,
    }:
    {
      type = "goproxy";
      inherit modulePath version sha256;
    };

  # Helper to create a fetch spec for URL (npm packages, etc.)
  mkUrlSpec =
    { url, hash }:
    {
      type = "url";
      inherit url hash;
    };
}
