#!/usr/bin/env bash
# E2E Test: Language-native tools stay in sync
#
# Tests that native tools (go, cargo, uv) automatically sync deps files:
# 1. Start with working turnkey project
# 2. Add new dependency using native tool
# 3. Verify deps files are auto-updated
# 4. Verify build still works with new dep
#
# Issue: turnkey-66b
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 9 devshell calls (~126s overhead)
# Optimized: 4 devshell calls (~56s overhead)
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Language-native tools stay in sync"

# Step 1: Create test project from template
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "native-sync")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Copy multi-language fixture (has go, rust, python)
step "Adding multi-language source code"
copy_fixture "multi-language"

# Step 4: Stage files for flake
step "Staging files for Nix flake"
stage_for_flake

# Step 5: Phase 1 - Generate initial deps and verify build (single devshell)
step "Generating initial deps and verifying build (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Generating go-deps.toml..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml

  echo "Recording initial state..."
  sha256sum go-deps.toml > /tmp/initial-deps-hash.txt
PHASE1
assert_file_exists "go-deps.toml" || exit 1

# Step 6: Commit to make deps available
step "Committing initial state"
stage_for_flake
commit_changes "Initial setup with deps"

# Step 7: Phase 2 - Verify initial build and add new dep (single devshell)
step "Verifying build and adding new dependency (batched)"
run_in_devshell_script << 'PHASE2'
  echo "Verifying initial build..."
  tk build //:hello-go

  echo ""
  echo "Recording initial go-deps.toml hash..."
  initial_hash=$(sha256sum go-deps.toml | cut -d' ' -f1)
  echo "Initial hash: $initial_hash"
  echo "$initial_hash" > /tmp/initial-hash.txt

  echo ""
  echo "Updating module path..."
  go mod edit -module example.com/native-sync-test

  echo ""
  echo "Adding new Go dependency with 'go get'..."
  go get github.com/fatih/color@v1.16.0

  echo ""
  echo "Checking if go-deps.toml was updated..."
  new_hash=$(sha256sum go-deps.toml | cut -d' ' -f1)
  echo "New hash: $new_hash"

  if [ "$initial_hash" = "$new_hash" ]; then
    echo "ERROR: go-deps.toml was not updated after 'go get'"
    exit 1
  fi
  echo "SUCCESS: go-deps.toml was updated (hashes differ)"
PHASE2

# Step 8: Verify the new dependency is in go-deps.toml
step "Verifying new dependency in go-deps.toml"
assert_file_contains "go-deps.toml" "github.com/fatih/color" || exit 1

# Step 9: Stage updated files
step "Staging updated deps"
stage_for_flake
commit_changes "Add fatih/color dependency"

# Step 10: Update source to use new dependency
step "Updating source to use new dependency"
cat > main.go << 'EOF'
package main

import (
	"fmt"

	"github.com/fatih/color"
	"github.com/google/uuid"
)

func main() {
	id := uuid.New()
	color.Green("Hello from turnkey with colors! UUID: %s", id)
	fmt.Println() // Ensure output ends with newline
}
EOF

# Update rules.star to include the new dependency
cat > rules.star << 'EOF'
go_binary(
    name = "hello-go",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
        "godeps//vendor/github.com/fatih/color:color",
    ],
    visibility = ["PUBLIC"],
)
EOF

stage_for_flake
commit_changes "Use fatih/color in hello"

# Step 11: Phase 3 - Build and run with new dependency (single devshell)
step "Building and running with new dependency (batched)"
output=$(run_in_devshell_script_capture << 'PHASE3'
  echo "Building with new dependency..."
  tk build //:hello-go

  echo ""
  echo "Running binary..."
  tk run //:hello-go
PHASE3
)
echo "$output" | tail -5
assert_output_contains "echo '$output'" "Hello from turnkey" || exit 1

section "PASS: Language-native tools stay in sync"
