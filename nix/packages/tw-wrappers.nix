# Transparent wrappers for native tools
#
# These wrapper scripts shadow the real tools (go, cargo, uv) in PATH,
# transparently invoking tw to enable auto-sync when dependency files change.
#
# The wrappers set TURNKEY_REAL_<TOOL> to the actual tool path, so tw
# can invoke the real tool without recursion.
#
# Usage: Add tw-go, tw-cargo, tw-uv to your shell packages. They will
# shadow the real tools, making `go get` automatically sync.
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
  # Go wrapper - shadows `go` command
  tw-go = mkWrapper { name = "go"; pkg = pkgs.go; };

  # Cargo wrapper - shadows `cargo` command
  tw-cargo = mkWrapper { name = "cargo"; pkg = pkgs.cargo; };

  # UV wrapper - shadows `uv` command
  tw-uv = mkWrapper { name = "uv"; pkg = pkgs.uv; };
}
