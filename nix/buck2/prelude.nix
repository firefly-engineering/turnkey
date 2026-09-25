# Turnkey Prelude - Nix-backed Buck2 prelude cell
#
# This derivation builds a customizable prelude by:
# 1. Taking the upstream buck2-prelude of the pinned buck2 release
#    (nix/buck2/buck2-source.nix, which is the only caller)
# 2. Applying turnkey's patch set, nix/patches/prelude/*.patch
# 3. Copying extensions from nix/buck2/prelude-extensions/
#
# The patches are written against the pinned release's prelude; a bump ports
# them in place. turnkey's test result caching depends on them.
#
# The result is symlinked to .turnkey/prelude in downstream projects.
{
  pkgs,
  lib,
  upstreamPrelude,
}:

let
  # turnkey's patch set, applied in name order
  patchDir = ../patches/prelude;
  patchFiles = map (name: patchDir + "/${name}") (
    lib.sort (a: b: a < b) (
      builtins.filter (name: lib.hasSuffix ".patch" name) (builtins.attrNames (builtins.readDir patchDir))
    )
  );

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
