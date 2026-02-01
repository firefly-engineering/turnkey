package main

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// Directories for edit overlay system
const (
	editsDir   = ".turnkey/edits"
	patchesDir = ".turnkey/patches"
)

// runCompose handles the "tk compose" subcommand for edit workflow.
// Usage:
//
//	tk compose status              # Show edited files
//	tk compose edit <cell/path>    # Copy file from cell to edits for modification
//	tk compose patch               # Generate patches from edited files
//	tk compose patch <cell>        # Generate patches for specific cell
//	tk compose reset               # Revert all edits
//	tk compose reset <cell>        # Revert edits for specific cell
//	tk compose reset <cell/path>   # Revert specific file edit
func runCompose(args []string) int {
	if len(args) == 0 {
		printComposeHelp()
		return 0
	}

	subcmd := args[0]
	subargs := args[1:]

	switch subcmd {
	case "status":
		return runComposeStatus(subargs)
	case "edit":
		return runComposeEdit(subargs)
	case "patch":
		return runComposePatch(subargs)
	case "reset":
		return runComposeReset(subargs)
	case "help", "--help", "-h":
		printComposeHelp()
		return 0
	default:
		fmt.Fprintf(os.Stderr, "tk compose: unknown subcommand %q\n", subcmd)
		printComposeHelp()
		return 1
	}
}

// runComposeStatus shows the status of edited files.
func runComposeStatus(args []string) int {
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk compose: %v\n", err)
		return 1
	}

	// Parse flags
	var showPatches bool
	for _, arg := range args {
		switch arg {
		case "--patches", "-p":
			showPatches = true
		case "--verbose", "-v":
			verbose = true
		}
	}

	editsPath := filepath.Join(root, editsDir)
	patchesPath := filepath.Join(root, patchesDir)

	// List edited files
	edited := listEditedFiles(editsPath)
	if len(edited) == 0 {
		fmt.Println("No edited files.")
	} else {
		fmt.Printf("Edited files (%d):\n", len(edited))
		for cell, files := range edited {
			fmt.Printf("\n  %s:\n", cell)
			for _, f := range files {
				fmt.Printf("    %s\n", f)
			}
		}
	}

	// List patches if requested
	if showPatches {
		patches := listPatchFiles(patchesPath)
		if len(patches) == 0 {
			fmt.Println("\nNo patches generated.")
		} else {
			fmt.Printf("\nGenerated patches (%d):\n", len(patches))
			for cell, files := range patches {
				fmt.Printf("\n  %s:\n", cell)
				for _, f := range files {
					fmt.Printf("    %s\n", f)
				}
			}
		}
	}

	return 0
}

// runComposeEdit copies a file from a cell to the edits directory for modification.
func runComposeEdit(args []string) int {
	if len(args) == 0 {
		fmt.Fprintf(os.Stderr, "tk compose edit: missing argument\n")
		fmt.Fprintf(os.Stderr, "Usage: tk compose edit <cell>/<path>\n")
		fmt.Fprintf(os.Stderr, "Example: tk compose edit godeps/vendor/github.com/spf13/cobra/command.go\n")
		return 1
	}

	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk compose: %v\n", err)
		return 1
	}

	target := args[0]

	// Parse cell/path
	parts := strings.SplitN(target, "/", 2)
	if len(parts) < 2 {
		fmt.Fprintf(os.Stderr, "tk compose edit: invalid path format, expected <cell>/<path>\n")
		return 1
	}
	cell := parts[0]
	relPath := parts[1]

	// Find the cell source (symlink in .turnkey/)
	cellLink := filepath.Join(root, ".turnkey", cell)
	cellSource, err := os.Readlink(cellLink)
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk compose edit: cell %q not found: %v\n", cell, err)
		return 1
	}

	// Source file in the Nix store
	sourceFile := filepath.Join(cellSource, relPath)
	if _, err := os.Stat(sourceFile); os.IsNotExist(err) {
		fmt.Fprintf(os.Stderr, "tk compose edit: file not found: %s\n", sourceFile)
		return 1
	}

	// Destination in edits directory
	editFile := filepath.Join(root, editsDir, cell, relPath)

	// Check if already edited
	if _, err := os.Stat(editFile); err == nil {
		fmt.Fprintf(os.Stderr, "tk compose edit: file already being edited: %s\n", editFile)
		fmt.Fprintf(os.Stderr, "Use 'tk compose reset %s' to revert first.\n", target)
		return 1
	}

	// Create parent directory
	if err := os.MkdirAll(filepath.Dir(editFile), 0755); err != nil {
		fmt.Fprintf(os.Stderr, "tk compose edit: failed to create directory: %v\n", err)
		return 1
	}

	// Copy the file
	if err := copyFile(sourceFile, editFile); err != nil {
		fmt.Fprintf(os.Stderr, "tk compose edit: failed to copy file: %v\n", err)
		return 1
	}

	fmt.Printf("Created editable copy: %s\n", editFile)
	fmt.Printf("Original: %s\n", sourceFile)
	fmt.Println("\nEdit the file, then run 'tk compose patch' to generate a patch.")
	return 0
}

