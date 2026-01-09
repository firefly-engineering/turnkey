// tk is the turnkey CLI wrapper for buck2.
//
// It automatically runs sync operations before buck2 commands that read
// the build graph, ensuring generated files are up-to-date.
//
// Usage:
//
//	tk build //some:target     # syncs first, then runs buck2 build
//	tk test //some:target      # syncs first, then runs buck2 test
//	tk clean                   # passes through directly (no sync)
//	tk --no-sync build ...     # skip sync, run buck2 directly
//
// The command lists (syncFirstCommands and passThroughCommands) are
// configurable at the top of this file.
package main

import (
	"fmt"
	"os"
	"os/exec"
	"slices"
	"strings"
	"syscall"

	"github.com/firefly-engineering/turnkey/go/pkg/syncconfig"
	"github.com/firefly-engineering/turnkey/go/pkg/syncer"
)

// syncFirstCommands are buck2 subcommands that read the build graph.
// These commands will have sync run before them to ensure generated
// files (BUCK files, dependency cells) are up-to-date.
var syncFirstCommands = []string{
	"build",
	"run",
	"test",
	"query",
	"cquery",
	"uquery",
	"targets",
	"audit",
	"bxl",
}

// passThroughCommands are buck2 subcommands that don't read the build
// graph and can be passed through directly without syncing.
var passThroughCommands = []string{
	"clean",
	"kill",
	"killall",
	"status",
	"log",
	"rage",
	"help",
	"docs",
	"init",
}

// Flags
var (
	noSync  bool
	verbose bool
	dryRun  bool
)

func main() {
	args := os.Args[1:]

	// Parse tk-specific flags (before the subcommand)
	args = parseFlags(args)

	// If no subcommand, show help
	if len(args) == 0 {
		printHelp()
		os.Exit(0)
	}

	subcommand := args[0]

	// Handle tk-specific subcommands
	switch subcommand {
	case "sync":
		exitCode := runSync()
		os.Exit(exitCode)
	case "check":
		exitCode := runCheck()
		os.Exit(exitCode)
	}

	// Determine if this command needs sync first
	needsSync := shouldSync(subcommand)

	if needsSync && !noSync {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: syncing before %s...\n", subcommand)
		}
		if exitCode := runSync(); exitCode != 0 {
			os.Exit(exitCode)
		}
	}

	// Delegate to buck2
	delegateToBuck2(args)
}

// parseFlags extracts tk-specific flags from the beginning of args.
// Returns the remaining args after flags.
func parseFlags(args []string) []string {
	for len(args) > 0 {
		switch args[0] {
		case "--no-sync":
			noSync = true
			args = args[1:]
		case "--verbose", "-v":
			verbose = true
			args = args[1:]
		case "--dry-run", "-n":
			dryRun = true
			args = args[1:]
		case "--help", "-h":
			printHelp()
			os.Exit(0)
		default:
			// Not a tk flag, done parsing
			return args
		}
	}
	return args
}

// shouldSync returns true if the subcommand should have sync run first.
// Unknown commands default to requiring sync (safe default).
func shouldSync(subcommand string) bool {
	// Explicit pass-through commands don't need sync
	if slices.Contains(passThroughCommands, subcommand) {
		return false
	}

	// Sync-first commands and unknown commands need sync
	// Unknown commands default to sync (safe default)
	return true
}

// runSync runs the turnkey sync operation.
// Returns exit code (0 for success, non-zero for failure).
func runSync() int {
	// Find project root (where .buckconfig is)
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: %v\n", err)
		return 1
	}

	// Load configuration
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to load sync config: %v\n", err)
		return 1
	}

	// Validate configuration
	if err := cfg.Validate(); err != nil {
		fmt.Fprintf(os.Stderr, "tk: invalid sync config: %v\n", err)
		return 1
	}

	// Run sync
	s := syncer.New(cfg, root)
	s.Verbose = verbose
	s.DryRun = dryRun
	s.Output = os.Stderr

	result, err := s.SyncDeps()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: sync failed: %v\n", err)
		return 1
	}

	// Report results
	if len(result.Errors) > 0 {
		fmt.Fprintf(os.Stderr, "tk: sync completed with %d error(s):\n", len(result.Errors))
		for _, e := range result.Errors {
			fmt.Fprintf(os.Stderr, "  - %v\n", e)
		}
		return 1
	}

	if result.Synced > 0 {
		fmt.Fprintf(os.Stderr, "tk: synced %d file(s)\n", result.Synced)
	} else if verbose {
		fmt.Fprintln(os.Stderr, "tk: nothing to sync, all files up-to-date")
	}

	return 0
}

