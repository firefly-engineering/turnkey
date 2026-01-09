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
	"syscall"
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
		runSync()
		return
	case "check":
		runCheck()
		return
	}

	// Determine if this command needs sync first
	needsSync := shouldSync(subcommand)

	if needsSync && !noSync {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: syncing before %s...\n", subcommand)
		}
		runSync()
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
func runSync() {
	if verbose {
		fmt.Fprintln(os.Stderr, "tk: running sync...")
	}

	// TODO: Implement actual sync logic
	// This will call into the staleness package and regenerate
	// files as needed.
	//
	// For now, this is a stub that does nothing.

	if verbose {
		fmt.Fprintln(os.Stderr, "tk: sync complete (stub)")
	}
}

// runCheck checks if any files are stale without regenerating them.
func runCheck() {
	if verbose {
		fmt.Fprintln(os.Stderr, "tk: checking staleness...")
	}

	// TODO: Implement actual check logic
	// This will call into the staleness package to report
	// which files need regeneration.
	//
	// For now, this is a stub.

	fmt.Println("tk check: all files up-to-date (stub)")
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

Examples:
  tk build //some:target          # sync then build
  tk test //some:target           # sync then test
  tk --no-sync build //some:target # skip sync
  tk sync                         # just run sync
  tk check                        # check staleness
  tk clean                        # clean (no sync needed)
`)
}
