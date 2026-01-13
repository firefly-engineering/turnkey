#!/usr/bin/env bash
# Assertion helpers for E2E tests
#
# All assertions print a message on failure and return 1.
# Usage: assert_* "args" || return 1

# Colors for output (disabled if not a terminal)
if [[ -t 1 ]]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  NC='\033[0m' # No Color
else
  RED=''
  GREEN=''
  NC=''
fi

# Internal: print failure message
_assert_fail() {
  echo -e "${RED}ASSERTION FAILED:${NC} $1" >&2
}

# Internal: print success message (only in verbose mode)
_assert_pass() {
  if [[ -n "${TURNKEY_VERBOSE:-}" ]]; then
    echo -e "${GREEN}ASSERT OK:${NC} $1"
  fi
}

# Assert a file exists
# Usage: assert_file_exists "/path/to/file" ["optional message"]
assert_file_exists() {
  local file="$1"
  local msg="${2:-File should exist: $file}"
  if [[ -e "$file" ]]; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}

# Assert a file does NOT exist
assert_file_not_exists() {
  local file="$1"
  local msg="${2:-File should not exist: $file}"
  if [[ ! -e "$file" ]]; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}

# Assert a file is a symlink
assert_is_symlink() {
  local file="$1"
  local msg="${2:-Should be a symlink: $file}"
  if [[ -L "$file" ]]; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}

# Assert a directory exists
assert_dir_exists() {
  local dir="$1"
  local msg="${2:-Directory should exist: $dir}"
  if [[ -d "$dir" ]]; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}

# Assert a command succeeds (exit code 0)
# Usage: assert_command_succeeds "command to run" ["optional message"]
assert_command_succeeds() {
  local cmd="$1"
  local msg="${2:-Command should succeed: $cmd}"
  if eval "$cmd" >/dev/null 2>&1; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}

# Assert a command fails (non-zero exit code)
assert_command_fails() {
  local cmd="$1"
  local msg="${2:-Command should fail: $cmd}"
  if eval "$cmd" >/dev/null 2>&1; then
    _assert_fail "$msg"
    return 1
  else
    _assert_pass "$msg"
    return 0
  fi
}

# Assert file contains a pattern (grep)
# Usage: assert_file_contains "/path/to/file" "pattern" ["optional message"]
assert_file_contains() {
  local file="$1"
  local pattern="$2"
  local msg="${3:-File $file should contain: $pattern}"
  if [[ ! -f "$file" ]]; then
    _assert_fail "File does not exist: $file"
    return 1
  fi
  if grep -q "$pattern" "$file"; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}

# Assert file does NOT contain a pattern
assert_file_not_contains() {
  local file="$1"
  local pattern="$2"
  local msg="${3:-File $file should not contain: $pattern}"
  if [[ ! -f "$file" ]]; then
    _assert_fail "File does not exist: $file"
    return 1
  fi
  if grep -q "$pattern" "$file"; then
    _assert_fail "$msg"
    return 1
  else
    _assert_pass "$msg"
    return 0
  fi
}

# Assert command output contains a pattern
# Usage: assert_output_contains "command" "pattern" ["optional message"]
assert_output_contains() {
  local cmd="$1"
  local pattern="$2"
  local msg="${3:-Output of '$cmd' should contain: $pattern}"
  local output
  output=$(eval "$cmd" 2>&1) || true
  if echo "$output" | grep -q "$pattern"; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    echo "Actual output:" >&2
    echo "$output" | head -20 >&2
    return 1
  fi
}

# Assert command output does NOT contain a pattern
assert_output_not_contains() {
  local cmd="$1"
  local pattern="$2"
  local msg="${3:-Output of '$cmd' should not contain: $pattern}"
  local output
  output=$(eval "$cmd" 2>&1) || true
  if echo "$output" | grep -q "$pattern"; then
    _assert_fail "$msg"
    return 1
  else
    _assert_pass "$msg"
    return 0
  fi
}

# Assert two files are identical
assert_files_equal() {
  local file1="$1"
  local file2="$2"
  local msg="${3:-Files should be identical: $file1 vs $file2}"
  if diff -q "$file1" "$file2" >/dev/null 2>&1; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    echo "Diff:" >&2
    diff "$file1" "$file2" >&2 || true
    return 1
  fi
}

# Assert a string equals expected value
# Usage: assert_equals "actual" "expected" ["optional message"]
assert_equals() {
  local actual="$1"
  local expected="$2"
  local msg="${3:-Values should be equal}"
  if [[ "$actual" == "$expected" ]]; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    echo "  Expected: $expected" >&2
    echo "  Actual:   $actual" >&2
    return 1
  fi
}

# Assert a string is not empty
assert_not_empty() {
  local value="$1"
  local msg="${2:-Value should not be empty}"
  if [[ -n "$value" ]]; then
    _assert_pass "$msg"
    return 0
  else
    _assert_fail "$msg"
    return 1
  fi
}
