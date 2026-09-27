# Transparent wrappers for native tools
#
# These wrapper scripts shadow the real tools (go, cargo, uv) in PATH,
# transparently invoking tw to enable auto-sync when dependency files change.
#
# The wrappers set TURNKEY_REAL_<TOOL> to the actual tool path, so tw
# can invoke the real tool without recursion.
#
# The flake-parts module wraps the registry's own packages with mkWrapper,
# so the wrapped tool is the one toolchain.toml resolves. tw-<tool> wraps
# nixpkgs' <tool>, for use outside a turnkey shell: one per native tool a
# language record names (nix/buck2/languages.nix), so tw-go, tw-cargo and
# tw-uv.
{ pkgs, lib, tw }:

let
  # Create a wrapper script that shadows a tool
  # The wrapper exports TURNKEY_REAL_<TOOL> so tw can find the real binary
  mkWrapper = { name, pkg }: pkgs.writeShellScriptBin name ''
    # If TURNKEY_NO_WRAP is set, bypass tw and use the real tool
    if [ -n "''${TURNKEY_NO_WRAP:-}" ]; then
      exec "${pkg}/bin/${name}" "$@"
    fi
    # Tell tw where the real tool is (avoids infinite recursion)
    export TURNKEY_REAL_${lib.toUpper name}="${pkg}/bin/${name}"
    exec "${tw}/bin/tw" "${name}" "$@"
  '';

  # The native tools tw wraps: one per language that has a wrapper
  # (nix/buck2/languages.nix)
  languages = import ../buck2/languages.nix { inherit pkgs lib; };
  tools = map (language: language.wrapper.tool) (
    builtins.filter (language: language ? wrapper) languages
  );

in
{
  inherit mkWrapper tools;

  # tw-<tool> for each tool, wrapping nixpkgs' <tool>
  packages = lib.listToAttrs (
    map (
      tool:
      lib.nameValuePair "tw-${tool}" (mkWrapper {
        name = tool;
        pkg = pkgs.${tool};
      })
    ) tools
  );
}
