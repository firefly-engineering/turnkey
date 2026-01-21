# Turnkey E2E Tests

End-to-end tests for validating turnkey workflows.

## Running Tests

### Run a specific test
```bash
./e2e/harness/runner.sh greenfield-template
```

### Run all tests
```bash
./e2e/harness/runner.sh all
```

### List available tests
```bash
./e2e/harness/runner.sh --list
```

### Verbose output
```bash
TURNKEY_VERBOSE=1 ./e2e/harness/runner.sh greenfield-template
```

### Keep test workdir on success (for debugging)
```bash
TURNKEY_KEEP_WORKDIR=1 ./e2e/harness/runner.sh greenfield-template
```

## Directory Structure

```
e2e/
├── harness/
│   ├── runner.sh           # Main test runner
│   └── lib/
│       ├── assertions.sh   # assert_* helper functions
│       └── setup.sh        # setup_test_project, run_in_devshell
├── fixtures/
│   ├── greenfield-go/      # Minimal Go project
│   └── multi-language/     # Go + Rust + Python
└── tests/
    ├── 01-greenfield-template.sh
    ├── 02-ci-headless.sh
    └── ...
```

## Test Scenarios

| Test | Description | Issue |
|------|-------------|-------|
| greenfield-template | New project from template | turnkey-1us |
| ci-headless | Non-interactive CI execution | turnkey-2t5 |
| native-tools-sync | go get/cargo add syncs deps | turnkey-66b |
| multi-language | Go + Rust + Python monorepo | turnkey-tps |
| brownfield-adoption | Add turnkey to existing project | turnkey-njc |
| git-workflow | Branch switching with deps | turnkey-m5t |
| reproducibility | Same build across machines | turnkey-s52 |
| error-recovery | Error handling and recovery | turnkey-dw7 |
| rules-star-sync | Auto-sync rules.star deps | turnkey-rlv3 |

## Writing Tests

Tests are shell scripts that use the assertion library:

```bash
#!/usr/bin/env bash
set -euo pipefail

source "${LIB_DIR}/assertions.sh"
source "${LIB_DIR}/setup.sh"

section "Test: My Test"

# Create isolated test project
PROJECT_DIR=$(setup_test_project "my-test")
cd "$PROJECT_DIR"

# Initialize from template
init_from_template

# Run assertions
assert_file_exists "flake.nix" || exit 1
assert_command_in_devshell "buck2" || exit 1

# Run commands in devshell
run_in_devshell "buck2 build //..."

section "PASS: My Test"
```

## CI Integration

Tests run automatically in GitHub Actions on PRs and pushes to main.
See `.github/workflows/ci.yaml` for the workflow configuration.
