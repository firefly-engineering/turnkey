# The toolchains cell for a shell's declared toolchains, built.
#
# The flake-parts module builds one per shell and hands it to the shell's
# devenv module (turnkey.buck2.toolchainsCell), which symlinks it at
# .turnkey/toolchains. It also exposes the default shell's as the
# toolchains-cell package, which the composition daemon builds like the
# other *-cell packages.
#
# The cell carries what the shell reads alongside it in passthru: the
# declared toolchains, the registry resolved at their versions, the cell's
# content (nix/buck2/toolchains-cell.nix) and where its .buckconfig expects
# the prelude cell.
{ pkgs, lib }:

{
  # The shell's toolchain.toml, or null
  declarationFile,
  # The versioned registry the shell resolves toolchains from
  registry,
  # Teller's library, which resolves a registry entry at a version
  tellerLib,
  # The Buck2 options (nix/buck2/options.nix)
  buck2,
}:

let
  mappings = import ./mappings.nix {
    inherit lib;
    mdbookPreprocessors = buck2.mdbook.preprocessors;
  };

  # Toolchain declarations (name -> spec, e.g. { version = "3"; }) from the declaration file
  declaredToolchains =
    if declarationFile != null then
      (import ../lib/toolchain-declaration.nix { inherit lib; }).toolchains declarationFile
    else
      { };

  # The registry resolved to packages, what mappings.nix's dynamicAttrs read
  # (e.g. ${registry.clang}/bin/clang). Declared toolchains resolve at their
  # declared version, so a path baked into the cell is the same package the
  # dev shell provides.
  resolvedRegistry = builtins.mapAttrs (
    name: _: tellerLib.resolveTool registry name (declaredToolchains.${name} or { })
  ) registry;

  content = import ./toolchains-cell.nix { inherit lib; } {
    inherit mappings declaredToolchains resolvedRegistry;
  };

  # The platforms the project builds for (nix/buck2/platforms.nix)
  platforms = import ./platforms.nix { inherit lib; };

  # Where the shell symlinks the prelude cell, which the cell's .buckconfig
  # names
  preludeCellPath = ".turnkey/prelude";
in
pkgs.runCommand "turnkey-toolchains-cell"
  {
    passthru = {
      inherit
        declaredToolchains
        resolvedRegistry
        content
        preludeCellPath
        ;
    };
  }
  ''
    mkdir -p $out

    # Create BUCK file (Buck2's buildfile name setting only applies to root cell)
    cat > $out/BUCK <<'BUCK'
    ${content.buckFile}
    BUCK

    # The combined <os>-<cpu> config_settings rules sync's select()s use
    # (nix/buck2/platforms.nix)
    mkdir -p $out/conditions
    cat > $out/conditions/BUCK <<'BUCK'
    ${platforms.settingsBuckFile (map platforms.fromSystem buck2.platforms) (
      lib.optionals buck2.go.enable buck2.go.allowedBuildTags
    )}
    BUCK

    # Create cell identity .buckconfig
    cat > $out/.buckconfig <<'BUCKCONFIG'
    [cells]
        toolchains = .
        prelude = ${preludeCellPath}
    BUCKCONFIG
  ''
