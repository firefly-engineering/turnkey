#!/usr/bin/env bash
# Turnkey E2E Test Runner
#
# Usage:
#   turnkey-e2e-runner <test-name>    Run a specific test
#   turnkey-e2e-runner all            Run all tests
#   turnkey-e2e-runner --list         List available tests
#
# Environment variables:
#   TURNKEY_VERBOSE=1     Show verbose output
#   TURNKEY_KEEP_WORKDIR=1  Don't clean up test workdir on success

set -euo pipefail

# Determine script location and paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
E2E_DIR="$(dirname "$SCRIPT_DIR")"
TESTS_DIR="${E2E_DIR}/tests"
FIXTURES_DIR="${E2E_DIR}/fixtures"
LIB_DIR="${SCRIPT_DIR}/lib"

# Create unique log directory for this run
LOG_DIR="${TMPDIR:-/tmp}/turnkey-e2e-$(date +%Y%m%d-%H%M%S)-$$"

# Export for use by tests
export E2E_DIR TESTS_DIR FIXTURES_DIR LIB_DIR LOG_DIR

# Colors
if [[ -t 1 ]]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  YELLOW='\033[0;33m'
  BLUE='\033[0;34m'
  NC='\033[0m'
else
  RED='' GREEN='' YELLOW='' BLUE='' NC=''
fi

# Test registry: name -> script file
declare -A TESTS=(
  ["greenfield-template"]="01-greenfield-template.sh"
  ["ci-headless"]="02-ci-headless.sh"
  ["native-tools-sync"]="03-native-tools-sync.sh"
  ["multi-language"]="04-multi-language.sh"
  ["brownfield-adoption"]="05-brownfield-adoption.sh"
  ["git-workflow"]="06-git-workflow.sh"
  ["reproducibility"]="07-reproducibility.sh"
  ["error-recovery"]="08-error-recovery.sh"
)

usage() {
  cat <<EOF
Turnkey E2E Test Runner

Usage:
  $(basename "$0") <test-name>    Run a specific test
  $(basename "$0") all            Run all tests
  $(basename "$0") --list         List available tests
  $(basename "$0") --help         Show this help

Available tests:
$(for t in "${!TESTS[@]}"; do echo "  - $t"; done | sort)

Environment variables:
  TURNKEY_VERBOSE=1       Show verbose output during tests
  TURNKEY_KEEP_WORKDIR=1  Don't clean up test workdir on success

Example:
  $(basename "$0") greenfield-template
  TURNKEY_VERBOSE=1 $(basename "$0") all
EOF
}

# Run a single test
run_test() {
  local test_name="$1"
  local test_script="${TESTS[$test_name]}"
  local test_script_path="${TESTS_DIR}/${test_script}"
  local test_workdir="${LOG_DIR}/${test_name}"
  local test_log="${test_workdir}/output.log"

  # Check if test script exists
  if [[ ! -f "$test_script_path" ]]; then
    echo -e "${YELLOW}SKIP${NC} ${test_name}: Test script not implemented yet"
    return 2  # Skip code
  fi

  mkdir -p "${test_workdir}"

  echo -e "${BLUE}RUN${NC}  ${test_name}"
  echo "     Workdir: ${test_workdir}"
  echo "     Log: ${test_log}"

  # Export test context
  export TEST_NAME="${test_name}"
  export TEST_WORKDIR="${test_workdir}"

  # Source library functions
  source "${LIB_DIR}/assertions.sh"
  source "${LIB_DIR}/setup.sh"

  # Run test with logging
  local start_time
  start_time=$(date +%s)

  local exit_code=0
  if bash "$test_script_path" > "${test_log}" 2>&1; then
    exit_code=0
  else
    exit_code=$?
  fi

  local end_time
  end_time=$(date +%s)
  local duration=$((end_time - start_time))

  if [[ $exit_code -eq 0 ]]; then
    echo -e "${GREEN}PASS${NC} ${test_name} (${duration}s)"
    # Clean up on success unless TURNKEY_KEEP_WORKDIR is set
    if [[ -z "${TURNKEY_KEEP_WORKDIR:-}" ]]; then
      rm -rf "${test_workdir}"
    fi
    return 0
  else
    echo -e "${RED}FAIL${NC} ${test_name} (${duration}s)"
    echo ""
    echo "--- Last 30 lines of log ---"
    tail -30 "${test_log}" | sed 's/^/  /'
    echo "--- End of log ---"
    echo ""
    echo "Full log: ${test_log}"
    return 1
  fi
}

# List available tests
list_tests() {
  echo "Available tests:"
  for t in $(echo "${!TESTS[@]}" | tr ' ' '\n' | sort); do
    local script="${TESTS[$t]}"
    if [[ -f "${TESTS_DIR}/${script}" ]]; then
      echo "  $t"
    else
      echo "  $t (not implemented)"
    fi
  done
}

# Main entry point
main() {
  local arg="${1:-}"

  case "${arg}" in
    --help|-h|"")
      usage
      exit 0
      ;;
    --list|-l)
      list_tests
      exit 0
      ;;
    all)
      mkdir -p "${LOG_DIR}"
      echo "Running all E2E tests..."
      echo "Log directory: ${LOG_DIR}"
      echo ""

      local total=0
      local passed=0
      local failed=0
      local skipped=0

      for test_name in $(echo "${!TESTS[@]}" | tr ' ' '\n' | sort); do
        ((++total))
        local result=0
        run_test "${test_name}" || result=$?
        case $result in
          0) ((++passed)) ;;
          2) ((++skipped)) ;;
          *) ((++failed)) ;;
        esac
        echo ""
      done

      echo "=========================================="
      echo "E2E Test Summary"
      echo "=========================================="
      echo -e "  Total:   ${total}"
      echo -e "  ${GREEN}Passed:${NC}  ${passed}"
      echo -e "  ${RED}Failed:${NC}  ${failed}"
      echo -e "  ${YELLOW}Skipped:${NC} ${skipped}"
      echo ""

      if [[ $failed -gt 0 ]]; then
        echo "Log directory preserved: ${LOG_DIR}"
        exit 1
      else
        # Clean up log dir if all passed
        if [[ -z "${TURNKEY_KEEP_WORKDIR:-}" ]]; then
          rm -rf "${LOG_DIR}"
        fi
        exit 0
      fi
      ;;
    *)
      # Run specific test
      if [[ ! -v "TESTS[${arg}]" ]]; then
        echo "Error: Unknown test '${arg}'"
        echo ""
        list_tests
        exit 1
      fi
      mkdir -p "${LOG_DIR}"
      run_test "${arg}"
      ;;
  esac
}

main "$@"
