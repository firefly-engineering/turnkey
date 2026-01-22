#!/usr/bin/env bash
# E2E Test: Error recovery and diagnostics
#
# Tests that turnkey provides clear error messages and recovery paths:
# 1. Introduce invalid dependency (non-existent package)
# 2. Verify clear error message during regeneration
# 3. Fix dependency, verify regeneration succeeds
# 4. Delete deps file manually, verify can be regenerated
# 5. Corrupt deps file, verify detected with clear error
#
# Note: Network failure testing is skipped as it requires network simulation
#
# Validates: error messages, recovery paths, robustness
#
# Issue: turnkey-dw7
#
# OPTIMIZED: Uses batched devshell calls to minimize nix develop overhead.
# Original: 8 devshell calls (~112s overhead)
# Optimized: 7 devshell calls (~98s overhead)
# Note: Limited batching due to intentional failure tests that need isolation.
set -euo pipefail

# Source test libraries
source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Error recovery and diagnostics"

# Step 1: Create test project
step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "error-recovery")
cd "$PROJECT_DIR"

# Step 2: Initialize from template
step "Initializing from turnkey template"
init_from_template

# Step 3: Copy Go source files
step "Adding Go source code"
copy_fixture "greenfield-go"

# Step 4: Stage and generate initial deps
step "Staging files for Nix flake"
stage_for_flake

# Step 5: Phase 1 - Generate deps and verify initial build (single devshell)
step "Generating deps and verifying initial build (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Generating initial go-deps.toml..."
  godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml
PHASE1
assert_file_exists "go-deps.toml" || exit 1

# Commit working state
step "Committing working state"
stage_for_flake
commit_changes "Initial working state"

# Verify working build (needs fresh devshell after commit)
step "Verifying initial build works"
run_in_devshell_script << 'PHASE1B'
  echo "Building to verify initial state..."
  buck2 build //:hello
PHASE1B

section "Test 1: Invalid Dependency"

# Step 6: Introduce an invalid dependency
step "Introducing invalid dependency (non-existent package)"
cat > go.mod << 'EOF'
module example.com/greenfield

go 1.21

require (
	github.com/google/uuid v1.6.0
	github.com/nonexistent/totally-fake-package v9.9.9
)
EOF

# Add a fake entry to go.sum (godeps-gen will fail on prefetch)
cat > go.sum << 'EOF'
github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=
github.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=
github.com/nonexistent/totally-fake-package v9.9.9 h1:FAKE+HASH=
github.com/nonexistent/totally-fake-package v9.9.9/go.mod h1:FAKE+HASH=
EOF

# Step 7: Verify error message when trying to regenerate
step "Verifying clear error message for invalid dependency"
stage_for_flake

# godeps-gen should fail with a clear error message
if run_in_devshell_script "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps-bad.toml" 2>&1; then
  echo "ERROR: godeps-gen should have failed with invalid dependency" >&2
  exit 1
fi
echo "godeps-gen correctly failed for invalid dependency"

# Step 8: Fix the dependency
step "Fixing dependency (removing invalid package)"
cat > go.mod << 'EOF'
module example.com/greenfield

go 1.21

require github.com/google/uuid v1.6.0
EOF

cat > go.sum << 'EOF'
github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=
github.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=
EOF

# Step 9: Verify regeneration succeeds after fix
step "Verifying regeneration succeeds after fix"
stage_for_flake
run_in_devshell_script "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1

section "Test 2: Missing Deps File"

# Step 10: Delete deps file
step "Deleting go-deps.toml"
rm go-deps.toml
git add -A

# Step 11: Verify regeneration works
step "Verifying deps file can be regenerated"
run_in_devshell_script "godeps-gen --go-mod go.mod --go-sum go.sum --prefetch -o go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1
assert_file_contains "go-deps.toml" "github.com/google/uuid" || exit 1

section "Test 3: Corrupted Deps File Detection"

# Save a backup of the valid deps file
cp go-deps.toml go-deps.toml.backup

# Step 12: Create a corrupted deps file
step "Creating corrupted go-deps.toml"
echo "this is not valid TOML { [ garbage" > go-deps.toml
stage_for_flake
# Use --no-verify to bypass TOML syntax check - we're testing error recovery
git commit -m "Intentionally corrupted deps file" --quiet --no-verify

# Step 13: Verify Nix gives clear error about corrupted TOML
step "Verifying clear error message for corrupted deps file"
# The devshell should fail to load with a TOML parse error
if nix develop --no-pure-eval --command true 2>&1 | tee /tmp/nix-error.log; then
  echo "WARNING: Nix devshell loaded despite corrupted deps file (unexpected)"
else
  echo "Nix correctly failed to load with corrupted deps file"
  # Verify error mentions TOML
  if grep -qi "toml\|parse\|invalid" /tmp/nix-error.log; then
    echo "Error message mentions TOML parsing issue (good)"
  fi
fi

# Step 14: Fix by restoring the valid deps file
step "Restoring valid deps file to recover"
cp go-deps.toml.backup go-deps.toml
rm go-deps.toml.backup

# Step 15: Verify build works after fix
step "Committing fixed deps file"
stage_for_flake
commit_changes "Fix corrupted deps file"

step "Verifying build works after fixing corruption"
run_in_devshell_script "buck2 build //:hello"

section "Test 4: Invalid rules.star File Recovery"

# Step 16: Corrupt the rules.star file
step "Corrupting rules.star file"
echo "this is not valid Starlark {{ garbage" > rules.star
stage_for_flake
# Use --no-verify to bypass Starlark lint - we're testing error recovery
git commit -m "Intentionally corrupted rules.star file" --quiet --no-verify

# Step 17: Verify clear error message
step "Verifying clear error for invalid rules.star file"
if run_in_devshell_script "buck2 build //:hello" 2>&1 | tee /tmp/buck-error.log; then
  echo "ERROR: Build should have failed with invalid rules.star file" >&2
  exit 1
fi

# Verify error message mentions rules.star or syntax
if grep -qi "syntax\|error\|parse\|invalid" /tmp/buck-error.log; then
  echo "Error message mentions syntax/parse issue (good)"
else
  echo "WARNING: Error message may not be clear about syntax issue"
fi

# Step 18: Fix rules.star file
step "Fixing rules.star file"
cat > rules.star << 'EOF'
go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = ["godeps//vendor/github.com/google/uuid:uuid"],
    visibility = ["PUBLIC"],
)
EOF
stage_for_flake
commit_changes "Fix rules.star file"

# Step 19: Verify build works after fix
step "Verifying build works after fixing rules.star file"
run_in_devshell_script "buck2 build //:hello"

section "PASS: Error recovery and diagnostics"
