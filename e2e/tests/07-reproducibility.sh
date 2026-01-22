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
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 6 devshell calls (~84s overhead)
# Optimized: 3 devshell calls (~42s overhead)
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

# Step 8: Phase 1 - Generate all deps files (single devshell)
step "Generating all deps files (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Generating go-deps.toml..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml

  echo ""
  echo "Generating rust-deps.toml..."
  rustdeps-gen --cargo-lock rust_lib/Cargo.lock -o rust-deps.toml

  echo ""
  echo "Generating python-deps.toml..."
  pydeps-gen --lock python_app/pylock.toml -o python-deps.toml
PHASE1
assert_file_exists "go-deps.toml" || exit 1
assert_file_exists "rust-deps.toml" || exit 1
assert_file_exists "python-deps.toml" || exit 1

# Step 9: Commit deps files
step "Committing deps files"
stage_for_flake
commit_changes "Add deps files"

# Step 10: Phase 2 - First build (single devshell)
step "Building targets (first build)"
run_in_devshell_script << 'PHASE2'
  echo "Building all targets..."
  tk build //:hello-go //rust_lib:greeting //python_app:hello-python
PHASE2

step "Capturing output hashes (first build)"
# Find outputs using find with timeout to avoid hangs
# Go binary is named 'hello-go'
# Rust library produces .rmeta (metadata) file, not .rlib for rust_library
# Python binary produces a .pex or just an executable file
echo "Searching for build outputs..."
echo "PWD: $(pwd)"

# Use timeout and -maxdepth to avoid slow searches
# Search from buck-out root since structure may vary (.turnkey/gen or v2/gen)
go_output=$(timeout 30 find buck-out -maxdepth 15 -name "hello-go" -type f ! -name "*.d" ! -name "*.dwp" 2>/dev/null | head -1)
echo "Go output: ${go_output:-<not found>}"

rust_output=$(timeout 30 find buck-out -maxdepth 15 -name "libgreeting*.rmeta" -type f 2>/dev/null | head -1)
echo "Rust output: ${rust_output:-<not found>}"

# Python might produce .pex or a plain executable
python_output=$(timeout 30 find buck-out -maxdepth 15 \( -name "hello-python.pex" -o -name "hello-python" \) -type f 2>/dev/null | head -1)
echo "Python output: ${python_output:-<not found>}"

# Debug: show buck-out structure
echo "buck-out structure (first 30 lines):"
find buck-out -maxdepth 5 -type f 2>/dev/null | head -30 || echo "  <empty or error>"

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

# Step 11: Phase 3 - Clean and rebuild (single devshell)
step "Cleaning build artifacts and rebuilding (batched)"
run_in_devshell_script << 'PHASE3'
  echo "Cleaning build artifacts..."
  tk clean

  echo ""
  echo "Rebuilding all targets..."
  tk build //:hello-go //rust_lib:greeting //python_app:hello-python
PHASE3

# Step 12: Capture hashes again
step "Capturing output hashes (second build)"
# Find outputs again with timeout and maxdepth
# Search from buck-out root since structure may vary
go_output2=$(timeout 30 find buck-out -maxdepth 15 -name "hello-go" -type f ! -name "*.d" ! -name "*.dwp" 2>/dev/null | head -1)
rust_output2=$(timeout 30 find buck-out -maxdepth 15 -name "libgreeting*.rmeta" -type f 2>/dev/null | head -1)
python_output2=$(timeout 30 find buck-out -maxdepth 15 \( -name "hello-python.pex" -o -name "hello-python" \) -type f 2>/dev/null | head -1)

echo "Go output (build 2): ${go_output2:-<not found>}"
echo "Rust output (build 2): ${rust_output2:-<not found>}"
echo "Python output (build 2): ${python_output2:-<not found>}"

assert_not_empty "$go_output2" "Go binary path should not be empty (build 2)" || exit 1
assert_not_empty "$rust_output2" "Rust library path should not be empty (build 2)" || exit 1
assert_not_empty "$python_output2" "Python binary path should not be empty (build 2)" || exit 1

go_hash2=$(sha256sum "$go_output2" | cut -d' ' -f1)
rust_hash2=$(sha256sum "$rust_output2" | cut -d' ' -f1)
python_hash2=$(sha256sum "$python_output2" | cut -d' ' -f1)

echo "Go hash (build 2): $go_hash2"
echo "Rust hash (build 2): $rust_hash2"
echo "Python hash (build 2): $python_hash2"

# Step 13: Compare hashes
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
