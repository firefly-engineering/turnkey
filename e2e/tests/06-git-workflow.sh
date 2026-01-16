#!/usr/bin/env bash
# E2E Test: Git workflow with dependency updates
#
# Tests the PR workflow with dependency changes:
# 1. Start with working project on main branch
# 2. Create feature branch, add new dependency
# 3. Verify deps files updated, commit changes
# 4. Switch back to main - verify old deps restored
# 5. Switch to feature branch - verify new deps active
# 6. Merge to main, verify deps correctly merged
#
# Validates: git branch switching, no stale state
#
# Issue: turnkey-m5t
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 8 devshell calls (~112s overhead)
# Optimized: 6 devshell calls (~84s overhead)
# Note: Deps generation and build must be in separate devshells because
#       the godeps cell is evaluated at devshell entry time.
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Git workflow with dependency updates"

# Step 1: Create test project
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "git-workflow")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Create initial Go project with one dependency (uuid)
step "Creating initial Go project"
mkdir -p cmd/hello

cat > cmd/hello/main.go << 'EOF'
package main

import (
	"fmt"
	"github.com/google/uuid"
)

func main() {
	id := uuid.New()
	fmt.Printf("Hello from git-workflow test! UUID: %s\n", id.String())
}
EOF

cat > go.mod << 'EOF'
module example.com/git-workflow

go 1.21

require github.com/google/uuid v1.6.0
EOF

cat > go.sum << 'EOF'
github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=
github.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=
EOF

cat > BUCK << 'EOF'
go_binary(
    name = "hello",
    srcs = ["cmd/hello/main.go"],
    deps = ["godeps//vendor/github.com/google/uuid:uuid"],
)
EOF

# Step 4: Stage and generate deps for main branch
step "Staging files for Nix flake"
stage_for_flake

# Step 5: Phase 1 - Generate deps (must be committed before build)
step "Generating go-deps.toml for main branch"
run_in_devshell_script << 'PHASE1'
  echo "Generating go-deps.toml for main branch..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
PHASE1
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1

# Step 6: Commit main branch state (deps must be committed before Buck2 can see godeps cell)
step "Committing main branch state"
stage_for_flake
commit_changes "Initial project with uuid dependency"

# Step 7: Phase 2 - Build and run (single devshell, after deps committed)
step "Verifying main branch build (batched)"
output=$(run_in_devshell_script_capture << 'PHASE2'
  echo "Building main branch..."
  buck2 build //:hello

  echo ""
  echo "Running main branch binary..."
  buck2 run //:hello
PHASE2
)
echo "$output" | tail -5
assert_output_contains "echo '$output'" "UUID:" || exit 1

# Save main branch dep count for comparison
main_deps_count=$(grep -c '^\[deps\.' go-deps.toml || echo 0)
step "Main branch has ${main_deps_count} dependencies"

section "Feature Branch: Adding New Dependency"

# Step 8: Create feature branch
step "Creating feature branch"
git checkout -b feature/add-color

# Step 9: Update code to use new dependency (pkg/errors - pure Go, no assembly)
step "Updating code with new dependency"
cat > cmd/hello/main.go << 'EOF'
package main

import (
	"fmt"
	"github.com/google/uuid"
	"github.com/pkg/errors"
)

func main() {
	id := uuid.New()
	fmt.Printf("Hello from git-workflow test! UUID: %s\n", id.String())
	// Use pkg/errors to demonstrate new dep
	err := errors.New("example error for testing")
	fmt.Printf("Error with stack: %+v\n", err)
}
EOF

# Update go.mod with new dependency
cat > go.mod << 'EOF'
module example.com/git-workflow

go 1.21

require (
	github.com/google/uuid v1.6.0
	github.com/pkg/errors v0.9.1
)
EOF

