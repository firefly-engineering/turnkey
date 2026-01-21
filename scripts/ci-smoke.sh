#!/usr/bin/env bash
# CI Smoke Test Suite for Turnkey
#
# Fast feedback loop (~3 min) for PRs that catches regressions
# without running the full e2e suite.
#
# Usage:
#   ./scripts/ci-smoke.sh           # Run all tiers
#   ./scripts/ci-smoke.sh --tier 1  # Run specific tier only
#   ./scripts/ci-smoke.sh --help    # Show help

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Timing
SECONDS=0

# Parse arguments
TIER=""
VERBOSE=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --tier)
            TIER="$2"
            shift 2
            ;;
        --verbose|-v)
            VERBOSE="1"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --tier N     Run only tier N (1, 2, or 3)"
            echo "  --verbose    Show detailed output"
            echo "  --help       Show this help"
            echo ""
            echo "Tiers:"
            echo "  1: Syntax & Lint (nix flake check, pre-commit, starlark)"
            echo "  2: Build Verification (prelude, tools, devshell)"
            echo "  3: Integration (buck2 build/test for each language)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

log_error() {
    echo -e "${RED}[FAIL]${NC} $1"
}

log_section() {
    echo ""
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}  $1${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

run_cmd() {
    local desc="$1"
    shift
    log_info "$desc"
    if [[ -n "$VERBOSE" ]]; then
        if "$@"; then
            log_success "$desc"
            return 0
        else
            log_error "$desc"
            return 1
        fi
    else
        if "$@" > /dev/null 2>&1; then
            log_success "$desc"
            return 0
        else
            log_error "$desc"
            # Show output on failure
            "$@" 2>&1 || true
            return 1
        fi
    fi
}

# ==============================================================================
# Tier 1: Syntax & Lint
# ==============================================================================
tier1() {
    log_section "Tier 1: Syntax & Lint"
    local failed=0

    run_cmd "Nix flake check (syntax only)" \
        nix flake check --no-build --impure || ((failed++))

    # Pre-commit hooks (if available)
    if command -v pre-commit &> /dev/null; then
        run_cmd "Pre-commit hooks" \
            pre-commit run --all-files || ((failed++))
    else
        log_info "Skipping pre-commit (not installed)"
    fi

    return $failed
}

# ==============================================================================
# Tier 2: Build Verification
# ==============================================================================
tier2() {
    log_section "Tier 2: Build Verification"
    local failed=0

    run_cmd "Build turnkey-prelude" \
        nix build .#turnkey-prelude --no-link || ((failed++))

    run_cmd "Build tk (Buck2 wrapper)" \
        nix build .#tk --no-link || ((failed++))

    run_cmd "Build godeps-gen" \
        nix build .#godeps-gen --no-link || ((failed++))

    run_cmd "Devshell enters successfully" \
        nix develop --impure -c true || ((failed++))

    return $failed
}

# ==============================================================================
# Tier 3: Integration (Buck2 builds and tests)
# ==============================================================================
tier3() {
    log_section "Tier 3: Integration (Buck2 builds & tests)"
    local failed=0

    # Go
    run_cmd "Go: build go-hello" \
        buck2 build //src/examples/go-hello:go-hello || ((failed++))

    # Rust
    run_cmd "Rust: build rust-hello" \
        buck2 build //src/examples/rust-hello:rust-hello || ((failed++))
    run_cmd "Rust: test rust-hello" \
        buck2 test //src/examples/rust-hello:rust-hello-test || ((failed++))

    # Python
    run_cmd "Python: build python-hello" \
        buck2 build //src/examples/python-hello:python-hello || ((failed++))
    run_cmd "Python: test python-hello" \
        buck2 test //src/examples/python-hello:python-hello-test || ((failed++))

    # TypeScript
    run_cmd "TypeScript: build typescript-hello" \
        buck2 build //src/examples/typescript-hello:typescript-hello || ((failed++))

    # Solidity
    run_cmd "Solidity: build counter" \
        buck2 build //src/examples/solidity-hello:counter || ((failed++))
    run_cmd "Solidity: test counter" \
        buck2 test //src/examples/solidity-hello:counter_test || ((failed++))

    # Jsonnet
    run_cmd "Jsonnet: build config-dev" \
        buck2 build //src/examples/jsonnet-config:config-dev || ((failed++))
    run_cmd "Jsonnet: test common" \
        buck2 test //src/examples/jsonnet-config:common-test || ((failed++))

    return $failed
}

# ==============================================================================
# Main
# ==============================================================================
main() {
    log_section "Turnkey CI Smoke Tests"
    log_info "Starting smoke test suite..."

    local total_failed=0

    if [[ -z "$TIER" || "$TIER" == "1" ]]; then
        tier1 || ((total_failed += $?))
    fi

    if [[ -z "$TIER" || "$TIER" == "2" ]]; then
        tier2 || ((total_failed += $?))
    fi

    if [[ -z "$TIER" || "$TIER" == "3" ]]; then
        tier3 || ((total_failed += $?))
    fi

    # Summary
    log_section "Summary"
    local elapsed=$SECONDS
    local mins=$((elapsed / 60))
    local secs=$((elapsed % 60))

    if [[ $total_failed -eq 0 ]]; then
        log_success "All smoke tests passed in ${mins}m ${secs}s"
        exit 0
    else
        log_error "$total_failed test(s) failed in ${mins}m ${secs}s"
        exit 1
    fi
}

main
