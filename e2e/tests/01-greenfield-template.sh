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
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 6 devshell calls (~84s overhead)
# Optimized: 2 devshell calls (~28s overhead)
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

# Step 6: Phase 1 - Verify tools and generate deps (single devshell)
step "Verifying devshell tools and generating deps (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Checking required tools..."
  for cmd in buck2 go godeps-gen; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "ERROR: Command not found: $cmd" >&2
      exit 1
    fi
    echo "  Found: $cmd"
  done

  echo "Generating go-deps.toml..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
  echo "Generated go-deps.toml"
PHASE1

# Verify deps file was created
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1
assert_file_contains "go-deps.toml" "schema_version" || exit 1

# Step 7: Commit to make deps available
step "Committing deps file"
stage_for_flake
commit_changes "Add go-deps.toml"

# Step 8: Phase 2 - Build and run (single devshell)
step "Building and running with Buck2 (batched)"
output=$(run_in_devshell_script_capture << 'PHASE2'
  echo "Building //:hello..."
  buck2 build //:hello

  echo "Running //:hello..."
  buck2 run //:hello
PHASE2
)
echo "$output" | tail -5

# Step 9: Verify output
assert_output_contains "echo '$output'" "Hello from turnkey" || exit 1

section "PASS: Greenfield project from template"
