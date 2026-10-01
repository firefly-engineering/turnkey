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
//	tk rules check              # Check every rules.star file
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
	return checkRules(root, args)
}

// checkRules checks every rules.star under root, or under the directory
// args names. It ignores git status and file times, which only tell what
// changed since the last commit: a stale rules.star that was committed is
// still stale. --all and --force are accepted and change nothing.
func checkRules(root string, args []string) int {
	// Parse check-specific flags
	var targetDir string
	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--all", "-a", "--force", "-f":
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
		Force:       true,
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
	anyUnreadable := false
	checkedCount := 0
	skippedCount := 0
	for _, result := range results {
		if result.Skipped {
			skippedCount++
			if verbose {
				relPath, _ := filepath.Rel(root, result.Path)
				fmt.Fprintf(os.Stderr, "SKIPPED: %s (up-to-date)\n", relPath)
			}
			continue
		}
		checkedCount++
		if !quiet {
			relPath, _ := filepath.Rel(root, result.Path)
			printKeptDeps(relPath, result.Changes)
			printSkippedTargets(relPath, result)
		}
		if len(result.Unreadable) > 0 {
			anyUnreadable = true
		}
		if result.Updated {
			anyNeedsUpdate = true
			relPath, _ := filepath.Rel(root, result.Path)
			fmt.Fprintf(os.Stderr, "NEEDS UPDATE: %s\n", relPath)
			if verbose {
				printTargetChanges(result.Changes, "Would add", "Would remove")
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
	if anyUnreadable {
		fmt.Fprintf(os.Stderr, "\ntk rules: some targets' deps can't be synced (see UNREADABLE above)\n")
		return 1
	}

	if !quiet {
		fmt.Fprintf(os.Stderr, "tk rules: all rules.star files up-to-date (%d checked", checkedCount)
		if skippedCount > 0 {
			fmt.Fprintf(os.Stderr, ", %d skipped", skippedCount)
		}
		fmt.Fprintln(os.Stderr, ")")
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
	var forceSync bool

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--all", "-a", "--force", "-f":
			forceSync = true
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
		Force:       forceSync,
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
	skippedCount := 0
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

		if !quiet {
			printKeptDeps(relPath, result.Changes)
			printSkippedTargets(relPath, result)
		}

		if result.Skipped {
			skippedCount++
			if verbose {
				fmt.Fprintf(os.Stderr, "SKIPPED: %s (up-to-date)\n", relPath)
			}
		} else if result.Updated {
			updatedCount++
			if dryRun {
				fmt.Fprintf(os.Stderr, "WOULD UPDATE: %s\n", relPath)
			} else {
				fmt.Fprintf(os.Stderr, "UPDATED: %s\n", relPath)
			}
			if verbose {
				printTargetChanges(result.Changes, "Added", "Removed")
			}
		} else if verbose {
			fmt.Fprintf(os.Stderr, "OK: %s (no changes)\n", relPath)
		}
	}

	// Summary
	if !quiet {
		if dryRun {
			fmt.Fprintf(os.Stderr, "\ntk rules: would update %d file(s)", updatedCount)
		} else {
			fmt.Fprintf(os.Stderr, "\ntk rules: updated %d file(s)", updatedCount)
		}
		if skippedCount > 0 {
			fmt.Fprintf(os.Stderr, ", skipped %d (up-to-date)", skippedCount)
		}
		fmt.Fprintln(os.Stderr)
	}

	if errorCount > 0 {
		return 1
	}
	return 0
}

// printTargetChanges prints each changed target's added and removed deps.
func printTargetChanges(changes []rulessync.TargetChange, addedLabel, removedLabel string) {
	for _, c := range changes {
		if len(c.Added) == 0 && len(c.Removed) == 0 {
			continue
		}
		if c.Attribute != "" {
			fmt.Fprintf(os.Stderr, "       :%s (%s)\n", c.Target, c.Attribute)
		} else {
			fmt.Fprintf(os.Stderr, "       :%s\n", c.Target)
		}
		if len(c.Added) > 0 {
			fmt.Fprintf(os.Stderr, "         %s: %v\n", addedLabel, c.Added)
		}
		if len(c.Removed) > 0 {
			fmt.Fprintf(os.Stderr, "         %s: %v\n", removedLabel, c.Removed)
		}
	}
}

// printKeptDeps reports, for each target of a rules.star file, the deps sync
// kept instead of removing because the target has unmapped imports.
func printKeptDeps(relPath string, changes []rulessync.TargetChange) {
	for _, c := range changes {
		if len(c.Kept) == 0 {
			continue
		}
		fmt.Fprintf(os.Stderr, "KEPT: %s:%s: not removing %v: sources have unmapped imports %v\n",
			relPath, c.Target, c.Kept, c.Unmapped)
	}
}

// printSkippedTargets reports the targets of a rules.star file that sync
// skipped: always those whose deps it can't read, and with -v those opted
// out with # turnkey:no-sync.
func printSkippedTargets(relPath string, result rulessync.SyncResult) {
	for _, u := range result.Unreadable {
		fmt.Fprintf(os.Stderr, "UNREADABLE: %s:%s: %s is not a list of labels; write it as one, or add # turnkey:no-sync before the rule\n",
			relPath, u.Target, u.Attribute)
	}
	if verbose {
		for _, target := range result.OptedOut {
			fmt.Fprintf(os.Stderr, "OPTED OUT: %s:%s\n", relPath, target)
		}
	}
}

// printRulesHelp prints help for the rules subcommand.
func printRulesHelp() {
	fmt.Fprintln(os.Stderr, `Usage: tk rules <command> [options] [path]

Commands:
  check              Check every rules.star file against its sources
  sync               Update rules.star files with detected dependencies
  help               Show this help

Options:
  --all, -a          sync: process all files (skip staleness detection)
  --force, -f        Same as --all
  --verbose, -v      Show detailed output including skipped files
  --quiet, -q        Suppress output
  --dry-run, -n      Show what would be changed without writing

Staleness Detection:
  check always checks every rules.star file, so it also catches a stale
  file that is already committed. sync, by default, only processes
  directories with uncommitted changes whose source files are newer than
  rules.star. Use --all or --force to sync all files.

Examples:
  tk rules check                    # Check all rules.star files
  tk rules check src/cmd/tk         # Check one directory
  tk rules sync                     # Update stale rules.star files
  tk rules sync --all               # Force update all files
  tk rules sync src/cmd/tk          # Sync specific directory

The rules command automatically detects imports from source files and
updates the deps list in rules.star. Manual dependencies can be preserved
using turnkey:preserve-start/end markers, and a "# turnkey:no-sync" comment
before a rule leaves that target's deps alone.`)
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
		Sync:        cfg,
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
	var unreadable []string
	for _, result := range results {
		relPath, _ := filepath.Rel(root, result.Path)
		for _, u := range result.Unreadable {
			unreadable = append(unreadable, relPath+":"+u.Target)
		}
		if !quiet {
			relPath, _ := filepath.Rel(root, result.Path)
			printKeptDeps(relPath, result.Changes)
		}
		if result.Updated {
			updatedCount++
		}
	}

	// Strict mode (CI): a target whose deps sync can't read, and nothing opts
	// out, fails like a stale rules.star. Otherwise the check before a build
	// stays quiet about it: tk rules sync and tk rules check report it.
	if (cfg.Rules.Strict || strictRules) && len(unreadable) > 0 {
		fmt.Fprintf(os.Stderr, "tk: %d target(s) have deps rules sync can't read (strict mode):\n", len(unreadable))
		for _, u := range unreadable {
			fmt.Fprintf(os.Stderr, "  - %s\n", u)
		}
		fmt.Fprintln(os.Stderr, "\ntk: write their deps as a list of labels, or add # turnkey:no-sync before the rule")
		return 1
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
