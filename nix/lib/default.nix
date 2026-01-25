# Turnkey library functions
{
  lib,
  pkgs,
  currentTime ? null,
}:

let
  # Check if deprecation warnings are suppressed via environment variable
  # In pure mode, builtins.getEnv returns "" so warnings are shown by default
  suppressWarnings = builtins.getEnv "TURNKEY_NO_DEPRECATION_WARNINGS" != "";

  # Extract the package derivation from a version entry.
  # Handles both formats:
  #   - Plain derivation: "1.22" = pkgs.go_1_22
  #   - Extended format:  "1.22" = { package = pkgs.go_1_22; deprecated = true; ... }
  extractPackage =
    versionEntry: if versionEntry ? package then versionEntry.package else versionEntry;

  # Check if a version entry is deprecated or past EOL.
  # Returns null if no warning needed, otherwise returns the warning message.
  checkDeprecation =
    name: version: versionEntry:
    let
      isExtended = versionEntry ? package;
      deprecated = isExtended && (versionEntry.deprecated or false);
      deprecationMessage = versionEntry.deprecationMessage or null;
      eol = versionEntry.eol or null;

      # Check if EOL date has passed (ISO 8601 format: "2025-02-01")
      # Uses passed-in currentTime (e.g. self.lastModified) or builtins.currentTime
      # Convert to YYYY-MM-DD string for comparison
      currentDate =
        let
          # Use provided currentTime, or fallback to builtins.currentTime (impure only), or 0
          t =
            if currentTime != null && currentTime != 0 then
              currentTime
            else if builtins ? currentTime then
              builtins.currentTime
            else
              0;
          # Days since epoch
          days = t / 86400;
          # Approximate year/month/day calculation
          # Good enough for EOL comparison (off by at most a day)
          y400 = days / 146097; # 400-year cycles
          d400 = days - y400 * 146097;
          y100 = lib.min 3 (d400 / 36524); # 100-year cycles within 400
          d100 = d400 - y100 * 36524;
          y4 = d100 / 1461; # 4-year cycles
          d4 = d100 - y4 * 1461;
          y1 = lib.min 3 (d4 / 365); # years within 4-year cycle
          year = 1970 + y400 * 400 + y100 * 100 + y4 * 4 + y1;
          dayOfYear = d4 - y1 * 365;
          # Approximate month (good enough for date comparison)
          month = (dayOfYear / 30) + 1;
          day = (dayOfYear - (month - 1) * 30) + 1;
          pad2 = n: if n < 10 then "0${toString n}" else toString n;
        in
        "${toString year}-${pad2 month}-${pad2 day}";

      # ISO 8601 dates are lexicographically sortable
      eolPassed = eol != null && eol < currentDate;

      warningParts =
        lib.optional deprecated "DEPRECATED: Toolchain '${name}' version '${version}' is deprecated."
        ++ lib.optional (deprecated && deprecationMessage != null) "  ${deprecationMessage}"
        ++ lib.optional eolPassed "EOL: Toolchain '${name}' version '${version}' reached end-of-life on ${eol}.";

    in
    if warningParts == [ ] || suppressWarnings then
      null
    else
      builtins.concatStringsSep "\n" warningParts;

  # Apply warning to a package if needed
  warnIfNeeded =
    name: version: versionEntry:
    let
      pkg = extractPackage versionEntry;
      warning = checkDeprecation name version versionEntry;
    in
    if warning == null then pkg else lib.warn warning pkg;

in
{
  # ===========================================================================
  # Registry Helpers
  # ===========================================================================

  # Create a registry overlay with two-level merging:
  # - Toolchain level: new toolchains added, existing merged
  # - Version level: versions combined additively, default overridden
  #
  # Usage:
  #   overlays.default = turnkey.lib.mkRegistryOverlay (final: prev: {
  #     go = {
  #       versions = { "1.23" = final.go_1_23; };
  #       default = "1.23";
  #     };
  #   });
  mkRegistryOverlay =
    packagesFn: final: prev:
    let
      prevRegistry = prev.turnkeyRegistry or { };
      newPackages = packagesFn final prev;

      # Merge a single toolchain: combine versions, override default
      mergeToolchain =
        name: new:
        let
          existing = prevRegistry.${name} or null;
        in
        if existing == null then
          new
        else
          {
            versions = (existing.versions or { }) // (new.versions or { });
            default = if new ? default then new.default else existing.default;
          };
    in
    {
      turnkeyRegistry = prevRegistry // (builtins.mapAttrs mergeToolchain newPackages);
    };

  # Create a meta-package combining multiple components into a single derivation.
  # All component binaries end up in $out/bin, available in PATH.
  #
  # Usage:
  #   rust = turnkey.lib.mkMetaPackage {
  #     name = "rust-1.80";
  #     components = {
  #       rustc = final.rustc;
  #       cargo = final.cargo;
  #       clippy = final.clippy;
  #     };
  #   };
  mkMetaPackage =
    { name, components }:
    pkgs.symlinkJoin {
      inherit name;
      paths = builtins.attrValues components;
      passthru = {
        inherit components;
      }
      // components;
    };

  # Resolve a toolchain from a versioned registry.
  # Returns the derivation for the specified (or default) version.
  #
  # Supports both plain derivation entries and extended entries with metadata:
  #   versions = {
  #     "1.23" = pkgs.go_1_23;                    # Plain
  #     "1.22" = {                                 # Extended with metadata
  #       package = pkgs.go_1_22;
  #       deprecated = true;
  #       deprecationMessage = "Use 1.23 instead";
  #       eol = "2025-02-01";
  #     };
  #   };
  #
  # Emits warnings (via lib.warn) for deprecated or EOL versions.
  # Set TURNKEY_NO_DEPRECATION_WARNINGS=1 to suppress warnings.
  #
  # Usage:
  #   go = turnkey.lib.resolveTool registry "go" { version = "1.22"; };
  #   rust = turnkey.lib.resolveTool registry "rust" {};  # uses default
  resolveTool =
    registry: name: spec:
    let
      entry = registry.${name} or (throw "Unknown toolchain: ${name}");
      version = spec.version or entry.default;
      availableVersions = builtins.attrNames entry.versions;
      versionEntry =
        entry.versions.${version} or (throw ''
          Version '${version}' of toolchain '${name}' is not available.

          Available versions for '${name}':
            ${builtins.concatStringsSep "\n    " (
              map (v: if v == entry.default then "- ${v} (default)" else "- ${v}") availableVersions
            )}
        '');
    in
    warnIfNeeded name version versionEntry;

  # Resolve all toolchains from a toolchain.toml declaration.
  # Returns a list of packages.
  #
  # Supports extended version entries with deprecation/EOL metadata.
  # See resolveTool for details on the metadata format.
  #
  # Usage:
  #   packages = turnkey.lib.resolveToolchains registry toolchainDeclaration;
  resolveToolchains =
    registry: declaration:
    let
      toolchains = declaration.toolchains or { };
      resolveOne =
        name: spec:
        let
          entry = registry.${name} or (throw "Unknown toolchain '${name}' in toolchain.toml");
          version = spec.version or entry.default;
          availableVersions = builtins.attrNames entry.versions;
          versionEntry =
            entry.versions.${version} or (throw ''
              Version '${version}' of toolchain '${name}' is not available.

              Available versions for '${name}':
                ${builtins.concatStringsSep "\n    " (
                  map (v: if v == entry.default then "- ${v} (default)" else "- ${v}") availableVersions
                )}

              Requested in: toolchain.toml
            '');
        in
        warnIfNeeded name version versionEntry;
    in
    lib.mapAttrsToList resolveOne toolchains;
}
