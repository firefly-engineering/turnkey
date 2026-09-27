// tw is the turnkey wrapper for native language tools.
//
// It transparently wraps tools like go, cargo, and uv, detecting when they
// modify dependency files and automatically triggering sync operations.
//
// Usage:
//
//	tw go get github.com/foo/bar    # runs go get, syncs if go.mod changed
//	tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
//	tw uv add requests              # runs uv add, syncs if pyproject.toml changed
//
// Wrapper rules are read from .turnkey/sync.toml, which turnkey generates
// from its language records (nix/buck2/languages.nix):
//
//	[[wrappers]]
//	name = "go"
//	command = "go"
//	mutating_subcommands = ["get", "mod"]
//	watch_files = ["go.mod", "go.sum"]
//	deps_rule = "go"
//
// A tool without a wrapper rule is passed through untouched.
package main

import (
	"fmt"
	"os"

	"github.com/firefly-engineering/turnkey/src/go/pkg/wrap"
)

var (
	verbose bool
	noSync  bool
)

func main() {
	args := parseFlags(os.Args[1:])
	if len(args) == 0 {
		printHelp()
		os.Exit(0)
	}

	cwd, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tw: failed to get working directory: %v\n", err)
		os.Exit(wrap.RealExec("", args[0], args[1:]))
	}
	w := wrap.Open(cwd, wrap.RealExec, os.Stderr)
	w.Verbose = verbose
	w.NoSync = noSync
	os.Exit(w.Run(args[0], args[1:]))
}

// parseFlags extracts tw-specific flags from the beginning of args.
func parseFlags(args []string) []string {
	for len(args) > 0 {
		switch args[0] {
		case "--verbose", "-v":
			verbose = true
			args = args[1:]
		case "--no-sync":
			noSync = true
			args = args[1:]
		case "--help", "-h":
			printHelp()
			os.Exit(0)
		default:
			return args
		}
	}
	return args
}

func printHelp() {
	fmt.Print(`tw - turnkey wrapper for native language tools

Usage: tw [tw-flags] <tool> [tool-args...]

tw transparently wraps native language tools (go, cargo, uv), detecting when
they modify dependency files and automatically triggering sync operations.

tw-specific flags (must come before tool name):
  --verbose    Show what tw is doing
  -v           Same as --verbose
  --no-sync    Disable automatic sync after tool runs
  --help       Show this help
  -h           Same as --help

Examples:
  tw go get github.com/foo/bar    # runs go get, then go mod tidy, then sync
  tw cargo add serde              # runs cargo add, syncs if Cargo.lock changed
  tw uv add requests              # runs uv add, syncs if pyproject.toml changed
  tw go build ./...               # just runs go build (not a mutating command)
  tw --no-sync go get foo         # runs go get without post-commands or sync

Default behavior for Go:
  After 'go get' or 'go mod' commands, tw automatically runs 'go mod tidy'
  to ensure direct/indirect dependencies are correctly classified before
  syncing go-deps.toml.

Configuration:
  tw reads wrapper rules from .turnkey/sync.toml:

  [[wrappers]]
  name = "go"
  command = "go"
  mutating_subcommands = ["get", "mod"]
  watch_files = ["go.mod", "go.sum"]
  deps_rule = "go"
  post_commands = ["go mod tidy"]  # run after main command, before sync

Environment:
  TURNKEY_NO_WRAP=1   Bypass tw wrapper entirely (use real tool)
`)
}
