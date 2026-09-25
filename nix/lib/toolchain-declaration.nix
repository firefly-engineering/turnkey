# Resolving a toolchain.toml declaration to packages
#
# The one place a declaration file becomes packages, for every shell and for
# the toolchain profile. buck2 is turnkey's, never a toolchain to declare
# (docs/adr/0002-turnkey-owns-the-buck2-version.md), so declaring it is an
# error here rather than a package from the consumer's registry.
#
# Usage:
#   let
#     toolchainDeclaration = import ./toolchain-declaration.nix { inherit lib; };
#   in
#   toolchainDeclaration.resolve { inherit tellerLib registry declarationFile; }
#   toolchainDeclaration.toolchains declarationFile  # { go = { version = "3"; }; ... }
#
{ lib }:

rec {
  # The declared toolchains, name -> spec (e.g. { version = "3"; }), with
  # the same checks as resolve
  toolchains = declarationFile: (read declarationFile).toolchains or { };

  resolve =
    {
      tellerLib,
      registry,
      declarationFile,
    }:
    tellerLib.resolveToolchains registry (read declarationFile);

  # The parsed declaration file, or an error if it declares buck2
  read =
    declarationFile:
    let
      declaration = builtins.fromTOML (builtins.readFile declarationFile);
      declaredBuck2 = builtins.filter (name: (declaration.toolchains or { }) ? ${name}) [
        "buck2"
        "buck2-toolchain"
      ];
    in
    if declaredBuck2 != [ ] then
      throw ''
        turnkey: ${toString declarationFile} declares ${lib.concatStringsSep " and " declaredBuck2}, but turnkey now ships buck2 itself.
        Remove the entry; turnkey.toolchains.buck2.shells chooses which shells get buck2.
        See https://github.com/firefly-engineering/turnkey/blob/main/docs/user-manual/src/configuration/buck2-integration.md#the-buck2-version''
    else
      declaration;
}
