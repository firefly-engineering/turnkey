#!/usr/bin/env bash
# E2E Test: Multi-language monorepo (Go + Rust + Python + TypeScript)
#
# Tests that Go, Rust, Python, and TypeScript can coexist in the same project:
# 1. Initialize from turnkey template
# 2. Enable Go, Rust, Python, and TypeScript support
# 3. Add multi-language fixture code
# 4. Generate deps files for all languages
# 5. Build Go, Rust, Python, and TypeScript targets
# 6. Run tests
#
# Issue: turnkey-tps
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 21 devshell calls (~294s overhead)
# Optimized: 3 devshell calls (~42s overhead)
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Multi-language monorepo (Go + Rust + Python + TypeScript)"

# Step 1: Create test project
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "multi-lang")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Enable multi-language support in flake.nix
step "Enabling Go + Rust + Python + TypeScript support in flake.nix"
# Get the turnkey path that init_from_template set
turnkey_path=$(grep 'turnkey.url' flake.nix | sed 's/.*"\(.*\)".*/\1/')
# Write a multi-language flake.nix
cat > flake.nix << EOF
{
  description = "Multi-language Buck2 project (Go + Rust + Python + TypeScript)";

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
            nodejs = pkgs.nodejs;
            typescript = pkgs.typescript;
            godeps-gen = inputs.turnkey.packages.\${pkgs.system}.godeps-gen;
            rustdeps-gen = inputs.turnkey.packages.\${pkgs.system}.rustdeps-gen;
            pydeps-gen = inputs.turnkey.packages.\${pkgs.system}.pydeps-gen;
            tk = inputs.turnkey.packages.\${pkgs.system}.tk;
          };

          buck2 = {
            enable = true;
            # Use nix prelude strategy for TypeScript support
            prelude.strategy = "nix";

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

# Step 5: Add rust, python, and typescript toolchains to toolchain.toml
step "Adding rust, python, and typescript to toolchain.toml"
cat >> toolchain.toml << 'EOF'
rust = {}
rustdeps-gen = {}
python = {}
uv = {}
pydeps-gen = {}
nodejs = {}
typescript = {}
EOF

# Step 5b: Create rust-features.toml to enable serde derive
step "Creating rust-features.toml"
cat > rust-features.toml << 'EOF'
# Feature overrides for Rust crates
[overrides]
serde = { add = ["derive", "serde_derive"] }
EOF

# Step 6: Stage files for flake
step "Staging files for Nix flake"
stage_for_flake

# Step 7: Phase 1 - Verify tools and generate all deps (single devshell)
step "Verifying devshell tools and generating deps (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Checking required tools..."
  for cmd in buck2 go godeps-gen rustdeps-gen cargo python uv pydeps-gen node tsc; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "ERROR: Command not found: $cmd" >&2
      exit 1
    fi
    echo "  Found: $cmd"
  done

  echo ""
  echo "Generating go-deps.toml..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
  echo "Generated go-deps.toml"

  echo ""
  echo "Generating rust-deps.toml..."
  rustdeps-gen --cargo-lock rust_lib/Cargo.lock -o rust-deps.toml
  echo "Generated rust-deps.toml"

  echo ""
  echo "Generating python-deps.toml..."
  pydeps-gen --lock python_app/pylock.toml -o python-deps.toml
  echo "Generated python-deps.toml"
PHASE1

# Verify deps files were created
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1
assert_file_exists "rust-deps.toml" || exit 1
assert_file_contains "rust-deps.toml" "serde" || exit 1
assert_file_exists "python-deps.toml" || exit 1
assert_file_contains "python-deps.toml" "six" || exit 1

# Step 8: Commit deps files
step "Committing deps files"
stage_for_flake
commit_changes "Add go-deps.toml, rust-deps.toml, and python-deps.toml"

# Step 9: Phase 2 - Build all targets (single devshell)
step "Building all targets (batched)"
run_in_devshell_script << 'PHASE2'
  echo "Building Go binary..."
  buck2 build //:hello-go

  echo ""
  echo "Building Rust library..."
  buck2 build //rust_lib:greeting

  echo ""
  echo "Running Rust tests..."
  buck2 test //rust_lib:greeting-test

  echo ""
  echo "Building Python binary..."
  buck2 build //python_app:hello-python

  echo ""
  echo "Building TypeScript binary..."
  buck2 build //typescript_app:hello-typescript
PHASE2

# Step 10: Phase 3 - Run binaries and verify output (single devshell)
step "Running binaries and verifying output (batched)"
output=$(run_in_devshell_script_capture << 'PHASE3'
  echo "=== Go output ==="
  buck2 run //:hello-go

  echo ""
  echo "=== Python output ==="
  buck2 run //python_app:hello-python

  echo ""
  echo "=== TypeScript output ==="
  buck2 run //typescript_app:hello-typescript
PHASE3
)
echo "$output" | tail -15

# Verify outputs
assert_output_contains "echo '$output'" "Go: Hello" || exit 1
assert_output_contains "echo '$output'" "Python: Hello" || exit 1
assert_output_contains "echo '$output'" "TypeScript: Hello" || exit 1

section "PASS: Multi-language monorepo (Go + Rust + Python + TypeScript)"
