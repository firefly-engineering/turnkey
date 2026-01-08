# Buck2 toolchain mappings
#
# Maps turnkey toolchain names to Buck2 toolchain rule specifications.
# Used by the buck2.nix module to generate the toolchains cell.
#
# Each toolchain can specify:
#   - targets: Buck2 toolchain rules to generate in the toolchains cell
#   - implicitDependencies: Other toolchains that must also be included
#   - runtimeDependencies: Packages needed in PATH for Buck2 action execution
#   - skip: true if this is not a Buck2 toolchain (just a dev tool)
#   - alwaysInclude: true if this toolchain should always be generated

{ lib }:

{
  # ==========================================================================
  # Language Toolchains
  # ==========================================================================

  go = {
    skip = false;
    targets = [
      {
        name = "go";
        rule = "system_go_toolchain";
        load = "@prelude//toolchains/go:system_go_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
      {
        name = "go_bootstrap";
        rule = "system_go_bootstrap_toolchain";
        load = "@prelude//toolchains/go:system_go_bootstrap_toolchain.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    # Go needs python for bootstrap scripts and cxx for linking
    implicitDependencies = [ "python" "cxx" ];
  };

  rust = {
    skip = false;
    targets = [
      {
        name = "rust";
        rule = "system_rust_toolchain";
        load = "@prelude//toolchains:rust.bzl";
        visibility = [ "PUBLIC" ];
        attrs = {
          default_edition = "2021";
        };
      }
    ];
    # Rust needs CXX for linking and python for build scripts
    implicitDependencies = [ "cxx" "python" ];
  };

  python = {
    skip = false;
    targets = [
      {
        name = "python_bootstrap";
        rule = "system_python_bootstrap_toolchain";
        load = "@prelude//toolchains:python.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    implicitDependencies = [ ];
  };

  cxx = {
    skip = false;
    targets = [
      {
        name = "cxx";
        rule = "system_cxx_toolchain";
        load = "@prelude//toolchains:cxx.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    implicitDependencies = [ ];
    # clang must be in PATH for Buck2 actions that use the cxx toolchain
    runtimeDependencies = [ "clang" ];
  };

  # ==========================================================================
  # Always-Included Toolchains
  # ==========================================================================

  # genrule is needed for most Buck2 builds
  genrule = {
    skip = false;
    alwaysInclude = true;
    targets = [
      {
        name = "genrule";
        rule = "system_genrule_toolchain";
        load = "@prelude//toolchains:genrule.bzl";
        visibility = [ "PUBLIC" ];
      }
    ];
    implicitDependencies = [ ];
  };

  # ==========================================================================
  # Non-Buck2 Tools (skipped)
  # ==========================================================================

  # Buck2 itself - not a language toolchain
  buck2 = {
    skip = true;
    reason = "Buck2 binary itself, not a language toolchain";
  };

  # Nix package manager
  nix = {
    skip = true;
    reason = "Nix package manager, not a Buck2 toolchain";
  };

  # Development tools
  beads = {
    skip = true;
    reason = "Issue tracking tool";
  };

  beads_viewer = {
    skip = true;
    reason = "Issue visualization tool";
  };

  jj = {
    skip = true;
    reason = "Jujutsu VCS tool";
  };

  clang = {
    skip = true;
    reason = "C/C++ compiler, needed in PATH for Buck2 actions but not a toolchain rule";
  };
}
