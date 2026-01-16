# Go Fixups Registry
#
# Fixups for Go modules that require special handling during build.
# Go modules rarely need fixups since they don't have build scripts.
#
# Each fixup is a function: context -> string (shell commands)
# Context includes: { importPath, version, vendorPath, ... }

{ pkgs, lib }:

{
  # No Go fixups currently required
  # Add entries here if specific modules need patching
}