// runComposePatch generates patches from edited files.
func runComposePatch(args []string) int {
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk compose: %v\n", err)
		return 1
	}

	// Parse flags and optional cell filter
	var cellFilter string
	for _, arg := range args {
		switch arg {
		case "--verbose", "-v":
			verbose = true
		default:
			if arg != "" && arg[0] != '-' {
				cellFilter = arg
			}
		}
	}

	editsPath := filepath.Join(root, editsDir)
	patchesPath := filepath.Join(root, patchesDir)

	edited := listEditedFiles(editsPath)
	if len(edited) == 0 {
		fmt.Println("No edited files to generate patches from.")
		return 0
	}

	patchCount := 0
	for cell, files := range edited {
		// Skip if cell filter specified and doesn't match
		if cellFilter != "" && cell != cellFilter {
			continue
		}

		// Get cell source path
		cellLink := filepath.Join(root, ".turnkey", cell)
		cellSource, err := os.Readlink(cellLink)
		if err != nil {
			fmt.Fprintf(os.Stderr, "tk compose patch: cell %q not found: %v\n", cell, err)
			continue
		}

		for _, relPath := range files {
			originalFile := filepath.Join(cellSource, relPath)
			editedFile := filepath.Join(editsPath, cell, relPath)

			// Generate patch filename (replace / with -)
			patchName := strings.ReplaceAll(relPath, "/", "-") + ".patch"
			patchFile := filepath.Join(patchesPath, cell, patchName)

			// Generate diff
			diff, err := generateUnifiedDiff(originalFile, editedFile, "a/"+relPath, "b/"+relPath)
			if err != nil {
				fmt.Fprintf(os.Stderr, "tk compose patch: failed to diff %s: %v\n", relPath, err)
				continue
			}

			if diff == "" {
				if verbose {
					fmt.Printf("No changes: %s/%s\n", cell, relPath)
				}
				continue
			}

			// Write patch file
			if err := os.MkdirAll(filepath.Dir(patchFile), 0755); err != nil {
				fmt.Fprintf(os.Stderr, "tk compose patch: failed to create directory: %v\n", err)
				continue
			}

			if err := os.WriteFile(patchFile, []byte(diff), 0644); err != nil {
				fmt.Fprintf(os.Stderr, "tk compose patch: failed to write patch: %v\n", err)
				continue
			}

			fmt.Printf("Generated: %s\n", patchFile)
			patchCount++
		}
	}

	if patchCount == 0 {
		fmt.Println("No patches generated (no changes detected).")
	} else {
		fmt.Printf("\nGenerated %d patch(es).\n", patchCount)
	}

	return 0
}

