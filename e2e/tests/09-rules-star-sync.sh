#!/usr/bin/env bash
# E2E Test: rules.star sync
#
# Tests rules.star sync as a project changes (10-go-coverage covers the
# first sync of a fresh project):
# 1. Add new import to Go file
# 2. Run tk rules check, verify staleness detected
# 3. Run tk rules sync, verify deps updated
# 4. Verify build still works
# 5. Remove the import: sync drops the dep and keeps a preserved section
#
# The template leaves rules sync before builds off, so only tk rules
# touches rules.star.
#
# Issues: turnkey-rlv3, https://github.com/firefly-engineering/turnkey/issues/225
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: rules.star sync"

# Step 1: Create test project from template
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "rules-sync")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Copy greenfield-go fixture
step "Adding Go source code"
copy_fixture "greenfield-go"

# Step 4: Stage files for flake
step "Staging files for Nix flake"
stage_for_flake

# Step 5: Generate initial deps
step "Generating deps"
run_in_devshell_script << 'INIT'
  echo "Generating initial go-deps.toml..."
  tk sync go
INIT

# Step 6: Commit initial state
step "Committing initial state"
stage_for_flake
commit_changes "Initial setup"

# Step 7: Verify initial build
step "Verifying initial build"
run_in_devshell_script << 'BUILD1'
  echo "Building with initial rules.star..."
  tk build //:hello
BUILD1

# Step 8: Add new import to main.go
step "Adding new import to source file"
cat > main.go << 'EOF'
package main

import (
	"fmt"

	"github.com/google/uuid"
	"github.com/fatih/color"
)

func main() {
	id := uuid.New()
	color.Green("Hello, World! ID: %s", id.String())
	fmt.Println()
}
EOF

# Add fatih/color to go.mod, and go-deps.toml with it: the commit's
# pre-commit hook rejects a go-deps.toml older than go.mod
run_in_devshell_script << 'GOGET'
  go get github.com/fatih/color@v1.16.0
  tk sync go
GOGET
assert_file_contains "go-deps.toml" "github.com/fatih/color" || exit 1

stage_for_flake
commit_changes "Add fatih/color import"

# Step 9: Run tk rules check - should detect staleness
step "Checking rules.star staleness"
check_result=0
run_in_devshell_script << 'CHECK' || check_result=$?
  echo "Running tk rules check..."
  tk rules check --force
CHECK

if [ $check_result -eq 0 ]; then
  echo "ERROR: tk rules check should have detected staleness (exit 1)"
  exit 1
fi
echo "Staleness correctly detected (exit code: $check_result)"

# Step 10: Run tk rules sync
step "Syncing rules.star"
run_in_devshell_script << 'SYNC'
  echo "Running tk rules sync..."
  tk rules sync --force --verbose
SYNC

# Step 11: Verify the new dependency was added to rules.star
step "Verifying rules.star updated"
assert_file_contains "rules.star" "fatih/color" || exit 1
echo "New dependency found in rules.star"

# Step 12: Commit and build
step "Committing updated rules.star and building"
stage_for_flake
commit_changes "Update rules.star with new dependency"

run_in_devshell_script << 'BUILD2'
  echo "Building with updated rules.star..."
  tk build //:hello

  echo ""
  echo "Running binary..."
  tk run //:hello
BUILD2

# Step 13: Test preservation markers
step "Testing preservation markers"
# Add a preserved section holding a dep no import needs (go-isatty, which
# go get brought in with fatih/color): sync keeps the deps in one
cat > rules.star << 'EOF'
# Auto-managed by turnkey. Hash: test123
# Manual sections marked with turnkey:preserve-start/end are not modified.

go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/fatih/color:color",
        "godeps//vendor/github.com/google/uuid:uuid",
        # turnkey:auto-end
        # turnkey:preserve-start
        "godeps//vendor/github.com/mattn/go-isatty:go-isatty",
        # turnkey:preserve-end
    ],
    visibility = ["PUBLIC"],
)
EOF

stage_for_flake
commit_changes "Add preservation markers"

# Remove the fatih/color import: sync must drop its dep, and only that
cat > main.go << 'EOF'
package main

import (
	"fmt"

	"github.com/google/uuid"
)

func main() {
	id := uuid.New()
	fmt.Println("Hello, World! ID:", id.String())
}
EOF

stage_for_flake
commit_changes "Remove fatih/color import"

# Run sync
run_in_devshell_script << 'SYNC2'
  echo "Running sync after import removal..."
  tk rules sync --force --verbose
SYNC2

# Verify the dep went and the preserved section stayed, with its dep
step "Verifying preserved section"
assert_file_not_contains "rules.star" "fatih/color" || exit 1
assert_file_contains "rules.star" "turnkey:preserve-start" || exit 1
assert_file_contains "rules.star" "mattn/go-isatty" || exit 1
echo "Preservation markers respected"

# Step 14: Verify final build
step "Verifying final build"
stage_for_flake
commit_changes "Sync after import removal"

run_in_devshell_script << 'BUILD3'
  echo "Final build verification..."
  tk build //:hello
  tk run //:hello
BUILD3

section "PASS: rules.star sync"
