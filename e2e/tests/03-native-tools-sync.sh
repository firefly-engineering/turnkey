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

# Step 5: Generate initial deps files
step "Generating initial deps files"
run_in_devshell "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1

# Step 6: Commit to make deps available
step "Committing initial state"
stage_for_flake
commit_changes "Initial setup with deps"

# Step 7: Verify initial build works
step "Verifying initial build"
run_in_devshell "tk build //:hello-go"

# Step 8: Record initial go-deps.toml content
step "Recording initial deps state"
initial_deps_hash=$(run_in_devshell_capture "sha256sum go-deps.toml | cut -d' ' -f1")
echo "Initial go-deps.toml hash: $initial_deps_hash"

# Step 9: Add a new Go dependency using 'go get'
# The wrapped 'go' command should auto-sync go-deps.toml
step "Adding new Go dependency with 'go get'"

# First, update go.mod to use a module path that allows adding deps
run_in_devshell "go mod edit -module example.com/native-sync-test"

# Add a new dependency - tw should detect the change and sync
run_in_devshell "go get github.com/fatih/color@v1.16.0"

# Step 10: Verify go-deps.toml was auto-updated
step "Verifying go-deps.toml was auto-updated"
new_deps_hash=$(run_in_devshell_capture "sha256sum go-deps.toml | cut -d' ' -f1")
echo "New go-deps.toml hash: $new_deps_hash"

if [[ "$initial_deps_hash" == "$new_deps_hash" ]]; then
  echo "ERROR: go-deps.toml was not updated after 'go get'" >&2
  echo "This suggests the tw wrapper did not trigger sync" >&2
  exit 1
fi
echo "go-deps.toml was updated (hashes differ)"

# Step 11: Verify the new dependency is in go-deps.toml
step "Verifying new dependency in go-deps.toml"
assert_file_contains "go-deps.toml" "github.com/fatih/color" || exit 1

# Step 12: Stage updated files
step "Staging updated deps"
stage_for_flake
commit_changes "Add fatih/color dependency"

# Step 13: Verify build still works after adding dep
step "Verifying build works with new dependency"
run_in_devshell "tk build //:hello-go"

# Step 14: Update BUCK to use the new dep and create source that uses it
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

# Update BUCK to include the new dependency
cat > BUCK << 'EOF'
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

# Step 15: Build and run with the new dependency
step "Building with new dependency usage"
run_in_devshell "tk build //:hello-go"

step "Running binary with new dependency"
output=$(run_in_devshell_capture "tk run //:hello-go")
assert_output_contains "echo '$output'" "Hello from turnkey" || exit 1

section "PASS: Language-native tools stay in sync"