// runComposeReset reverts edited files.
func runComposeReset(args []string) int {
	root, err := findProjectRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "tk compose: %v\n", err)
		return 1
	}

	// Parse flags and optional target
	var target string
	var force bool
	for _, arg := range args {
		switch arg {
		case "--force", "-f":
			force = true
		case "--verbose", "-v":
			verbose = true
		default:
			if arg != "" && arg[0] != '-' {
				target = arg
			}
		}
	}

	editsPath := filepath.Join(root, editsDir)

	if target == "" {
		// Reset all edits
		edited := listEditedFiles(editsPath)
		if len(edited) == 0 {
			fmt.Println("No edited files to reset.")
			return 0
		}

		if !force {
			fmt.Printf("This will remove all %d edited file(s). Continue? [y/N] ", countFiles(edited))
			reader := bufio.NewReader(os.Stdin)
			response, _ := reader.ReadString('\n')
			response = strings.TrimSpace(strings.ToLower(response))
			if response != "y" && response != "yes" {
				fmt.Println("Aborted.")
				return 0
			}
		}

		// Remove all edits
		if err := os.RemoveAll(editsPath); err != nil {
			fmt.Fprintf(os.Stderr, "tk compose reset: failed to remove edits: %v\n", err)
			return 1
		}

		fmt.Println("All edits have been reverted.")
		return 0
	}

	// Check if target is a cell or a specific file
	parts := strings.SplitN(target, "/", 2)
	cell := parts[0]

	if len(parts) == 1 {
		// Reset entire cell
		cellEditsPath := filepath.Join(editsPath, cell)
		if _, err := os.Stat(cellEditsPath); os.IsNotExist(err) {
			fmt.Printf("No edits for cell %q.\n", cell)
			return 0
		}

		if !force {
			fmt.Printf("This will remove all edits for cell %q. Continue? [y/N] ", cell)
			reader := bufio.NewReader(os.Stdin)
			response, _ := reader.ReadString('\n')
			response = strings.TrimSpace(strings.ToLower(response))
			if response != "y" && response != "yes" {
				fmt.Println("Aborted.")
				return 0
			}
		}

		if err := os.RemoveAll(cellEditsPath); err != nil {
			fmt.Fprintf(os.Stderr, "tk compose reset: failed to remove edits: %v\n", err)
			return 1
		}

		fmt.Printf("Reverted all edits for cell %q.\n", cell)
		return 0
	}

	// Reset specific file
	relPath := parts[1]
	editFile := filepath.Join(editsPath, cell, relPath)

	if _, err := os.Stat(editFile); os.IsNotExist(err) {
		fmt.Printf("File not being edited: %s\n", target)
		return 0
	}

	if err := os.Remove(editFile); err != nil {
		fmt.Fprintf(os.Stderr, "tk compose reset: failed to remove file: %v\n", err)
		return 1
	}

	// Clean up empty directories
	cleanEmptyDirs(filepath.Dir(editFile), editsPath)

	fmt.Printf("Reverted: %s\n", target)
	return 0
}

// listEditedFiles returns a map of cell -> list of relative paths.
func listEditedFiles(editsPath string) map[string][]string {
	result := make(map[string][]string)

	if _, err := os.Stat(editsPath); os.IsNotExist(err) {
		return result
	}

	entries, err := os.ReadDir(editsPath)
	if err != nil {
		return result
	}

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		cell := entry.Name()
		cellPath := filepath.Join(editsPath, cell)

		var files []string
		filepath.Walk(cellPath, func(path string, info os.FileInfo, err error) error {
			if err != nil || info.IsDir() {
				return nil
			}
			relPath, _ := filepath.Rel(cellPath, path)
			files = append(files, relPath)
			return nil
		})

		if len(files) > 0 {
			result[cell] = files
		}
	}

	return result
}

