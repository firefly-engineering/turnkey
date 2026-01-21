package main

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/firefly-engineering/turnkey/src/go/pkg/rules"
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

// runRulesCheck checks if rules.star files are stale.
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

	// Build config from sync.toml settings
	config := rules.SyncConfig{
		ProjectRoot: root,
		Enabled:     cfg.Rules.Enabled,
		AutoSync:    cfg.Rules.IsAutoSync(),
		Strict:      cfg.Rules.Strict || strictRules,
		DryRun:      dryRun,
		Go: rules.GoSyncConfig{
			Enabled:        cfg.Rules.IsGoEnabled(),
			InternalPrefix: cfg.Rules.Go.InternalPrefix,
			ExternalCell:   cfg.Rules.Go.ExternalCell,
		},
	}

	// Apply defaults if not specified
	if config.Go.InternalPrefix == "" {
		config.Go.InternalPrefix = "//src/go"
	}
	if config.Go.ExternalCell == "" {
		config.Go.ExternalCell = "godeps"
	}

	syncer := rules.NewSyncer(config)

	// First check what's stale
	staleResults, err := syncer.Checker.CheckDirectory(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: rules check failed: %v\n", err)
		return 1
	}

	// Find stale files
	var staleFiles []*rules.StalenessResult
	for _, r := range staleResults {
		if r.Stale {
			staleFiles = append(staleFiles, r)
		}
	}

	if len(staleFiles) == 0 {
		if verbose && !quiet {
			fmt.Fprintln(os.Stderr, "tk: all rules.star files up-to-date")
		}
		return 0
	}

	// Handle strict mode (CI): fail if any rules.star would change
	if config.Strict {
		fmt.Fprintf(os.Stderr, "tk: %d rules.star file(s) are stale (strict mode):\n", len(staleFiles))
		for _, r := range staleFiles {
			relPath, _ := filepath.Rel(root, r.RulesFile)
			fmt.Fprintf(os.Stderr, "  - %s: %s\n", relPath, r.Reason)
		}
		fmt.Fprintln(os.Stderr, "\ntk: run 'tk rules sync' locally and commit the changes")
		return 1
	}

	// Handle auto-sync disabled: just warn
	if !config.AutoSync {
		if !quiet {
			fmt.Fprintf(os.Stderr, "tk: %d rules.star file(s) are stale:\n", len(staleFiles))
			for _, r := range staleFiles {
				relPath, _ := filepath.Rel(root, r.RulesFile)
				fmt.Fprintf(os.Stderr, "  - %s\n", relPath)
			}
			fmt.Fprintln(os.Stderr, "tk: run 'tk rules sync' to update")
		}
		return 0 // Don't fail, just warn
	}

	// Auto-sync: update stale files
	if verbose {
		fmt.Fprintf(os.Stderr, "tk: syncing %d stale rules.star file(s)...\n", len(staleFiles))
	}

	results, err := syncer.SyncDirectory(root)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk: rules sync failed: %v\n", err)
		return 1
	}

	// Report results
	updatedCount := 0
	errorCount := 0

	for _, result := range results {
		if len(result.Errors) > 0 {
			errorCount++
			relPath, _ := filepath.Rel(root, result.Path)
			fmt.Fprintf(os.Stderr, "tk: rules sync error in %s: %v\n", relPath, result.Errors)
			continue
		}
		if result.Updated {
			updatedCount++
			if !quiet {
				relPath, _ := filepath.Rel(root, result.Path)
				fmt.Fprintf(os.Stderr, "tk: updated %s\n", relPath)
			}
		}
	}

	if errorCount > 0 {
		return 1
	}

	return 0
}
