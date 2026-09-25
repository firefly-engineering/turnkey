# Pre-commit hooks for a shell with the Buck2 integration.
#
# Each hook is switched on by a buck2.tk option (nix/buck2/options.nix),
# except the syntax checks, which always run.
{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.turnkey.buck2;

  # Pre-commit check tools (Rust implementations with tree-sitter parsing)
  checkSourceCoverageRs = import ../../packages/check-source-coverage-rs.nix { inherit pkgs lib; };
  checkRustEditionRs = import ../../packages/check-rust-edition-rs.nix { inherit pkgs lib; };
in
{
  config = lib.mkIf (cfg.enable && config.turnkey.enable) {
    # Pre-commit hooks
    git-hooks.hooks = {
      # tk check - verify rules.star files and deps are in sync
      turnkey-check = lib.mkIf cfg.tk.preCommitCheck {
        enable = true;
        name = "turnkey-check";
        description = "Check that rules.star files and deps are in sync";
        # Run on any file change - tk check uses its own staleness detection
        always_run = true;
        pass_filenames = false;
        entry = ''
          sh -c '
            if command -v tk >/dev/null 2>&1; then
              tk check || {
                echo ""
                echo "Files out of sync. Run: tk sync"
                exit 1
              }
            else
              echo "Warning: tk not found in PATH, skipping sync check"
            fi
          '
        '';
      };

      # Rust edition alignment check (uses tree-sitter for proper Starlark parsing)
      rust-edition-check = lib.mkIf (cfg.rust.enable && cfg.tk.rustEditionCheck) {
        enable = true;
        name = "rust-edition-check";
        description = "Check Rust edition alignment between Cargo.toml and rules.star";
        files = "(Cargo\\.toml|rules\\.star)$";
        pass_filenames = false;
        entry = ''
          ${checkRustEditionRs}/bin/check-rust-edition-rs
        '';
      };

      # Monorepo dependency rules check
      monorepo-dep-check = lib.mkIf cfg.tk.monorepoDepCheck {
        enable = true;
        name = "monorepo-dep-check";
        description = "Check monorepo dependency rules (all languages)";
        files = "(go\\.mod|Cargo\\.toml|pyproject\\.toml|package\\.json)$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-monorepo-deps/__main__.py
        '';
      };

      # JS/TS tool config check (buck-out exclusions)
      js-test-config-check = lib.mkIf (cfg.javascript.enable && cfg.tk.jsTestConfigCheck) {
        enable = true;
        name = "js-test-config-check";
        description = "Check Jest/Vitest/Biome configs exclude buck-out directories";
        files = "(jest\\.config\\.(js|ts|mjs|cjs)|vitest\\.config\\.(js|ts|mjs|mts)|biome\\.jsonc?|package\\.json)$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-js-test-config/__main__.py
        '';
      };

      # Foundry configuration consistency check
      foundry-config-check = lib.mkIf (cfg.solidity.enable && cfg.tk.foundryConfigCheck) {
        enable = true;
        name = "foundry-config-check";
        description = "Check Foundry config consistency (solc version, dependencies)";
        files = "foundry\\.toml$";
        pass_filenames = false;
        entry = ''
          ${pkgs.python3}/bin/python src/cmd/check-foundry-config/__main__.py
        '';
      };

      # Source coverage check - validate all source files covered by Buck2 targets
      # Uses tree-sitter for proper Starlark parsing
      source-coverage-check = lib.mkIf cfg.tk.sourceCoverageCheck {
        enable = true;
        name = "source-coverage-check";
        description = "Check all source files are covered by Buck2 targets";
        files = "(\\.go|\\.rs|\\.py|\\.ts|\\.tsx|\\.js|\\.jsx|\\.sol|rules\\.star)$";
        pass_filenames = false;
        entry = ''
          ${checkSourceCoverageRs}/bin/check-source-coverage-rs --scope "${cfg.tk.sourceScope}"
        '';
      };

      # Nix flake check
      # Note: Disabled by default because devenv's container outputs require
      # nix2container input which may not be present in all projects.
      # Enable in your flake if you want this check.
      nix-flake-check = {
        enable = false;
        name = "nix-flake-check";
        description = "Check Nix flake validity";
        files = "\\.nix$";
        pass_filenames = false;
        entry = ''
          nix flake check --no-build --impure
        '';
      };

      # Starlark syntax validation using Buck2
      starlark-lint = {
        enable = true;
        name = "starlark-lint";
        description = "Lint Starlark files (rules.star, BUCK, etc.)";
        files = "(rules\\.star|BUCK|\\.bzl)$";
        pass_filenames = true;
        entry = ''
          ${cfg.package}/bin/buck2 starlark lint
        '';
      };

      # TOML syntax validation
      toml-syntax-check = {
        enable = true;
        name = "toml-syntax-check";
        description = "Check TOML syntax validity";
        files = "\\.toml$";
        # Exclude devenv/turnkey state directories and lock files
        excludes = [ "^\\.devenv/" "^\\.turnkey/" "^buck-out/" ];
        pass_filenames = true;
        entry = ''
          ${pkgs.python3}/bin/python -c '
import tomllib
import sys
errors = 0
for path in sys.argv[1:]:
    try:
        with open(path, "rb") as f:
            tomllib.load(f)
    except Exception as e:
        print(f"TOML syntax error in {path}: {e}", file=sys.stderr)
        errors += 1
if errors:
    sys.exit(1)
'
        '';
      };

      # JSON syntax validation
      json-syntax-check = {
        enable = true;
        name = "json-syntax-check";
        description = "Check JSON syntax validity";
        files = "\\.json$";
        # Exclude devenv/turnkey state directories
        excludes = [ "^\\.devenv/" "^\\.turnkey/" "^buck-out/" ];
        pass_filenames = true;
        entry = ''
          ${pkgs.python3}/bin/python -c '
import json
import sys
errors = 0
for path in sys.argv[1:]:
    try:
        with open(path, "r") as f:
            json.load(f)
    except Exception as e:
        print(f"JSON syntax error in {path}: {e}", file=sys.stderr)
        errors += 1
if errors:
    sys.exit(1)
'
        '';
      };
    };
  };
}