// listPatchFiles returns a map of cell -> list of patch filenames.
func listPatchFiles(patchesPath string) map[string][]string {
	result := make(map[string][]string)

	if _, err := os.Stat(patchesPath); os.IsNotExist(err) {
		return result
	}

	entries, err := os.ReadDir(patchesPath)
	if err != nil {
		return result
	}

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		cell := entry.Name()
		cellPath := filepath.Join(patchesPath, cell)

		var files []string
		patchEntries, err := os.ReadDir(cellPath)
		if err != nil {
			continue
		}
		for _, pe := range patchEntries {
			if !pe.IsDir() && strings.HasSuffix(pe.Name(), ".patch") {
				files = append(files, pe.Name())
			}
		}

		if len(files) > 0 {
			result[cell] = files
		}
	}

	return result
}

// countFiles counts total files across all cells.
func countFiles(m map[string][]string) int {
	count := 0
	for _, files := range m {
		count += len(files)
	}
	return count
}

// copyFile copies a file from src to dst.
func copyFile(src, dst string) error {
	data, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	return os.WriteFile(dst, data, 0644)
}

// cleanEmptyDirs removes empty directories up to the stopAt path.
func cleanEmptyDirs(dir, stopAt string) {
	for dir != stopAt && dir != "." && dir != "/" {
		entries, err := os.ReadDir(dir)
		if err != nil || len(entries) > 0 {
			break
		}
		os.Remove(dir)
		dir = filepath.Dir(dir)
	}
}

// generateUnifiedDiff generates a unified diff between two files.
func generateUnifiedDiff(originalPath, modifiedPath, originalLabel, modifiedLabel string) (string, error) {
	original, err := readLines(originalPath)
	if err != nil {
		return "", err
	}

	modified, err := readLines(modifiedPath)
	if err != nil {
		return "", err
	}

	return unifiedDiff(original, modified, originalLabel, modifiedLabel), nil
}

// readLines reads a file and returns its lines.
func readLines(path string) ([]string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	return strings.Split(string(data), "\n"), nil
}

// unifiedDiff generates a unified diff between two sets of lines.
func unifiedDiff(original, modified []string, originalLabel, modifiedLabel string) string {
	// Find longest common subsequence
	lcs := longestCommonSubsequence(original, modified)

	// Build hunks
	hunks := buildDiffHunks(original, modified, lcs)

	if len(hunks) == 0 {
		return ""
	}

	// Format output
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("--- %s\n", originalLabel))
	sb.WriteString(fmt.Sprintf("+++ %s\n", modifiedLabel))

	for _, hunk := range hunks {
		sb.WriteString(hunk.String())
	}

	return sb.String()
}

// diffHunk represents a hunk in a unified diff.
type diffHunk struct {
	origStart int
	origCount int
	modStart  int
	modCount  int
	lines     []diffLine
}

func (h *diffHunk) String() string {
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("@@ -%d,%d +%d,%d @@\n", h.origStart, h.origCount, h.modStart, h.modCount))
	for _, line := range h.lines {
		sb.WriteString(line.String())
		sb.WriteString("\n")
	}
	return sb.String()
}

// diffLine represents a single line in a diff.
type diffLine struct {
	kind byte // ' ', '+', '-'
	text string
}

func (l *diffLine) String() string {
	return string(l.kind) + l.text
}

// longestCommonSubsequence finds the LCS of two string slices.
func longestCommonSubsequence(a, b []string) []struct{ ai, bi int } {
	m, n := len(a), len(b)
	if m == 0 || n == 0 {
		return nil
	}

	// Build DP table
	dp := make([][]int, m+1)
	for i := range dp {
		dp[i] = make([]int, n+1)
	}

	for i := 1; i <= m; i++ {
		for j := 1; j <= n; j++ {
			if a[i-1] == b[j-1] {
				dp[i][j] = dp[i-1][j-1] + 1
			} else {
				dp[i][j] = max(dp[i-1][j], dp[i][j-1])
			}
		}
	}

	// Backtrack
	var result []struct{ ai, bi int }
	i, j := m, n
	for i > 0 && j > 0 {
		if a[i-1] == b[j-1] {
			result = append(result, struct{ ai, bi int }{i - 1, j - 1})
			i--
			j--
		} else if dp[i-1][j] > dp[i][j-1] {
			i--
		} else {
			j--
		}
	}

	// Reverse
	for i, j := 0, len(result)-1; i < j; i, j = i+1, j-1 {
		result[i], result[j] = result[j], result[i]
	}

	return result
}