// runCheck checks if any files are stale without regenerating them.
// Returns exit code (0 if all up-to-date, 1 if stale).
func runCheck() int {
	// Find project root
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: %v\n", err)
		return 1
	}

	// Load configuration
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to load sync config: %v\n", err)
		return 1
	}

	// Run check
	s := syncer.New(cfg, root)
	s.Verbose = verbose
	s.Output = os.Stderr

	result, anyStale, err := s.Check()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: check failed: %v\n", err)
		return 1
	}

	// Report results
	if len(result.Errors) > 0 {
		fmt.Fprintf(os.Stderr, "tk: check completed with %d error(s):\n", len(result.Errors))
		for _, e := range result.Errors {
			fmt.Fprintf(os.Stderr, "  - %v\n", e)
		}
		return 1
	}

	if anyStale {
		fmt.Fprintln(os.Stderr, "tk: some files are stale, run 'tk sync' to update")
		return 1
	}

	fmt.Fprintln(os.Stderr, "tk: all files up-to-date")
	return 0
}

// findProjectRoot walks up from cwd to find the project root.
// The project root is identified by the presence of .buckconfig.
func findProjectRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", fmt.Errorf("failed to get working directory: %w", err)
	}

	for {
		if _, err := os.Stat(dir + "/.buckconfig"); err == nil {
			return dir, nil
		}

		parent := dir[:max(0, len(dir)-len("/"+dir[strings.LastIndex(dir, "/")+1:]))]
		if parent == dir || parent == "" {
			break
		}
		dir = parent
	}

	// Fallback to current directory
	cwd, _ := os.Getwd()
	return cwd, nil
}

// delegateToBuck2 executes buck2 with the given arguments.
// It uses exec to replace the current process with buck2.
func delegateToBuck2(args []string) {
	buck2Path, err := exec.LookPath("buck2")
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: buck2 not found in PATH: %v\n", err)
		os.Exit(1)
	}

	if verbose {
		fmt.Fprintf(os.Stderr, "tk: executing buck2 %v\n", args)
	}

	// Use syscall.Exec to replace this process with buck2
	// This ensures signals, stdio, etc. work correctly
	err = syscall.Exec(buck2Path, append([]string{"buck2"}, args...), os.Environ())
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: failed to exec buck2: %v\n", err)
		os.Exit(1)
	}
}

func printHelp() {
	fmt.Print(`tk - turnkey CLI wrapper for buck2

Usage: tk [tk-flags] <subcommand> [buck2-args...]

tk automatically runs sync operations before buck2 commands that read
the build graph, ensuring generated files are up-to-date.

tk-specific flags (must come before subcommand):
  --no-sync    Skip sync, run buck2 directly
  --verbose    Show what tk is doing
  -v           Same as --verbose
  --dry-run    Show what would be synced without doing it
  -n           Same as --dry-run
  --help       Show this help
  -h           Same as --help

tk-specific subcommands:
  sync         Run sync manually
  check        Check if files are stale without regenerating

All other subcommands are delegated to buck2.

Commands that sync first (read build graph):
  build, run, test, query, cquery, uquery, targets, audit, bxl

Commands that pass through directly (no sync):
  clean, kill, killall, status, log, rage, help, docs, init

Unknown commands default to syncing first (safe default).

Configuration:
  tk reads .turnkey/sync.toml for staleness rules.

Examples:
  tk build //some:target          # sync then build
  tk test //some:target           # sync then test
  tk --no-sync build //some:target # skip sync
  tk sync                         # just run sync
  tk check                        # check staleness
  tk clean                        # clean (no sync needed)
  tk --dry-run sync               # show what would be synced
`)
}
