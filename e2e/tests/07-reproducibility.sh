#!/usr/bin/env bash
# E2E Test: Build reproducibility
#
# Tests that builds are reproducible (same inputs → same outputs):
# 1. Set up multi-language project (Go + Rust + Python)
# 2. Build targets, capture output hashes
# 3. Clean build artifacts
# 4. Rebuild same targets
# 5. Compare hashes - must be identical
#
# Issue: turnkey-tdb3
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Build reproducibility"

# Step 1: Create test project
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "reproducibility")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Enable multi-language support
step "Enabling Go + Rust + Python support in flake.nix"
turnkey_path=$(grep 'turnkey.url' flake.nix | sed 's/.*"\(.*\)".*/\1/')
cat > flake.nix << EOF
{
  description = "Reproducibility test project (Go + Rust + Python)";

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

      perSystem = { pkgs, config, ... }: {
        turnkey.toolchains = {
          enable = true;
          declarationFiles.default = ./toolchain.toml;

          registry = {
            buck2 = pkgs.buck2;
            nix = pkgs.nix;
            go = pkgs.go;
            rust = pkgs.rustc;
            python = pkgs.python3;
            uv = pkgs.uv;
            clang = pkgs.llvmPackages.clang;
            lld = pkgs.llvmPackages.lld;
            godeps-gen = inputs.turnkey.packages.\${pkgs.system}.godeps-gen;
            rustdeps-gen = inputs.turnkey.packages.\${pkgs.system}.rustdeps-gen;
            pydeps-gen = inputs.turnkey.packages.\${pkgs.system}.pydeps-gen;
            tk = inputs.turnkey.packages.\${pkgs.system}.tk;
          };

          buck2 = {
            enable = true;
            prelude.strategy = "bundled";

            go = {
              enable = true;
              depsFile = ./go-deps.toml;
            };

            rust = {
              enable = true;
              depsFile = ./rust-deps.toml;
              featuresFile = ./rust-features.toml;
              cargoTomlFile = "rust_lib/Cargo.toml";
              cargoLockFile = "rust_lib/Cargo.lock";
            };

            python = {
              enable = true;
              depsFile = ./python-deps.toml;
              pyprojectFile = "python_app/pyproject.toml";
            };
          };
        };
      };
    };
}
EOF

# Step 4: Copy multi-language fixture
step "Adding multi-language source code"
copy_fixture "multi-language"

# Step 5: Add rust and python toolchains
step "Adding rust and python to toolchain.toml"
cat >> toolchain.toml << 'EOF'
rust = {}
rustdeps-gen = {}
python = {}
uv = {}
pydeps-gen = {}
EOF

# Step 6: Create rust-features.toml
step "Creating rust-features.toml"
cat > rust-features.toml << 'EOF'
[overrides]
serde = { add = ["derive", "serde_derive"] }
EOF

# Step 7: Stage files for flake
step "Staging files for Nix flake"
stage_for_flake

# Step 8: Generate deps files
step "Generating go-deps.toml"
run_in_devshell "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1

step "Generating rust-deps.toml"
run_in_devshell "rustdeps-gen --cargo-lock rust_lib/Cargo.lock -o rust-deps.toml"
assert_file_exists "rust-deps.toml" || exit 1

step "Generating python-deps.toml"
run_in_devshell "pydeps-gen --lock python_app/pylock.toml -o python-deps.toml"
assert_file_exists "python-deps.toml" || exit 1

# Step 9: Commit deps files
step "Committing deps files"
stage_for_flake
commit_changes "Add deps files"

# Step 10: First build - capture hashes
step "Building targets (first build)"
run_in_devshell "tk build //:hello-go //rust_lib:greeting //python_app:hello-python"

step "Capturing output hashes (first build)"
# Find outputs using find (most reliable)
# Go binary is named 'hello-go'
# Rust library produces .rmeta (metadata) file, not .rlib for rust_library
# Python binary produces a .pex file
go_output=$(find buck-out -name "hello-go" -type f ! -name "*.d" ! -name "*.dwp" 2>/dev/null | head -1)
rust_output=$(find buck-out/v2/gen/root -name "libgreeting*.rmeta" -type f 2>/dev/null | head -1)
python_output=$(find buck-out/v2/gen/root -name "hello-python.pex" -type f 2>/dev/null | head -1)

echo "Go output: $go_output"
echo "Rust output: $rust_output"
echo "Python output: $python_output"

assert_not_empty "$go_output" "Go binary path should not be empty" || exit 1
assert_not_empty "$rust_output" "Rust library path should not be empty" || exit 1
assert_not_empty "$python_output" "Python binary path should not be empty" || exit 1

# Capture hashes
go_hash1=$(sha256sum "$go_output" | cut -d' ' -f1)
rust_hash1=$(sha256sum "$rust_output" | cut -d' ' -f1)
python_hash1=$(sha256sum "$python_output" | cut -d' ' -f1)

echo "Go hash (build 1): $go_hash1"
echo "Rust hash (build 1): $rust_hash1"
echo "Python hash (build 1): $python_hash1"

# Step 11: Clean build artifacts
step "Cleaning build artifacts"
run_in_devshell "tk clean"

# Verify clean worked
assert_file_not_exists "$go_output" "Go output should be cleaned" || exit 1

# Step 12: Second build
step "Building targets (second build)"
run_in_devshell "tk build //:hello-go //rust_lib:greeting //python_app:hello-python"

# Step 13: Capture hashes again
step "Capturing output hashes (second build)"
# Find outputs again
go_output2=$(find buck-out -name "hello-go" -type f ! -name "*.d" ! -name "*.dwp" 2>/dev/null | head -1)
rust_output2=$(find buck-out/v2/gen/root -name "libgreeting*.rmeta" -type f 2>/dev/null | head -1)
python_output2=$(find buck-out/v2/gen/root -name "hello-python.pex" -type f 2>/dev/null | head -1)

assert_not_empty "$go_output2" "Go binary path should not be empty (build 2)" || exit 1
assert_not_empty "$rust_output2" "Rust library path should not be empty (build 2)" || exit 1
assert_not_empty "$python_output2" "Python binary path should not be empty (build 2)" || exit 1

go_hash2=$(sha256sum "$go_output2" | cut -d' ' -f1)
rust_hash2=$(sha256sum "$rust_output2" | cut -d' ' -f1)
python_hash2=$(sha256sum "$python_output2" | cut -d' ' -f1)

echo "Go hash (build 2): $go_hash2"
echo "Rust hash (build 2): $rust_hash2"
echo "Python hash (build 2): $python_hash2"

# Step 14: Compare hashes
step "Verifying build reproducibility"

if [[ "$go_hash1" != "$go_hash2" ]]; then
  echo "ERROR: Go binary is NOT reproducible!" >&2
  echo "  Build 1: $go_hash1" >&2
  echo "  Build 2: $go_hash2" >&2
  exit 1
fi
echo "Go binary: REPRODUCIBLE"

if [[ "$rust_hash1" != "$rust_hash2" ]]; then
  echo "ERROR: Rust library is NOT reproducible!" >&2
  echo "  Build 1: $rust_hash1" >&2
  echo "  Build 2: $rust_hash2" >&2
  exit 1
fi
echo "Rust library: REPRODUCIBLE"

if [[ "$python_hash1" != "$python_hash2" ]]; then
  echo "ERROR: Python binary is NOT reproducible!" >&2
  echo "  Build 1: $python_hash1" >&2
  echo "  Build 2: $python_hash2" >&2
  exit 1
fi
echo "Python binary: REPRODUCIBLE"

section "PASS: Build reproducibility (Go + Rust + Python)"
