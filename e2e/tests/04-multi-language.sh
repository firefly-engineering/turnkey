#!/usr/bin/env bash
# E2E Test: Multi-language monorepo (Go + Rust + Python)
#
# Tests that Go, Rust, and Python can coexist in the same project:
# 1. Initialize from turnkey template
# 2. Enable Go, Rust, and Python support
# 3. Add multi-language fixture code
# 4. Generate deps files for all languages
# 5. Build Go, Rust, and Python targets
# 6. Run tests
#
# Issue: turnkey-tps
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Multi-language monorepo (Go + Rust + Python)"

# Step 1: Create test project
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "multi-lang")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Enable multi-language support in flake.nix
step "Enabling Go + Rust + Python support in flake.nix"
# Get the turnkey path that init_from_template set
turnkey_path=$(grep 'turnkey.url' flake.nix | sed 's/.*"\(.*\)".*/\1/')
# Write a multi-language flake.nix
cat > flake.nix << EOF
{
  description = "Multi-language Buck2 project (Go + Rust + Python)";

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

# Step 5: Add rust and python toolchains to toolchain.toml
step "Adding rust and python to toolchain.toml"
cat >> toolchain.toml << 'EOF'
rust = {}
rustdeps-gen = {}
python = {}
uv = {}
pydeps-gen = {}
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

# Step 7: Verify devshell has required tools
step "Verifying devshell tools (Go + Rust + Python)"
assert_command_in_devshell "buck2" || exit 1
assert_command_in_devshell "go" || exit 1
assert_command_in_devshell "godeps-gen" || exit 1
assert_command_in_devshell "rustdeps-gen" || exit 1
assert_command_in_devshell "cargo" || exit 1
assert_command_in_devshell "python" || exit 1
assert_command_in_devshell "uv" || exit 1
assert_command_in_devshell "pydeps-gen" || exit 1

# Step 8: Generate go-deps.toml
step "Generating go-deps.toml"
run_in_devshell "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1

# Step 9: Generate rust-deps.toml
step "Generating rust-deps.toml"
run_in_devshell "rustdeps-gen --cargo-lock rust_lib/Cargo.lock -o rust-deps.toml"
assert_file_exists "rust-deps.toml" || exit 1
assert_file_contains "rust-deps.toml" "serde" || exit 1

# Step 9b: Generate python-deps.toml
step "Generating python-deps.toml"
run_in_devshell "pydeps-gen --lock python_app/pylock.toml -o python-deps.toml"
assert_file_exists "python-deps.toml" || exit 1
assert_file_contains "python-deps.toml" "six" || exit 1

# Step 10: Commit deps files
step "Committing deps files"
stage_for_flake
commit_changes "Add go-deps.toml, rust-deps.toml, and python-deps.toml"

# Step 11: Build Go binary
step "Building Go binary"
run_in_devshell "buck2 build //:hello-go"

# Step 12: Build Rust library
step "Building Rust library"
run_in_devshell "buck2 build //rust_lib:greeting"

# Step 13: Run Rust tests
step "Running Rust tests"
run_in_devshell "buck2 test //rust_lib:greeting-test"

# Step 14: Run Go binary
step "Running Go binary"
output=$(run_in_devshell_capture "buck2 run //:hello-go")
assert_output_contains "echo '$output'" "Go: Hello" || exit 1

# Step 15: Build Python binary
step "Building Python binary"
run_in_devshell "buck2 build //python_app:hello-python"

# Step 16: Run Python binary
step "Running Python binary"
output=$(run_in_devshell_capture "buck2 run //python_app:hello-python")
assert_output_contains "echo '$output'" "Python: Hello" || exit 1

section "PASS: Multi-language monorepo (Go + Rust + Python)"
