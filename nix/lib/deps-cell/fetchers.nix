# Unified fetcher functions for dependency cells
#
# Provides a dispatch mechanism to fetch sources from different origins:
#   - github: GitHub repositories
#   - git: Generic git repositories (for Foundry deps, etc.)
#   - cratesio: Rust crates from crates.io
#   - pypi: Python packages from PyPI
#   - goproxy: Go modules from proxy.golang.org
#   - url/npm: Direct URL download (for npm tarballs, etc.)

{ pkgs, lib }:

rec {
  # Main fetch dispatcher
  # Takes a fetch specification and returns a fetched source derivation
  fetch = fetchSpec:
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
    else
      throw "Unknown fetch type: ${fetchSpec.type}";

  # Fetch from GitHub
  # fetchSpec: { type, owner, repo, rev, sha256, ?sparseCheckout }
  fetchGitHub = fetchSpec:
    pkgs.fetchFromGitHub {
      inherit (fetchSpec) owner repo rev;
      sha256 = fetchSpec.sha256;
      sparseCheckout = fetchSpec.sparseCheckout or [];
    };

  # Fetch from a generic git repository
  # fetchSpec: { type, url, rev, hash, ?submodules }
  # Used for Foundry/Solidity dependencies that reference git repos
  fetchGit = fetchSpec:
    builtins.fetchGit {
      inherit (fetchSpec) url rev;
      allRefs = true;
      submodules = fetchSpec.submodules or false;
    };

  # Fetch from crates.io
  # fetchSpec: { type, crateName, version, sha256 }
  fetchCratesIO = fetchSpec:
    pkgs.fetchzip {
      url = "https://crates.io/api/v1/crates/${fetchSpec.crateName}/${fetchSpec.version}/download";
      sha256 = fetchSpec.sha256;
      extension = "tar.gz";
    };

  # Fetch from PyPI
  # fetchSpec: { type, url, sha256 }
  fetchPyPI = fetchSpec:
    pkgs.fetchzip {
      inherit (fetchSpec) url;
      sha256 = fetchSpec.sha256;
      extension = "tar.gz";
    };

  # Fetch from a URL (for npm packages and other tarballs)
  # fetchSpec: { type, url, hash }
  # The hash should be an SRI hash (sha512-...)
  fetchUrl = fetchSpec:
    pkgs.fetchurl {
      inherit (fetchSpec) url;
      hash = fetchSpec.hash;
    };

  # Fetch from Go module proxy
  # fetchSpec: { type, modulePath, version, sha256 }
  # Note: Go module zips have a single root directory "modulePath@version/"
  # so we let fetchzip strip it (the default behavior) to get clean paths.
  fetchGoProxy = fetchSpec:
    let
      # Escape uppercase letters in module path for proxy URL
      escapedPath = lib.concatStrings (
        lib.forEach (lib.stringToCharacters fetchSpec.modulePath) (c:
          if lib.strings.match "[A-Z]" c != null
          then "!${lib.toLower c}"
          else c
        )
      );
    in
    pkgs.fetchzip {
      url = "https://proxy.golang.org/${escapedPath}/@v/${fetchSpec.version}.zip";
      sha256 = fetchSpec.sha256;
      # stripRoot = true by default, which strips the "modulePath@version/" root
    };

  # Helper to create a fetch spec for GitHub
  mkGitHubSpec = { owner, repo, rev, sha256, sparseCheckout ? [] }: {
    type = "github";
    inherit owner repo rev sha256 sparseCheckout;
  };

  # Helper to create a fetch spec for generic git repos (Foundry deps, etc.)
  mkGitSpec = { url, rev, hash ? null, submodules ? false }: {
    type = "git";
    inherit url rev submodules;
  } // lib.optionalAttrs (hash != null) { inherit hash; };

  # Helper to create a fetch spec for crates.io
  mkCratesIOSpec = { crateName, version, sha256 }: {
    type = "cratesio";
    inherit crateName version sha256;
  };

  # Helper to create a fetch spec for PyPI
  mkPyPISpec = { url, sha256 }: {
    type = "pypi";
    inherit url sha256;
  };

  # Helper to create a fetch spec for Go proxy
  mkGoProxySpec = { modulePath, version, sha256 }: {
    type = "goproxy";
    inherit modulePath version sha256;
  };

  # Helper to create a fetch spec for URL (npm packages, etc.)
  mkUrlSpec = { url, hash }: {
    type = "url";
    inherit url hash;
  };
}
