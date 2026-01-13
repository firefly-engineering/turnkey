#!/usr/bin/env bash
# Setup and utility helpers for E2E tests
#
# These functions help set up test projects and run commands in devshells.

# Get the turnkey repo root (where flake.nix lives)
get_turnkey_root() {
  local dir="${BASH_SOURCE[0]}"
  # Navigate up from e2e/harness/lib/setup.sh to repo root
  dir="$(cd "$(dirname "$dir")/../../.." && pwd)"
  echo "$dir"
}

# Create a fresh test project directory with git initialized
# Usage: PROJECT_DIR=$(setup_test_project "my-test")
setup_test_project() {
  local name="${1:-test-project}"
  local project_dir="${TEST_WORKDIR}/${name}"

  mkdir -p "${project_dir}"
  cd "${project_dir}"

  # Initialize git (required for Nix flakes)
  git init --quiet
  git config user.email "e2e-test@turnkey.local"
  git config user.name "Turnkey E2E Test"

  echo "${project_dir}"
}

# Copy a fixture into the current directory (or specified destination)
# Usage: copy_fixture "greenfield-go" [destination]
copy_fixture() {
  local fixture_name="$1"
  local dest="${2:-.}"
  local fixture_path="${FIXTURES_DIR}/${fixture_name}"

  if [[ ! -d "${fixture_path}" ]]; then
    echo "ERROR: Fixture not found: ${fixture_name}" >&2
    echo "  Looked in: ${fixture_path}" >&2
    return 1
  fi

  cp -r "${fixture_path}/." "${dest}/"
  echo "Copied fixture: ${fixture_name}"
}

# Initialize a project from the turnkey template
# Usage: init_from_template [template_name]
init_from_template() {
  local template="${1:-default}"
  local turnkey_root
  turnkey_root="$(get_turnkey_root)"
  local template_dir="${turnkey_root}/templates/${template}"

  if [[ ! -d "$template_dir" ]]; then
    echo "ERROR: Template not found: $template_dir" >&2
    return 1
  fi

  # Copy template files directly (more reliable for testing)
  cp -r "${template_dir}/." .

  # Update flake.nix to use local turnkey path for testing
  # Replace github:firefly-engineering/turnkey with a path reference
  if [[ -f flake.nix ]]; then
    sed -i "s|github:firefly-engineering/turnkey|git+file://${turnkey_root}|g" flake.nix
  fi

  echo "Initialized from template: ${template}"
}

# Stage all files for flake (git add)
# Nix flakes only see tracked files
stage_for_flake() {
  git add -A
  echo "Staged all files for flake"
}

# Commit staged files
# Usage: commit_changes "commit message"
commit_changes() {
  local msg="${1:-Test commit}"
  git commit -m "$msg" --quiet
  echo "Committed: $msg"
}

# Run a command inside the devshell (headless, no direnv)
# Usage: run_in_devshell "command to run"
run_in_devshell() {
  local cmd="$1"

  # Use --no-pure-eval to allow IFD (import from derivation)
  # The flake.nix has already been patched to use local turnkey by init_from_template
  nix develop --no-pure-eval --command bash -c "$cmd"
}

# Run a command in the devshell, capturing output
# Usage: output=$(run_in_devshell_capture "command")
run_in_devshell_capture() {
  local cmd="$1"
  run_in_devshell "$cmd" 2>&1
}

# Check if a command exists in the devshell
# Usage: assert_command_in_devshell "buck2"
assert_command_in_devshell() {
  local cmd="$1"
  if run_in_devshell "command -v $cmd" >/dev/null 2>&1; then
    echo "Command available in devshell: $cmd"
    return 0
  else
    echo "ERROR: Command not found in devshell: $cmd" >&2
    return 1
  fi
}

# Wait for a condition with timeout
# Usage: wait_for "condition command" [timeout_seconds] [message]
wait_for() {
  local condition="$1"
  local timeout="${2:-30}"
  local msg="${3:-Waiting for condition}"
  local elapsed=0

  echo "$msg (timeout: ${timeout}s)"
  while ! eval "$condition" >/dev/null 2>&1; do
    sleep 1
    ((elapsed++))
    if [[ $elapsed -ge $timeout ]]; then
      echo "ERROR: Timeout waiting for: $condition" >&2
      return 1
    fi
  done
  echo "Condition met after ${elapsed}s"
  return 0
}

# Print a section header for test output
section() {
  local title="$1"
  echo ""
  echo "=== $title ==="
}

# Print a step within a section
step() {
  local desc="$1"
  echo "  -> $desc"
}

# Log verbose output (only shown with TURNKEY_VERBOSE)
log_verbose() {
  if [[ -n "${TURNKEY_VERBOSE:-}" ]]; then
    echo "[verbose] $*"
  fi
}