// buildDiffHunks builds diff hunks from original, modified, and LCS.
func buildDiffHunks(original, modified []string, lcs []struct{ ai, bi int }) []*diffHunk {
	const contextLines = 3

	var hunks []*diffHunk
	var currentHunk *diffHunk

	// Build sets for quick lookup
	lcsOrig := make(map[int]bool)
	lcsMod := make(map[int]bool)
	for _, p := range lcs {
		lcsOrig[p.ai] = true
		lcsMod[p.bi] = true
	}

	origIdx, modIdx, lcsIdx := 0, 0, 0

	for origIdx < len(original) || modIdx < len(modified) {
		// Check if we're at a common line
		inLCS := lcsIdx < len(lcs) &&
			origIdx == lcs[lcsIdx].ai &&
			modIdx == lcs[lcsIdx].bi

		if inLCS {
			// Common line (context)
			if currentHunk != nil {
				currentHunk.lines = append(currentHunk.lines, diffLine{' ', original[origIdx]})
				currentHunk.origCount++
				currentHunk.modCount++
			}
			origIdx++
			modIdx++
			lcsIdx++
		} else {
			// Difference found - start or extend hunk
			if currentHunk == nil {
				contextStart := max(0, origIdx-contextLines)
				modContextStart := max(0, modIdx-contextLines)

				currentHunk = &diffHunk{
					origStart: contextStart + 1,
					modStart:  modContextStart + 1,
				}

				// Add leading context
				for i := contextStart; i < origIdx; i++ {
					currentHunk.lines = append(currentHunk.lines, diffLine{' ', original[i]})
					currentHunk.origCount++
					currentHunk.modCount++
				}
			}

			// Add removed lines
			for origIdx < len(original) && !lcsOrig[origIdx] {
				currentHunk.lines = append(currentHunk.lines, diffLine{'-', original[origIdx]})
				currentHunk.origCount++
				origIdx++
			}

			// Add added lines
			for modIdx < len(modified) && !lcsMod[modIdx] {
				currentHunk.lines = append(currentHunk.lines, diffLine{'+', modified[modIdx]})
				currentHunk.modCount++
				modIdx++
			}
		}

		// Check if we should finalize the hunk
		if currentHunk != nil && lcsIdx >= len(lcs) {
			// Add trailing context and finalize
			hasChanges := false
			for _, line := range currentHunk.lines {
				if line.kind == '+' || line.kind == '-' {
					hasChanges = true
					break
				}
			}
			if hasChanges {
				hunks = append(hunks, currentHunk)
			}
			currentHunk = nil
		}
	}

	return hunks
}

func printComposeHelp() {
	fmt.Fprintln(os.Stderr, `Usage: tk compose <command> [options]

Manage edits to external dependencies. Edited files are stored in .turnkey/edits/
and can be converted to patches for integration with Nix fixups.

Commands:
  status              Show edited files and generated patches
  edit <cell/path>    Copy a file from a cell to edits for modification
  patch [cell]        Generate patches from edited files
  reset [cell/path]   Revert edits (all, by cell, or specific file)

Examples:
  # Copy a file for editing
  tk compose edit godeps/vendor/github.com/spf13/cobra/command.go

  # Show what's being edited
  tk compose status

  # Generate patches from all edits
  tk compose patch

  # Generate patches for a specific cell
  tk compose patch godeps

  # Revert all edits
  tk compose reset

  # Revert edits for a specific cell
  tk compose reset godeps

  # Revert a specific file
  tk compose reset godeps/vendor/github.com/spf13/cobra/command.go

Options:
  --verbose, -v    Show detailed output
  --force, -f      Skip confirmation prompts (for reset)
  --patches, -p    Show patches in status output`)
}
