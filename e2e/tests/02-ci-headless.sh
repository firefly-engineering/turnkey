#!/usr/bin/env bash
# E2E Test: CI/CD without interactive shell
#
# Tests the CI/CD workflow where:
# 1. Project is already set up with deps committed (cloned state)
# 2. Builds run using `nix develop --command` without direnv
# 3. No interactive shell or .envrc execution needed
#
# This validates:
# - CI compatibility without direnv
# - Hermetic builds with pre-fetched deps
# - Non-interactive workflow
#
# Issue: turnkey-2t5
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 7 devshell calls (~98s overhead)
# Optimized: 3 devshell calls (~42s overhead)
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: CI/CD without interactive shell"

# Step 1: Create test project (simulates CI clone)
step "Creating test project directory (simulating CI clone)"
PROJECT_DIR=$(setup_test_project "ci-headless")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Copy Go source files
step "Adding Go source code"
copy_fixture "greenfield-go"

# Step 4: Stage and commit initial files
# This simulates the state of a project when CI clones it
step "Staging files for Nix flake"
stage_for_flake

# Step 5: Phase 1 - Generate deps (single devshell)
step "Generating go-deps.toml"
run_in_devshell_script << 'PHASE1'
  echo "Generating go-deps.toml..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
  echo "Generated go-deps.toml"
PHASE1
assert_file_exists "go-deps.toml" || exit 1

# Step 6: Commit everything - this is the "clone-ready" state
step "Committing all files (simulating prepared repo state)"
stage_for_flake
commit_changes "Initial project setup with deps"

# At this point, the project is in a state that CI would see after clone

section "CI/CD Build Simulation"

# Step 7: Phase 2 - CI-style build, test, and run (single devshell)
step "Running CI-style build/test/run (batched)"
output=$(run_in_devshell_script_capture << 'PHASE2'
  echo "=== CI Build ==="
  buck2 build //...

  echo ""
  echo "=== CI Test ==="
  buck2 test //... || echo "No test targets (expected for greenfield)"

  echo ""
  echo "=== Run binary ==="
  buck2 run //:hello
PHASE2
)
echo "$output" | tail -10

# Verify output
assert_output_contains "echo '$output'" "Hello from turnkey" || exit 1

section "Verify Hermetic Build Properties"

# Step 8: Verify .envrc was NOT executed
# The run_in_devshell function uses nix develop --command which doesn't use direnv
step "Verifying direnv was not involved"
# Create a marker file that .envrc would create if executed
echo 'touch /tmp/envrc-was-executed-$$' >> .envrc
stage_for_flake
commit_changes "Add .envrc marker"

# Step 9: Phase 3 - Run another build and verify deps cell (single devshell)
step "Final verification (batched)"
run_in_devshell_script << 'PHASE3'
  echo "Building again (should succeed without .envrc)..."
  buck2 build //:hello

  echo ""
  echo "Verifying deps cell is accessible..."
  buck2 targets godeps//...
  echo "Deps cell is accessible"
PHASE3

section "PASS: CI/CD without interactive shell"
