# Fixup Registry API
#
# Provides functions to look up and manage fixups for dependencies.
# Fixups are organized by language: fixups/rust/, fixups/go/, fixups/python/
#
# Fixups can be keyed by:
#   - name@version (e.g., "serde@1.0.219")
#   - name (e.g., "serde") - applies to all versions

{ pkgs, lib }:

let
  # Import language-specific fixups
  rustFixups = import ./rust { inherit pkgs lib; };
  goFixups = import ./go { inherit pkgs lib; };
  pythonFixups = import ./python { inherit pkgs lib; };
in
rec {
  # All built-in fixups by language
  builtinFixups = {
    rust = rustFixups;
    go = goFixups;
    python = pythonFixups;
  };

  # Get fixup for a dependency
  # Lookup order: version-specific key -> name -> null
  getFixup = {
    language,
    name,
    version,
    userFixups ? {},
  }:
  let
    defaults = builtinFixups.${language} or {};
    merged = defaults // userFixups;
    versionedKey = "${name}@${version}";
  in
  merged.${versionedKey} or merged.${name} or null;

  # Check if a fixup exists for a dependency
  hasFixup = args: (getFixup args) != null;

  # Merge built-in fixups with user-provided fixups
  mergeFixups = { language, userFixups ? {} }:
    (builtinFixups.${language} or {}) // userFixups;

  # Helper to create a fixup entry
  # commands can be:
  #   - A string of shell commands
  #   - A function: context -> string
  mkFixup = { name, version ? null, commands }:
    let
      key = if version != null then "${name}@${version}" else name;
    in
    { ${key} = commands; };

  # Export language-specific fixups for direct access
  inherit rustFixups goFixups pythonFixups;
}
