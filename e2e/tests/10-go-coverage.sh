#!/usr/bin/env bash
# E2E Test: Go coverage fixture (a go.work monorepo)
#
# Runs the whole Go path on the go-coverage fixture (see its README.md):
# tk sync -> godeps cell -> rules sync -> buck2 build/test.
# 1. Initialize from the turnkey template and add the fixture
# 2. tk sync resolves the go.work workspace into go-deps.toml
# 3. Rules sync fills in every target's deps (the fixture ships them
#    empty), including a direct dep the build would also reach through a
#    dependency's own deps
# 4. Build everything and run the binary
# 5. Run the tests twice: the second run reuses every recorded result
# 6. Run the external test package's tests from the test binary, one of
#    them through a dep that imports the package under test
#
# Issues: https://github.com/firefly-engineering/turnkey/issues/208,
# https://github.com/firefly-engineering/turnkey/issues/226
set -euo pipefail

source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: Go coverage fixture (go.work monorepo)"

step "Creating test project directory"
PROJECT_DIR=$(setup_test_project "go-coverage")
cd "$PROJECT_DIR"

step "Initializing from turnkey template"
init_from_template

step "Adding the go-coverage fixture"
copy_fixture "go-coverage"
stage_for_flake

# Phase 1: resolve the workspace and sync the rules. go-deps.toml doesn't
# exist yet, so the shell has no godeps cell: rules sync only needs the
# deps file.
step "Resolving the workspace and syncing rules.star (batched)"
run_in_devshell_script << 'PHASE1'
  echo "Resolving the Go workspace..."
  tk sync

  echo ""
  echo "Checking rules.star before sync (must be stale)..."
  if tk rules check; then
    echo "ERROR: tk rules check passed on the fixture's empty deps" >&2
    exit 1
  fi

  echo ""
  echo "Syncing rules.star..."
  tk rules sync --all --verbose

  echo ""
  echo "Checking rules.star after sync..."
  tk rules check
PHASE1

step "Verifying go-deps.toml"
assert_file_exists "go-deps.toml" || exit 1
# Both members, and each third-party module, at the version go.mod asks for
assert_file_contains "go-deps.toml" '"example.com/app"' || exit 1
assert_file_contains "go-deps.toml" '"example.com/lib"' || exit 1
assert_file_contains "go-deps.toml" 'github.com/hashicorp/golang-lru/v2@v2.0.7' || exit 1
assert_file_contains "go-deps.toml" 'github.com/davecgh/go-spew@v1.1.2-0.20180830191138-d8f796af33cc' || exit 1
assert_file_contains "go-deps.toml" 'github.com/BurntSushi/toml@v1.5.0' || exit 1

step "Verifying the deps rules sync wrote"
# Imports within a member
assert_file_contains "lib/greet/rules.star" '"//lib/text:text"' || exit 1
assert_file_contains "app/cmd/hello/rules.star" '"//app/config:config"' || exit 1
assert_file_contains "app/cmd/hello/rules.star" '"//app/cache:cache"' || exit 1
assert_file_contains "app/cmd/hello/rules.star" '"//app/dump:dump"' || exit 1
# An import across members
assert_file_contains "app/cmd/hello/rules.star" '"//lib/greet:greet"' || exit 1
# Third-party: upper-case path, pseudo-version, /v2 module
assert_file_contains "app/config/rules.star" '"godeps//vendor/github.com/BurntSushi/toml:toml"' || exit 1
assert_file_contains "app/dump/rules.star" '"godeps//vendor/github.com/davecgh/go-spew/spew:spew"' || exit 1
assert_file_contains "app/cache/rules.star" '"godeps//vendor/github.com/hashicorp/golang-lru/v2:v2"' || exit 1
# A direct import the build would also find through lru's own deps
# (https://github.com/firefly-engineering/turnkey/issues/201): rules sync
# must still declare it
assert_file_contains "app/cache/rules.star" '"godeps//vendor/github.com/hashicorp/golang-lru/v2/simplelru:simplelru"' || exit 1
# The external test package's imports are the test's deps; its import of
# the package under test is the target_under_test, not a dep
assert_file_not_contains "lib/greet/rules.star" '"//lib/greet:greet"' || exit 1
# A test dep that imports the package under test
# (https://github.com/firefly-engineering/turnkey/issues/226)
assert_file_contains "lib/greet/rules.star" '"//lib/greet/greettest:greettest"' || exit 1
assert_file_contains "lib/greet/greettest/rules.star" '"//lib/greet:greet"' || exit 1
echo "rules.star deps synced from the imports"

step "Committing the synced state"
stage_for_flake
commit_changes "Resolve the workspace and sync rules.star"

# Phase 2: build, run and test with the godeps cell built from go-deps.toml
step "Building, running and testing (batched)"
run_output=$(run_in_devshell_script_capture << 'PHASE2'
  echo "Building everything..."
  tk build //...

  echo ""
  echo "=== hello output ==="
  tk run //app/cmd/hello:hello

  echo ""
  echo "=== first test run ==="
  # --rerun: results recorded by an earlier run of this test, on the same
  # inputs, would otherwise make this one a hit too
  tk --rerun test //...

  echo ""
  echo "=== second test run ==="
  tk test //...

  echo ""
  echo "=== external test package ==="
  # The test binary runs the external package's tests too
  "$(buck2 build //lib/greet:greet_test --show-full-simple-output)" -test.v -test.run 'TestHelloShouts$|TestHelloThroughHelper$'
PHASE2
) || {
  echo "$run_output" | tail -60
  exit 1
}
echo "$run_output" | tail -40

step "Verifying the binary's output"
assert_output_contains 'printf "%s\n" "$run_output"' "Hello, TURNKEY!" || exit 1
assert_output_contains 'printf "%s\n" "$run_output"' 'Name: (string) (len=7) "turnkey"' || exit 1

step "Verifying the external test package ran"
assert_output_contains 'printf "%s\n" "$run_output"' "^--- PASS: TestHelloShouts" || exit 1
# Through greettest, a dep that imports greet: go_test recompiles it against
# greet as built with its internal tests
# (https://github.com/firefly-engineering/turnkey/issues/226)
assert_output_contains 'printf "%s\n" "$run_output"' "^--- PASS: TestHelloThroughHelper" || exit 1

step "Verifying both test runs"
# Two go_test targets, run by the first run and reused by the second
first_run=$(echo "$run_output" | sed -n '/=== first test run ===/,/=== second test run ===/p')
second_run=$(echo "$run_output" | sed -n '/=== second test run ===/,/=== external test package ===/p')
assert_output_contains 'printf "%s\n" "$first_run"' "Pass 2.*Fail 0\. Timeout 0\. Fatal 0\." || exit 1
assert_output_contains 'printf "%s\n" "$first_run"' "^0 recorded (reused without running)" || exit 1
assert_output_contains 'printf "%s\n" "$second_run"' "Pass 2.*Fail 0\. Timeout 0\. Fatal 0\." || exit 1
assert_output_contains 'printf "%s\n" "$second_run"' "^2 recorded (reused without running)" || exit 1

section "PASS: Go coverage fixture (go.work monorepo)"
