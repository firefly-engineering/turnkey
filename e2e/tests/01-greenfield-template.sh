#!/usr/bin/env bash
# E2E Test: Greenfield project from template
#
# Tests the new user onboarding flow:
# 1. Initialize from turnkey template
# 2. Add Go source code
# 3. Generate deps file
# 4. Build with Buck2
#
# Issue: turnkey-1us
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Greenfield project from template"

# Step 1: Create test project
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "greenfield-go")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Verify template files created
step "Verifying template files"
assert_file_exists "flake.nix" || exit 1
assert_file_exists "toolchain.toml" || exit 1
assert_file_exists ".envrc" || exit 1

# Step 4: Copy Go source files
step "Adding Go source code"
copy_fixture "greenfield-go"

# Step 5: Stage files for flake
step "Staging files for Nix flake"
stage_for_flake

# Step 6: Verify devshell has required tools
step "Verifying devshell tools"
assert_command_in_devshell "buck2" || exit 1
assert_command_in_devshell "go" || exit 1
assert_command_in_devshell "godeps-gen" || exit 1

# Step 7: Generate go-deps.toml
step "Generating go-deps.toml"
run_in_devshell "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1
assert_file_contains "go-deps.toml" "schema_version" || exit 1

# Step 8: Commit to make deps available
step "Committing deps file"
stage_for_flake
commit_changes "Add go-deps.toml"

# Step 9: Build with Buck2
step "Building with Buck2"
# Note: We need to re-enter devshell after committing for deps cell to be available
run_in_devshell "buck2 build //:hello"

# Step 10: Run the binary
step "Running the binary"
output=$(run_in_devshell_capture "buck2 run //:hello")
assert_output_contains "echo '$output'" "Hello from turnkey" || exit 1

section "PASS: Greenfield project from template"
