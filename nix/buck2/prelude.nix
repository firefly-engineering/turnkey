# Turnkey Prelude - Nix-backed Buck2 prelude cell
#
# This derivation builds a customizable prelude by:
# 1. Fetching upstream buck2-prelude at a pinned commit
# 2. Applying patches from nix/patches/prelude/ (if any)
# 3. Copying extensions from nix/buck2/prelude-extensions/
#
# The result is symlinked to .turnkey/prelude in downstream projects.
#
# IMPORTANT: The prelude version must match the buck2 binary version.
# See docs/buck2-prelude-compatibility.md for update instructions.
{ pkgs, lib }:

let
  # Pin to a specific buck2-prelude commit
  # This should be updated when buck2 version changes in nixpkgs
  # Current buck2 version: 2026-03-15 (from toolbox default)
  # Prelude must match buck2 version to avoid API incompatibilities
  version = "2026-03-15";
  rev = "27c8628d9bd9324e6dba3fd0e5c112e6ea4c5795";

  upstreamPrelude = pkgs.fetchFromGitHub {
    owner = "facebook";
    repo = "buck2-prelude";
    inherit rev;
    hash = "sha256-jTr/I75Vg7jBz0lyrud+Qr5fDtgVDkea7SwDaIyn+/8=";
  };

  # Directory containing patches to apply
  patchDir = ../patches/prelude;

  # Find all .patch files in the patch directory
  patchFiles =
    if builtins.pathExists patchDir then
      let
        dirContents = builtins.readDir patchDir;
        patchNames = builtins.filter
          (name: lib.hasSuffix ".patch" name)
          (builtins.attrNames dirContents);
      in
      map (name: patchDir + "/${name}") patchNames
    else
      [];

  # Directory containing custom extensions
  extensionsDir = ./prelude-extensions;
  hasExtensions = builtins.pathExists extensionsDir;

in
pkgs.runCommand "turnkey-prelude" {
  inherit upstreamPrelude;
  meta = {
    description = "Turnkey's customized Buck2 prelude";
    homepage = "https://github.com/firefly-engineering/turnkey";
  };
} ''
  # Copy upstream prelude
  cp -r $upstreamPrelude $out
  chmod -R u+w $out

  # Apply patches
  ${lib.concatMapStringsSep "\n" (p: ''
    echo "Applying patch: ${baseNameOf p}"
    patch -d $out -p1 < ${p}
  '') patchFiles}

  # Copy extensions (merged into prelude, can override files)
  ${lib.optionalString hasExtensions ''
    echo "Copying extensions from prelude-extensions/"
    cp -r ${extensionsDir}/* $out/ 2>/dev/null || true
  ''}

  echo "Turnkey prelude built successfully"
  echo "  Upstream: facebook/buck2-prelude@${rev}"
  echo "  Patches applied: ${toString (builtins.length patchFiles)}"
  ${lib.optionalString hasExtensions ''echo "  Extensions: included"''}
''
