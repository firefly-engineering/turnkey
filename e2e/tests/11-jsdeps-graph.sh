#!/usr/bin/env bash
# E2E Test: jsdeps package graph fixture
#
# Runs the whole JavaScript path on the jsdeps-graph fixture (see its
# README.md): pnpm-lock.yaml -> tk sync -> jsdeps cell -> rules check ->
# buck2 build/test, with tsc and node, for ES module and CommonJS code.
# 1. Initialize from the turnkey template and add the fixture, with a
#    flake reading its local packages' tarballs from the project
# 2. tk sync writes js-deps.toml from the committed lock
# 3. The cell is a real directory, GC-rooted, and rules sync agrees with
#    the fixture's npm_deps
# 4. Build everything (tsc type-checks every assertion, and fails if an
#    undeclared transitive dep resolves), run both programs (node checks
#    them at runtime), and test twice: the second run reuses both results
#
# Issue: https://github.com/firefly-engineering/turnkey/issues/232
set -euo pipefail

source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: jsdeps package graph fixture"

step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "jsdeps-graph")
cd "$PROJECT_DIR"

step "Initializing from turnkey template"
init_from_template

step "Enabling JavaScript in flake.nix"
turnkey_path=$(grep 'turnkey.url' flake.nix | sed 's/.*"\(.*\)".*/\1/')
cat > flake.nix << EOF
{
  description = "jsdeps package graph fixture";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devenv.url = "github:cachix/devenv";
    turnkey.url = "${turnkey_path}";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
        inputs.turnkey.flakeModules.turnkey
      ];

      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      perSystem = { lib, ... }: {
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;

          buck2 = {
            enable = true;

            go.enable = false;

            javascript = {
              enable = true;
              depsFile = ./js-deps.toml;
              # The @types packages are devDependencies
              includeDevDependencies = true;
              # The local packages, as the lock records the static
              # registry's URLs for them (registry/serve.mjs)
              tarballs = lib.mapAttrs' (
                name: _: lib.nameValuePair "http://localhost:4873/-/\${name}" (./registry/tarballs + "/\${name}")
              ) (builtins.readDir ./registry/tarballs);
            };
          };
        };
      };
    };
}
EOF

step "Adding the jsdeps-graph fixture"
copy_fixture "jsdeps-graph"
cat > toolchain.toml << 'EOF'
[toolchains]
nodejs = {}
typescript = {}
EOF
stage_for_flake

# Phase 1: js-deps.toml from the committed lock. It doesn't exist yet, so
# the shell has no jsdeps cell.
step "Writing js-deps.toml (batched)"
run_in_devshell_script << 'PHASE1'
  tk sync
PHASE1

step "Verifying js-deps.toml"
assert_file_exists "js-deps.toml" || exit 1
# The peer split, the cycle and both versions of a name, as instances
assert_file_contains "js-deps.toml" 'key = "@tkfixture/plugin@1.0.0(@tkfixture/host@1.0.0)"' || exit 1
assert_file_contains "js-deps.toml" 'key = "@tkfixture/plugin@1.0.0(@tkfixture/host@2.0.0)"' || exit 1
assert_file_contains "js-deps.toml" '"@tkfixture/cyc-a" = "@tkfixture/cyc-a@1.0.0"' || exit 1
assert_file_contains "js-deps.toml" 'key = "picomatch@2.3.2"' || exit 1
assert_file_contains "js-deps.toml" 'key = "picomatch@4.0.2"' || exit 1
# Only the root's dependencies are direct ([direct] is the file's last table)
direct=$(sed -n '/^\[direct\]/,$p' js-deps.toml)
assert_output_contains 'printf "%s\n" "$direct"' '^"@types/node" = "@types/node@24.10.1"' || exit 1
assert_output_not_contains 'printf "%s\n" "$direct"' '^braces' || exit 1

step "Committing js-deps.toml"
stage_for_flake
commit_changes "Write js-deps.toml"

# Phase 2: the cell, rules sync's view of the fixture, then build and test
step "Building and testing (batched)"
run_output=$(run_in_devshell_script_capture << 'PHASE2'
  echo "=== cell ==="
  test -d .turnkey/jsdeps && ! test -L .turnkey/jsdeps && echo "jsdeps is a directory"
  test -L .turnkey/gcroots/jsdeps && echo "jsdeps is rooted"
  ls .turnkey/jsdeps/vendor/@tkfixture

  echo ""
  echo "=== rules check ==="
  tk rules check

  echo ""
  echo "=== build ==="
  tk build //...

  echo ""
  echo "=== esm ==="
  tk run //app:esm

  echo ""
  echo "=== cjs ==="
  tk run //app:cjs

  echo ""
  echo "=== first test run ==="
  # --rerun: results recorded by an earlier run of this test, on the same
  # inputs, would otherwise make this one a hit too
  tk --rerun test //...

  echo ""
  echo "=== second test run ==="
  tk test //...
PHASE2
) || {
  echo "$run_output" | tail -60
  exit 1
}
echo "$run_output" | tail -40

step "Verifying the cell"
assert_output_contains 'printf "%s\n" "$run_output"' "^jsdeps is a directory" || exit 1
assert_output_contains 'printf "%s\n" "$run_output"' "^jsdeps is rooted" || exit 1
assert_output_contains 'printf "%s\n" "$run_output"' "^plugin@1.0.0" || exit 1

step "Verifying both programs ran under node"
assert_output_contains 'printf "%s\n" "$run_output"' "^esm: OK" || exit 1
assert_output_contains 'printf "%s\n" "$run_output"' "^cjs: OK" || exit 1

step "Verifying both test runs"
# The second run reuses both results: a test reads only declared inputs
first_run=$(echo "$run_output" | sed -n '/=== first test run ===/,/=== second test run ===/p')
second_run=$(echo "$run_output" | sed -n '/=== second test run ===/,$p')
assert_output_contains 'printf "%s\n" "$first_run"' "Pass 2.*Fail 0\. Timeout 0\. Fatal 0\." || exit 1
assert_output_contains 'printf "%s\n" "$second_run"' "Pass 2.*Fail 0\. Timeout 0\. Fatal 0\." || exit 1
assert_output_contains 'printf "%s\n" "$second_run"' "^2 recorded (reused without running)" || exit 1

section "PASS: jsdeps package graph fixture"
