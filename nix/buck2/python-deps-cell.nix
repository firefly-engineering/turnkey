# Python dependencies cell builder
#
# Reads a python-deps.toml file and builds a Buck2 cell containing
# all Python package dependencies with rules.star files for python_library targets.
#
# The TOML file format:
#   [deps.package-name]
#   version = "1.0.0"
#   hash = "sha256-..."
#   url = "https://files.pythonhosted.org/packages/.../package-1.0.0.tar.gz"
#
# This is now a thin wrapper around the deps-cell library.

{ pkgs, lib, depsFile }:

let
  depsCell = import ../lib/deps-cell { inherit pkgs lib; };
in
depsCell.mkPythonDepsCell {
  inherit depsFile;
}
