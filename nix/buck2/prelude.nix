# Turnkey Prelude - Nix-backed Buck2 prelude cell
#
# This derivation builds a customizable prelude by:
# 1. Taking an upstream buck2-prelude (from toolbox registry)
# 2. Applying patches from nix/patches/prelude/ (if any)
# 3. Copying extensions from nix/buck2/prelude-extensions/
#
# The result is symlinked to .turnkey/prelude in downstream projects.
{ pkgs, lib, upstreamPrelude }:

let
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
  echo "  Patches applied: ${toString (builtins.length patchFiles)}"
  ${lib.optionalString hasExtensions ''echo "  Extensions: included"''}
''
