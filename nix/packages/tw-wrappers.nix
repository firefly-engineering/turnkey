# Transparent wrappers for native tools
#
# These wrapper scripts shadow the real tools (go, cargo, uv) in PATH,
# transparently invoking tw to enable auto-sync when dependency files change.
#
# The wrappers set TURNKEY_REAL_<TOOL> to the actual tool path, so tw
# can invoke the real tool without recursion.
#
# The flake-parts module wraps the registry's own packages with mkWrapper,
# so the wrapped tool is the one toolchain.toml resolves. tw-go, tw-cargo
# and tw-uv wrap nixpkgs' tools, for use outside a turnkey shell.
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

in {
  inherit mkWrapper;

  # Go wrapper - shadows `go` command
  tw-go = mkWrapper { name = "go"; pkg = pkgs.go; };

  # Cargo wrapper - shadows `cargo` command
  tw-cargo = mkWrapper { name = "cargo"; pkg = pkgs.cargo; };

  # UV wrapper - shadows `uv` command
  tw-uv = mkWrapper { name = "uv"; pkg = pkgs.uv; };
}
