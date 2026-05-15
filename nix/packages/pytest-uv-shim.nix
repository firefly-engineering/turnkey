# pytest shim that routes through `uv run`
#
# The Nix Python toolchain bundles its own pytest, but that binary runs
# under the Nix-store interpreter and can't see the project's editable
# uv-workspace installs. Without intervention, a bare `pytest` invocation
# in the dev shell fails with ModuleNotFoundError for workspace packages.
#
# This shim shadows `pytest` and delegates to `uv run pytest`, which
# transparently:
#   - finds the workspace root (uv walks up from CWD for pyproject.toml),
#   - ensures the project venv is in sync with uv.lock,
#   - launches pytest from that venv (so `turnkey.*` and other workspace
#     members resolve through their editable installs).
#
# Bypass with TURNKEY_NO_PYTEST_SHIM=1 if a user needs the raw pytest.
{ pkgs, lib }:

pkgs.writeShellApplication {
  name = "pytest";
  runtimeInputs = [ pkgs.uv ];
  text = ''
    if [ -n "''${TURNKEY_NO_PYTEST_SHIM:-}" ]; then
      # Fall back to whichever pytest is next on PATH (e.g. python-toolchain's).
      shim_path="$(command -v pytest)"
      next_pytest="$(PATH="''${PATH#*:}" command -v pytest || true)"
      if [ -n "$next_pytest" ] && [ "$next_pytest" != "$shim_path" ]; then
        exec "$next_pytest" "$@"
      fi
      echo "pytest shim: TURNKEY_NO_PYTEST_SHIM set but no other pytest found on PATH" >&2
      exit 127
    fi
    exec uv run pytest "$@"
  '';
}
