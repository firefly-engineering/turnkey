# Python Fixups Registry
#
# Fixups for Python packages that require special handling during build.
# Python packages rarely need fixups for pure Python code.
#
# Each fixup is a function: context -> string (shell commands)
# Context includes: { name, version, vendorPath, ... }

{ pkgs, lib }:

{
  # No Python fixups currently required
  # Add entries here if specific packages need patching
}
