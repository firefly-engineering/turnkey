package main

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/rulessync"
	"github.com/firefly-engineering/turnkey/src/go/pkg/syncconfig"
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

// runRulesCheck checks if rules.star files need updates (dry-run mode).
func runRulesCheck(args []string) int {
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: %v\n", err)
		return 1
	}

	// Parse check-specific flags
	var targetDir string
	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--verbose", "-v":
			verbose = true
		case "--quiet", "-q":
			quiet = true
		default:
			// Assume it's a directory path
			if args[i] != "" && args[i][0] != '-' {
				targetDir = args[i]
			}
		}
	}

	// Determine directory to check
	dir := root
	if targetDir != "" {
		dir = filepath.Join(root, targetDir)
	}

	// Use new syncer in dry-run mode
	syncer, err := rulessync.NewSyncer(rulessync.Config{
		ProjectRoot: root,
		DryRun:      true,
		Verbose:     verbose,
	})
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: %v\n", err)
		return 1
	}

	results, err := syncer.SyncDirectory(dir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: check failed: %v\n", err)
		return 1
	}

	anyNeedsUpdate := false
	checkedCount := 0
	for _, result := range results {
		checkedCount++
		if result.Updated {
			anyNeedsUpdate = true
			relPath, _ := filepath.Rel(root, result.Path)
			fmt.Fprintf(os.Stderr, "NEEDS UPDATE: %s\n", relPath)
			if verbose {
				if len(result.Added) > 0 {
					fmt.Fprintf(os.Stderr, "         Would add: %v\n", result.Added)
				}
				if len(result.Removed) > 0 {
					fmt.Fprintf(os.Stderr, "         Would remove: %v\n", result.Removed)
				}
			}
		} else if verbose {
			relPath, _ := filepath.Rel(root, result.Path)
			fmt.Fprintf(os.Stderr, "OK:    %s\n", relPath)
		}

		// Report errors
		for _, e := range result.Errors {
			if verbose {
				fmt.Fprintf(os.Stderr, "       Warning: %s\n", e)
			}
		}
	}

	if anyNeedsUpdate {
		fmt.Fprintf(os.Stderr, "\ntk rules: some rules.star files need updates, run 'tk rules sync' to update\n")
		return 1
	}

	if !quiet {
		fmt.Fprintf(os.Stderr, "tk rules: all rules.star files up-to-date (%d checked)\n", checkedCount)
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
	var targetDir string

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--all", "-a":
			// --all is now the default behavior (always sync all)
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

	// Create syncer with new architecture
	syncer, err := rulessync.NewSyncer(rulessync.Config{
		ProjectRoot: root,
		DryRun:      dryRun,
		Verbose:     verbose,
	})
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk rules: %v\n", err)
		return 1
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

		// Report errors but don't count as failure if file was still updated
		if len(result.Errors) > 0 {
			for _, e := range result.Errors {
				if verbose {
					fmt.Fprintf(os.Stderr, "WARNING: %s: %s\n", relPath, e)
				}
			}
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

// runRulesAutoSync is called automatically before buck2 commands.
// It checks/syncs rules.star files based on configuration in sync.toml.
// Returns 0 on success, non-zero on failure.
func runRulesAutoSync() int {
	root, err := findProjectRoot()
	if err != nil {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: %v\n", err)
		}
		return 0 // Don't fail if we can't find project root
	}

	// Load sync config to check if rules sync is enabled
	cfg, err := syncconfig.LoadDefaultFrom(root)
	if err != nil {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: could not load sync config for rules: %v\n", err)
		}
		return 0 // Don't fail if config can't be loaded
	}

	// Skip if rules sync is not enabled
	if !cfg.Rules.Enabled {
		return 0
	}

	if verbose {
		fmt.Fprintln(os.Stderr, "tk: checking rules.star files...")
	}

	// Create syncer with new architecture
	syncer, err := rulessync.NewSyncer(rulessync.Config{
		ProjectRoot: root,
		DryRun:      cfg.Rules.Strict || strictRules, // Dry-run in strict mode
		Verbose:     verbose,
	})
	if err != nil {
		if verbose {
			fmt.Fprintf(os.Stderr, "tk: could not create rules syncer: %v\n", err)
		}
		return 0 // Don't fail on syncer creation issues
	}

	// Run sync
	results, err := syncer.SyncDirectory(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: rules sync failed: %v\n", err)
		return 1
	}

	// Count results
	updatedCount := 0
	for _, result := range results {
		if result.Updated {
			updatedCount++
		}
	}

	// No updates needed
	if updatedCount == 0 {
		if verbose && !quiet {
			fmt.Fprintln(os.Stderr, "tk: all rules.star files up-to-date")
		}
		return 0
	}

	// Handle strict mode (CI): fail if any rules.star would change
	if cfg.Rules.Strict || strictRules {
		fmt.Fprintf(os.Stderr, "tk: %d rules.star file(s) need updates (strict mode):\n", updatedCount)
		for _, result := range results {
			if result.Updated {
				relPath, _ := filepath.Rel(root, result.Path)
				fmt.Fprintf(os.Stderr, "  - %s\n", relPath)
			}
		}
		fmt.Fprintln(os.Stderr, "\ntk: run 'tk rules sync' locally and commit the changes")
		return 1
	}

	// Handle auto-sync disabled: just warn
	if !cfg.Rules.IsAutoSync() {
		if !quiet {
			fmt.Fprintf(os.Stderr, "tk: %d rules.star file(s) need updates:\n", updatedCount)
			for _, result := range results {
				if result.Updated {
					relPath, _ := filepath.Rel(root, result.Path)
					fmt.Fprintf(os.Stderr, "  - %s\n", relPath)
				}
			}
			fmt.Fprintln(os.Stderr, "tk: run 'tk rules sync' to update")
		}
		return 0 // Don't fail, just warn
	}

	// Auto-sync already happened, report results
	if !quiet {
		for _, result := range results {
			if result.Updated {
				relPath, _ := filepath.Rel(root, result.Path)
				fmt.Fprintf(os.Stderr, "tk: updated %s\n", relPath)
			}
		}
	}

	return 0
}
