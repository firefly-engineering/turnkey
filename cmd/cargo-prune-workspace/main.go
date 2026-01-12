// cargo-prune-workspace: Prune Cargo.toml workspace members to a whitelist
//
// This tool modifies a Cargo.toml file to keep only specified workspace members.
// Useful for creating minimal source trees for Nix builds.
package main

import (
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

func main() {
	var (
		membersFlag  string
		manifestPath string
		outputPath   string
	)

	flag.StringVar(&membersFlag, "members", "", "Workspace members to keep (comma-separated, required)")
	flag.StringVar(&manifestPath, "manifest-path", "Cargo.toml", "Path to Cargo.toml")
	flag.StringVar(&outputPath, "output", "", "Output path (defaults to in-place modification)")
	flag.Parse()

	if membersFlag == "" {
		fmt.Fprintln(os.Stderr, "error: --members is required")
		flag.Usage()
		os.Exit(1)
	}

	members := strings.Split(membersFlag, ",")
	whitelist := make(map[string]bool)
	for _, m := range members {
		whitelist[strings.TrimSpace(m)] = true
	}

	if err := run(manifestPath, outputPath, whitelist); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

func run(manifestPath, outputPath string, whitelist map[string]bool) error {
	// Read the Cargo.toml
	content, err := os.ReadFile(manifestPath)
	if err != nil {
		return fmt.Errorf("failed to read %s: %w", manifestPath, err)
	}

	// Parse as generic map to preserve structure
	var doc map[string]any
	if err := toml.Unmarshal(content, &doc); err != nil {
		return fmt.Errorf("failed to parse %s: %w", manifestPath, err)
	}

	// Get workspace section
	workspace, ok := doc["workspace"].(map[string]any)
	if !ok {
		return fmt.Errorf("no [workspace] section found")
	}

	// Get members array
	membersRaw, ok := workspace["members"].([]any)
	if !ok {
		return fmt.Errorf("workspace.members is not an array")
	}

	// Filter members
	var filtered []string
	for _, m := range membersRaw {
		member, ok := m.(string)
		if !ok {
			continue
		}
		if whitelist[member] {
			filtered = append(filtered, member)
			delete(whitelist, member)
		}
	}

	// Check for missing members
	if len(whitelist) > 0 {
		var missing []string
		for m := range whitelist {
			missing = append(missing, m)
		}
		return fmt.Errorf("requested members not found in workspace.members: %s", strings.Join(missing, ", "))
	}

	// Update members
	workspace["members"] = filtered

	// Marshal back to TOML
	out, err := toml.Marshal(doc)
	if err != nil {
		return fmt.Errorf("failed to marshal TOML: %w", err)
	}

	// Write output
	outPath := outputPath
	if outPath == "" {
		outPath = manifestPath
	}
	if err := os.WriteFile(outPath, out, 0644); err != nil {
		return fmt.Errorf("failed to write %s: %w", outPath, err)
	}

	return nil
}
