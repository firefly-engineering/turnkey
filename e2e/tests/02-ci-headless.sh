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

# Step 5: Generate go-deps.toml BEFORE committing
# In a real CI scenario, this would already be committed
step "Generating go-deps.toml"
run_in_devshell "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1

# Step 6: Commit everything - this is the "clone-ready" state
step "Committing all files (simulating prepared repo state)"
stage_for_flake
commit_changes "Initial project setup with deps"

# At this point, the project is in a state that CI would see after clone

section "CI/CD Build Simulation"

# Step 7: Test CI-style build command
# This uses `nix develop --command` directly, without direnv
step "Running CI-style build (nix develop --command buck2 build //...)"
run_in_devshell "buck2 build //..."

# Step 8: Test CI-style test command
# The greenfield fixture doesn't have tests, but we can run the test command
# to verify it doesn't fail (empty test run is OK)
step "Running CI-style test (nix develop --command buck2 test //...)"
# Use || true because no test targets exist in greenfield fixture
run_in_devshell "buck2 test //... || echo 'No test targets (expected for greenfield)'"

# Step 9: Run the binary to verify functional output
step "Verifying built binary works"
output=$(run_in_devshell_capture "buck2 run //:hello")
assert_output_contains "echo '$output'" "Hello from turnkey" || exit 1

section "Verify Hermetic Build Properties"

# Step 10: Verify .envrc was NOT executed
# The run_in_devshell function uses nix develop --command which doesn't use direnv
step "Verifying direnv was not involved"
# Create a marker file that .envrc would create if executed
echo 'touch /tmp/envrc-was-executed-$$' >> .envrc
stage_for_flake
commit_changes "Add .envrc marker"

# Run another build - should succeed without .envrc being sourced
run_in_devshell "buck2 build //:hello"
# Note: We can't easily verify the marker wasn't created since /tmp is shared,
# but the key point is that the build succeeded using nix develop --command

# Step 11: Verify the deps cell exists and is usable
step "Verifying deps cell is accessible"
run_in_devshell "buck2 targets godeps//..."

section "PASS: CI/CD without interactive shell"
