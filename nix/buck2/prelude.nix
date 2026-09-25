# Turnkey Prelude - Nix-backed Buck2 prelude cell
#
# This derivation builds a customizable prelude by:
# 1. Taking the upstream buck2-prelude of the pinned buck2 release
#    (nix/buck2/buck2-source.nix)
# 2. Applying the patch set for that upstream version, from
#    nix/patches/prelude/<version>/
# 3. Copying extensions from nix/buck2/prelude-extensions/
#
# Patches are written against one upstream version, so the set is keyed by
# it. turnkey's test result caching depends on them: an upstream version
# without a set is an error, not an unpatched prelude.
#
# The result is symlinked to .turnkey/prelude in downstream projects.
{
  pkgs,
  lib,
  upstreamPrelude,
  # toolbox's buck2-prelude packages carry their version (a release date)
  version ? upstreamPrelude.version or null,
}:

let
  # The patch set for this upstream version
  patchDir = ../patches/prelude + "/${toString version}";

  # All .patch files in it, applied in name order
  patchFiles =
    if version != null && builtins.pathExists patchDir then
      let
        patchNames = builtins.filter (name: lib.hasSuffix ".patch" name) (
          builtins.attrNames (builtins.readDir patchDir)
        );
      in
      map (name: patchDir + "/${name}") (lib.sort (a: b: a < b) patchNames)
    else
      throw "turnkey: no prelude patch set for buck2-prelude ${toString version} (nix/patches/prelude/${toString version}/)";

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
