{ turnkeyLib, devenvRoot }:

{
  lib,
  flake-parts-lib,
  ...
}:

let
  inherit (flake-parts-lib) mkPerSystemOption;
  inherit (lib) mkOption types;
  # turnkey's own flake lib. perSystem rebinds `turnkeyLib` to the teller lib,
  # so the pinned buck2 release is resolved through this name.
  turnkeyFlakeLib = turnkeyLib;
in
{
  options.perSystem = mkPerSystemOption (
    {
      config,
      pkgs,
      system,
      ...
    }:
    {
      options.turnkey.toolchains = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Enable turnkey toolchain management";
        };

        declarationFiles = mkOption {
          type = types.attrsOf types.path;
          default = { };
          example = lib.literalExpression ''
            {
              default = ./toolchain.toml;
              ci = ./toolchain.ci.toml;
            }
          '';
          description = ''
            Attribute set mapping shell names to toolchain declaration files.
            Each file will create a corresponding devenv shell.
            The shell name "default" maps to the default shell.
          '';
        };

        registry = mkOption {
          type = types.lazyAttrsOf types.anything;
          default = { };
          defaultText = "Default versioned registry from nix/registry";
          description = ''
            Complete registry override. When set, replaces the default registry entirely.
            Each entry should have: { versions = { "<ver>" = <pkg>; }; default = "<ver>"; }
            Prefer using registryExtensions to add packages without duplicating defaults.
          '';
        };

        registryExtensions = mkOption {
          type = types.lazyAttrsOf types.anything;
          default = { };
          example = lib.literalExpression ''
            {
              # Single-version entry
              beads = {
                versions = { "default" = inputs.beads.packages.''${system}.default; };
                default = "default";
              };
              # Or use the helper: turnkey.lib.single inputs.beads.packages.''${system}.default
            }
          '';
          description = ''
            Extend the default registry with additional toolchains.
            Each entry should have: { versions = { "<ver>" = <pkg>; }; default = "<ver>"; }
            These are merged on top of the default registry.
          '';
        };

        tellerLib = mkOption {
          type = types.anything;
          default = turnkeyLib.defaultTellerLib;
          defaultText = lib.literalExpression "inputs.turnkey.lib.defaultTellerLib";
          description = ''
            Teller library instance. Defaults to the teller flake input
            bundled with turnkey. Override only when using a non-default
            teller revision (e.g. a fork or a pinned commit).
          '';
        };

        tellerRegistry = mkOption {
          type = types.lazyAttrsOf types.anything;
          default = turnkeyLib.defaultTellerRegistry system;
          defaultText = lib.literalExpression "inputs.turnkey.lib.defaultTellerRegistry system";
          description = ''
            Versioned toolchain registry. Defaults to the toolbox-backed
            registry bundled with turnkey (nixpkgs + teller overlay +
            toolbox overlay). Override to point at a private registry
            overlay or an alternate toolbox revision.
          '';
        };

        wrapNativeTools = mkOption {
          type = types.bool;
          default = true;
          description = ''
            Automatically wrap native language tools (go, cargo, uv) with tw for auto-sync.
            When enabled, requesting 'go' in toolchain.toml gives you a wrapped version
            that automatically syncs dependency files when they change.

            Set to false to use unwrapped tools.
          '';
        };

        buck2 = mkOption {
          type = types.submoduleWith {
            modules = [
              (import ../../buck2/options.nix {
                inherit lib;
                inherit ((import ../../buck2/buck2-source.nix { inherit pkgs lib; })) version;
              })
              {
                options.shells = mkOption {
                  type = types.listOf types.str;
                  default = [ "default" ];
                  example = [
                    "default"
                    "ci"
                  ];
                  description = ''
                    Names of the shells (keys of `declarationFiles`) that get the
                    Buck2 integration when `buck2.enable` is set. Other shells get
                    only the toolchains their declaration file lists.
                  '';
                };
              }
            ];
          };
          default = { };
          description = "turnkey's Buck2 integration (options declared in nix/buck2/options.nix).";
        };
      };
    }
  );

  config.perSystem =
    {
      config,
      pkgs,
      system,
      inputs',
      ...
    }:
    let
      cfg = config.turnkey.toolchains;

      # Teller library (required)
      turnkeyLib = cfg.tellerLib;

      # Default registry from teller
      defaultRegistry = cfg.tellerRegistry;

      # Helper for single-version entries
      single = pkg: {
        versions = {
          "default" = pkg;
        };
        default = "default";
      };

      # Normalize a registry entry to versioned format
      # Handles both flat (buck2 = pkgs.buck2) and versioned ({ versions = ...; default = ...; }) formats
      normalizeEntry =
        entry:
        if entry ? versions && entry ? default then
          entry # Already versioned
        else
          single entry; # Flat package -> convert to versioned

      # Normalize all entries in a registry to versioned format
      normalizeRegistry = reg: builtins.mapAttrs (_name: normalizeEntry) reg;

      # Merge versioned registries (toolchain level merge, version level merge)
      mergeRegistries =
        base: extensions:
        let
          mergeToolchain =
            name: ext:
            let
              existing = base.${name} or null;
            in
            if existing == null then
              ext
            else
              {
                versions = (existing.versions or { }) // (ext.versions or { });
                default = if ext ? default then ext.default else existing.default;
              };
        in
        base // (builtins.mapAttrs mergeToolchain extensions);

      # Built-in turnkey tools that all consumers get automatically
      tk = import ../../packages/tk.nix {
        inherit pkgs lib;
        buck2 = pinnedBuck2;
      };
      builtinExtensions = {
        tk = single tk;
      };

      # Registry merging:
      # 1. Start with default registry (from teller)
      # 2. Merge built-in turnkey tools (tk)
      # 3. Merge user registryExtensions on top (versions are additive, default overrides)
      # 4. If registry is explicitly set (non-empty), use that as complete override
      # 5. Normalize all entries to versioned format (handles flat pkgs.foo entries)
      baseRegistry = normalizeRegistry (
        if cfg.registry != { } then
          # Complete override - user specified full registry
          cfg.registry
        else
          # Default + builtins + user extensions with proper merging
          mergeRegistries (mergeRegistries defaultRegistry builtinExtensions) cfg.registryExtensions
      );

      # The pinned buck2 release, from turnkey's own registry (never the
      # consumer's), and turnkey's patched prelude for it
      pinnedRelease = turnkeyFlakeLib.pinnedBuck2Release system;
      pinnedBuck2 = pinnedRelease.buck2;
      turnkeyPrelude = pinnedRelease.prelude;

      # Build tw for wrapping native tools
      tw = import ../../packages/tw.nix { inherit pkgs lib; };

      # Build wrapper packages for native tools
      # Each wrapper provides a binary with the same name as the tool (e.g., 'go')
      # but transparently invokes tw for auto-sync
      twWrappers = import ../../packages/tw-wrappers.nix { inherit pkgs lib tw; };

      # The native tools tw wraps: one per language that has a wrapper
      # (nix/buck2/languages.nix)
      wrappableTools = map (language: language.wrapper.tool) (
        builtins.filter (language: language ? wrapper) languages
      );

      # Wrap every version of a registry entry. A version is a package, or
      # an attrset whose `package` carries deprecation metadata alongside.
      wrapEntry =
        tool: entry:
        entry
        // {
          versions = builtins.mapAttrs (
            _version: versionEntry:
            if versionEntry ? package then
              versionEntry // { package = twWrappers.mkWrapper { name = tool; pkg = versionEntry.package; }; }
            else
              twWrappers.mkWrapper { name = tool; pkg = versionEntry; }
          ) entry.versions;
        };

      # Augment registry with wrappers when wrapNativeTools is enabled: each
      # wrapped tool keeps its versions, and every version shadows the
      # registry's own package
      registry =
        if cfg.wrapNativeTools then
          baseRegistry
          // lib.genAttrs (builtins.filter (tool: baseRegistry ? ${tool}) wrappableTools) (
            tool: wrapEntry tool baseRegistry.${tool}
          )
        else
          baseRegistry;

      # Resolve user patches directory (only if it exists)
      userPatchesDir =
        if cfg.buck2.tk.userPatchesDir != null && builtins.pathExists cfg.buck2.tk.userPatchesDir then
          cfg.buck2.tk.userPatchesDir
        else
          null;

      # The languages turnkey manages dependencies for (nix/buck2/languages.nix)
      languages = import ../../buck2/languages.nix { inherit pkgs lib; };

      # Each enabled language's cell: built from its deps file, or the cell
      # the consumer set. The deps file may not exist on first run, before
      # tk sync generates it.
      languageCells = lib.listToAttrs (
        map (
          language:
          let
            langCfg = cfg.buck2.${language.name};
          in
          lib.nameValuePair language.name (
            if !langCfg.enable then
              null
            else if langCfg.depsFile != null && builtins.pathExists langCfg.depsFile then
              language.mkCell { inherit langCfg userPatchesDir; }
            else
              langCfg.cell
          )
        ) languages
      );

      # Create a shell configuration for each declaration file
      mkShellConfig = shellName: declarationFile:
        let
          # Only the shells listed in buck2.shells get the Buck2 integration
          shellNeedsBuck2 = cfg.buck2.enable && builtins.elem shellName cfg.buck2.shells;
        in
        {
        imports = [ ../../devenv/turnkey ];

        # Read the devenv-root override (if set) so that CI can evaluate
        # devShells without an interactive shell. Empty content (the
        # default /dev/null placeholder) leaves devenv.root unset and
        # falls back to its usual direnv-driven resolution.
        devenv.root =
          let
            content = builtins.readFile devenvRoot.outPath;
          in
          lib.mkIf (content != "") content;

        # devenv builds its task runner from its own locked nixpkgs by
        # importing a fetched source at evaluation time, which
        # `nix flake check --no-build` can't do on a clean store. The devenv
        # flake exports the same runner as a package; use it when the flake
        # has a `devenv` input, so evaluation needs no build.
        task.package = lib.mkIf (inputs' ? devenv && inputs'.devenv.packages ? devenv-tasks) (
          lib.mkDefault inputs'.devenv.packages.devenv-tasks
        );

        turnkey = {
          registry = lib.mkDefault registry;
          declarationFile = declarationFile;
          tellerLib = lib.mkDefault turnkeyLib;

          # The Buck2 options as the consumer set them (nix/buck2/options.nix),
          # plus what this module resolves: whether this shell gets Buck2,
          # the pinned buck2, turnkey's prelude and the dependency cells.
          buck2 = builtins.removeAttrs cfg.buck2 [ "shells" "version" ] // {
            enable = shellNeedsBuck2;
            package = pinnedBuck2;
            prelude = cfg.buck2.prelude // {
              package = turnkeyPrelude;
            };
          }
          // lib.mapAttrs (name: cell: cfg.buck2.${name} // { inherit cell; }) languageCells;
        };
      };

      # Generate shell configurations from declarationFiles
      shellConfigs =
        let
          unknownShells = builtins.filter (name: !(cfg.declarationFiles ? ${name})) cfg.buck2.shells;
        in
        if cfg.buck2.enable && unknownShells != [ ] then
          throw "turnkey: turnkey.toolchains.buck2.shells names ${lib.concatStringsSep ", " unknownShells}, which declarationFiles doesn't define"
        else
          lib.mapAttrs mkShellConfig cfg.declarationFiles;

      # Collect all non-null cells into an attrset for exposure
      allCells = lib.filterAttrs (_: v: v != null && builtins.isPath v || lib.isDerivation v) (
        lib.listToAttrs (
          map (language: lib.nameValuePair language.cellName languageCells.${language.name}) languages
        )
        // {
          prelude = if cfg.buck2.prelude.path != null then cfg.buck2.prelude.path else turnkeyPrelude;
        }
      );

    in
    lib.mkIf cfg.enable {
      # Create all shells from declaration files
      devenv.shells = shellConfigs;

      # Expose cell derivations as packages so the composition daemon
      # can build them directly with `nix build .#<cell>-cell`
      packages = lib.mapAttrs' (name: drv:
        lib.nameValuePair "${name}-cell" drv
      ) allCells // {
        # Combined toolchain profile with all tools in bin/
        # The daemon exposes this as a virtual bin/ directory at the mount root
        toolchain-profile =
          let
            defaultDecl = cfg.declarationFiles.default or null;
            # What the default shell gets: its declared toolchains, and the
            # pinned buck2 when it has the Buck2 integration
            toolchainPackages =
              lib.optionals (defaultDecl != null) (
                (import ../../lib/toolchain-declaration.nix { inherit lib; }).resolve {
                  tellerLib = turnkeyLib;
                  inherit registry;
                  declarationFile = defaultDecl;
                }
              )
              ++ lib.optional (cfg.buck2.enable && builtins.elem "default" cfg.buck2.shells) pinnedBuck2;
          in
          pkgs.buildEnv {
            name = "turnkey-toolchain-profile";
            paths = toolchainPackages;
            passthru = { inherit toolchainPackages; };
            # Ignore collisions (multiple packages may provide the same binary name)
            ignoreCollisions = true;
          };
      };
    };
}
