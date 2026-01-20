# Turnkey library functions
{ lib, pkgs }:

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
  mkRegistryOverlay = packagesFn: final: prev:
    let
      prevRegistry = prev.turnkeyRegistry or {};
      newPackages = packagesFn final prev;

      # Merge a single toolchain: combine versions, override default
      mergeToolchain = name: new:
        let
          existing = prevRegistry.${name} or null;
        in
          if existing == null then new
          else {
            versions = (existing.versions or {}) // (new.versions or {});
            default = if new ? default then new.default else existing.default;
          };
    in {
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
  mkMetaPackage = { name, components }:
    pkgs.symlinkJoin {
      inherit name;
      paths = builtins.attrValues components;
      passthru = {
        inherit components;
      } // components;
    };

  # Resolve a toolchain from a versioned registry.
  # Returns the derivation for the specified (or default) version.
  #
  # Usage:
  #   go = turnkey.lib.resolveTool registry "go" { version = "1.22"; };
  #   rust = turnkey.lib.resolveTool registry "rust" {};  # uses default
  resolveTool = registry: name: spec:
    let
      entry = registry.${name}
        or (throw "Unknown toolchain: ${name}");
      version = spec.version or entry.default;
      availableVersions = builtins.attrNames entry.versions;
      pkg = entry.versions.${version}
        or (throw ''
          Version '${version}' of toolchain '${name}' is not available.

          Available versions for '${name}':
            ${builtins.concatStringsSep "\n    " (map (v:
              if v == entry.default then "- ${v} (default)" else "- ${v}"
            ) availableVersions)}
        '');
    in pkg;

  # Resolve all toolchains from a toolchain.toml declaration.
  # Returns a list of packages.
  #
  # Usage:
  #   packages = turnkey.lib.resolveToolchains registry toolchainDeclaration;
  resolveToolchains = registry: declaration:
    let
      toolchains = declaration.toolchains or {};
      resolveOne = name: spec:
        let
          entry = registry.${name}
            or (throw "Unknown toolchain '${name}' in toolchain.toml");
          version = spec.version or entry.default;
          availableVersions = builtins.attrNames entry.versions;
        in
          entry.versions.${version}
            or (throw ''
              Version '${version}' of toolchain '${name}' is not available.

              Available versions for '${name}':
                ${builtins.concatStringsSep "\n    " (map (v:
                  if v == entry.default then "- ${v} (default)" else "- ${v}"
                ) availableVersions)}

              Requested in: toolchain.toml
            '');
    in
      lib.mapAttrsToList resolveOne toolchains;
}