cat > go.sum << 'EOF'
github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=
github.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=
github.com/pkg/errors v0.9.1 h1:FEBLx1zS214owpjy7qsBeixbURkuhQAwrK5UwLGTwt4=
github.com/pkg/errors v0.9.1/go.mod h1:bwawxfHBFNV+L2hUp1rHADufV3IMtnDRdf1r5NINEl0=
EOF

# Update BUCK to use new dependency
cat > BUCK << 'EOF'
go_binary(
    name = "hello",
    srcs = ["cmd/hello/main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
        "godeps//vendor/github.com/pkg/errors:errors",
    ],
)
EOF

# Step 10: Regenerate deps for feature branch
step "Regenerating deps for feature branch"
stage_for_flake
run_in_devshell_script << 'PHASE3'
  echo "Regenerating go-deps.toml for feature branch..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
PHASE3

# Verify new dependency is in deps file
assert_file_contains "go-deps.toml" "github.com/pkg/errors" || exit 1

# Stage updated deps so next devshell sees them
stage_for_flake

# Step 11: Build feature branch (separate devshell so it sees new deps)
step "Building feature branch"
run_in_devshell_script << 'PHASE4'
  echo "Building feature branch..."
  buck2 build //:hello
PHASE4

# Count deps on feature branch
feature_deps_count=$(grep -c '^\[deps\.' go-deps.toml || echo 0)
step "Feature branch has ${feature_deps_count} dependencies"

# Verify more deps than main
if [[ "$feature_deps_count" -le "$main_deps_count" ]]; then
  echo "ERROR: Feature branch should have more dependencies than main" >&2
  exit 1
fi

# Step 12: Commit feature branch changes
step "Committing feature branch changes"
stage_for_flake
commit_changes "Add fatih/color dependency"

section "Branch Switching: Verify State Isolation"

# Step 13: Switch back to main
step "Switching back to main branch"
git checkout main

# Verify main branch has original deps count
main_current_count=$(grep -c '^\[deps\.' go-deps.toml || echo 0)
step "Main branch now has ${main_current_count} dependencies"

if [[ "$main_current_count" -ne "$main_deps_count" ]]; then
  echo "ERROR: Main branch deps count changed unexpectedly" >&2
  echo "  Expected: ${main_deps_count}, Got: ${main_current_count}" >&2
  exit 1
fi

# Step 14: Phase 3 - Verify main branch still builds (single devshell)
step "Verifying main branch still builds"
run_in_devshell_script << 'PHASE3'
  echo "Building main branch..."
  buck2 build //:hello
PHASE3

# Step 15: Switch to feature branch
step "Switching to feature branch"
git checkout feature/add-color

# Verify feature branch has new deps
feature_current_count=$(grep -c '^\[deps\.' go-deps.toml || echo 0)
step "Feature branch now has ${feature_current_count} dependencies"

if [[ "$feature_current_count" -ne "$feature_deps_count" ]]; then
  echo "ERROR: Feature branch deps count changed unexpectedly" >&2
  echo "  Expected: ${feature_deps_count}, Got: ${feature_current_count}" >&2
  exit 1
fi

section "Merge: Verify Correct Dependency Integration"

# Step 16: Merge feature to main
step "Merging feature branch to main"
git checkout main
git merge feature/add-color --no-edit

# Verify merged main has feature branch deps
merged_deps_count=$(grep -c '^\[deps\.' go-deps.toml || echo 0)
step "Merged main has ${merged_deps_count} dependencies"

if [[ "$merged_deps_count" -ne "$feature_deps_count" ]]; then
  echo "ERROR: Merged main should have feature branch deps" >&2
  echo "  Expected: ${feature_deps_count}, Got: ${merged_deps_count}" >&2
  exit 1
fi

# Step 17: Phase 4 - Verify merged main builds (single devshell)
step "Verifying merged main builds"
run_in_devshell_script << 'PHASE4'
  echo "Building merged main..."
  buck2 build //:hello
PHASE4

# Verify the errors dependency is present
assert_file_contains "go-deps.toml" "github.com/pkg/errors" || exit 1

section "PASS: Git workflow with dependency updates"
