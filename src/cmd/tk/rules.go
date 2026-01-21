package main

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/rules"
)

// runRules handles the "tk rules" subcommand.
// Usage:
//
//	tk rules check              # Check if rules.star files are stale
//	tk rules sync               # Update stale rules.star files
//	tk rules sync --all         # Force update all rules.star files
//	tk rules sync path/to/dir   # Update specific directory
func runRules(args []string) int {
	if len(args) == 0 {
		printRulesHelp()
		return 0
	}

	subcmd := args[0]
	subargs := args[1:]

	switch subcmd {
	case "check":
		return runRulesCheck(subargs)
	case "sync":
		return runRulesSync(subargs)
	case "help", "--help", "-h":
		printRulesHelp()
		return 0
	default:
		fmt.Fprintf(os.Stderr, "tk rules: unknown subcommand %q\n", subcmd)
		printRulesHelp()
		return 1
	}
}

// runRulesCheck checks if rules.star files are stale.
func runRulesCheck(args []string) int {
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: %v\n", err)
		return 1
	}

	// Determine directory to check
	dir := root
	if len(args) > 0 && args[0] != "" && args[0][0] != '-' {
		dir = filepath.Join(root, args[0])
	}

	checker := rules.NewStalenessChecker(root)
	results, err := checker.CheckDirectory(dir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: check failed: %v\n", err)
		return 1
	}

	anyStale := false
	for _, result := range results {
		if result.Stale {
			anyStale = true
			relPath, _ := filepath.Rel(root, result.RulesFile)
			fmt.Fprintf(os.Stderr, "STALE: %s\n", relPath)
			if verbose {
				fmt.Fprintf(os.Stderr, "       Reason: %s (tier %d)\n", result.Reason, result.Tier)
				if len(result.ChangedFiles) > 0 {
					fmt.Fprintf(os.Stderr, "       Changed: %v\n", result.ChangedFiles)
				}
			}
		} else if verbose {
			relPath, _ := filepath.Rel(root, result.RulesFile)
			fmt.Fprintf(os.Stderr, "OK:    %s\n", relPath)
		}
	}

	if anyStale {
		fmt.Fprintf(os.Stderr, "\ntk rules: some rules.star files are stale, run 'tk rules sync' to update\n")
		return 1
	}

	if !quiet {
		fmt.Fprintf(os.Stderr, "tk rules: all rules.star files up-to-date (%d checked)\n", len(results))
	}
	return 0
}

// runRulesSync synchronizes rules.star files.
func runRulesSync(args []string) int {
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: %v\n", err)
		return 1
	}

	// Parse sync-specific flags
	forceAll := false
	var targetDir string

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--all", "-a":
			forceAll = true
		case "--verbose", "-v":
			verbose = true
		case "--quiet", "-q":
			quiet = true
		case "--dry-run", "-n":
			dryRun = true
		default:
			// Assume it's a directory path
			if args[i] != "" && args[i][0] != '-' {
				targetDir = args[i]
			}
		}
	}

	// Determine directory to sync
	dir := root
	if targetDir != "" {
		dir = filepath.Join(root, targetDir)
	}

	// Create sync config
	config := rules.SyncConfig{
		ProjectRoot: root,
		Enabled:     true,
		AutoSync:    true,
		Strict:      false, // Strict mode is for CI (--strict-rules flag)
		DryRun:      dryRun,
		Go: rules.GoSyncConfig{
			Enabled:        true,
			InternalPrefix: "//src/go",
			ExternalCell:   "godeps",
		},
	}

	syncer := rules.NewSyncer(config)

	// Check staleness first if not forcing all
	if !forceAll {
		staleResults, err := syncer.Checker.CheckDirectory(dir)
		if err != nil {
			fmt.Fprintf(os.Stderr, "tk rules: check failed: %v\n", err)
			return 1
		}

		anyStale := false
		for _, r := range staleResults {
			if r.Stale {
				anyStale = true
				break
			}
		}

		if !anyStale {
			if !quiet {
				fmt.Fprintf(os.Stderr, "tk rules: all rules.star files up-to-date\n")
			}
			return 0
		}
	}

	// Run sync
	results, err := syncer.SyncDirectory(dir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: sync failed: %v\n", err)
		return 1
	}

	// Report results
	updatedCount := 0
	errorCount := 0

	for _, result := range results {
		relPath, _ := filepath.Rel(root, result.Path)

		if len(result.Errors) > 0 {
			errorCount++
			fmt.Fprintf(os.Stderr, "ERROR: %s\n", relPath)
			for _, e := range result.Errors {
				fmt.Fprintf(os.Stderr, "       %s\n", e)
			}
			continue
		}

		if result.Updated {
			updatedCount++
			if dryRun {
				fmt.Fprintf(os.Stderr, "WOULD UPDATE: %s\n", relPath)
			} else {
				fmt.Fprintf(os.Stderr, "UPDATED: %s\n", relPath)
			}
			if verbose {
				if len(result.Added) > 0 {
					fmt.Fprintf(os.Stderr, "         Added: %v\n", result.Added)
				}
				if len(result.Removed) > 0 {
					fmt.Fprintf(os.Stderr, "         Removed: %v\n", result.Removed)
				}
				if len(result.Preserved) > 0 {
					fmt.Fprintf(os.Stderr, "         Preserved: %v\n", result.Preserved)
				}
			}
		} else if verbose {
			fmt.Fprintf(os.Stderr, "OK: %s (no changes)\n", relPath)
		}
	}

	// Summary
	if !quiet {
		if dryRun {
			fmt.Fprintf(os.Stderr, "\ntk rules: would update %d file(s)\n", updatedCount)
		} else {
			fmt.Fprintf(os.Stderr, "\ntk rules: updated %d file(s)\n", updatedCount)
		}
	}

	if errorCount > 0 {
		return 1
	}
	return 0
}

// printRulesHelp prints help for the rules subcommand.
func printRulesHelp() {
	fmt.Fprintln(os.Stderr, `Usage: tk rules <command> [options] [path]

Commands:
  check              Check if rules.star files are stale
  sync               Update stale rules.star files
  help               Show this help

Options:
  --all, -a          Sync all files (not just stale ones)
  --verbose, -v      Show detailed output
  --quiet, -q        Suppress output
  --dry-run, -n      Show what would be changed without writing

Examples:
  tk rules check                    # Check all rules.star files
  tk rules check src/cmd/tk         # Check specific directory
  tk rules sync                     # Update stale rules.star files
  tk rules sync --all               # Force update all files
  tk rules sync src/cmd/tk          # Sync specific directory

The rules command automatically detects imports from source files and
updates the deps list in rules.star. Manual dependencies can be preserved
using turnkey:preserve-start/end markers.`)
}
