// rust-deps-extract extracts Rust crate dependencies and outputs the extraction protocol JSON.
//
// Usage:
//
//	rust-deps-extract [flags] [dir]
//
// By default, it analyzes the current directory. If a directory is provided,
// it analyzes the Cargo workspace/package in that directory.
//
// Flags:
//
//	-o string
//	    Output file path (default: stdout)
//	-exclude string
//	    Comma-separated list of directory patterns to exclude (e.g., "target,tests")
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

func main() {
	var (
		output  = flag.String("o", "", "Output file path (default: stdout)")
		exclude = flag.String("exclude", "target", "Comma-separated list of directory patterns to exclude")
	)
	flag.Parse()

	dir := "."
	if flag.NArg() > 0 {
		dir = flag.Arg(0)
	}

	excludePatterns := strings.Split(*exclude, ",")
	for i := range excludePatterns {
		excludePatterns[i] = strings.TrimSpace(excludePatterns[i])
	}

	result, err := extract(dir, excludePatterns)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}

	// Output
	var w = os.Stdout
	if *output != "" {
		f, err := os.Create(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error creating output file: %v\n", err)
			os.Exit(1)
		}
		defer f.Close()
		w = f
	}

	if err := result.Write(w); err != nil {
		fmt.Fprintf(os.Stderr, "Error writing output: %v\n", err)
		os.Exit(1)
	}
}

// cargoMetadata represents the output of `cargo metadata --format-version 1`.
type cargoMetadata struct {
	Packages         []cargoPackage `json:"packages"`
	WorkspaceMembers []string       `json:"workspace_members"`
	WorkspaceRoot    string         `json:"workspace_root"`
}

// cargoPackage represents a package in cargo metadata output.
type cargoPackage struct {
	Name         string            `json:"name"`
	Version      string            `json:"version"`
	ID           string            `json:"id"`
	Source       *string           `json:"source"` // null for path deps, "registry+..." for crates.io
	ManifestPath string            `json:"manifest_path"`
	Dependencies []cargoDependency `json:"dependencies"`
	Targets      []cargoTarget     `json:"targets"`
}

// cargoDependency represents a dependency in cargo metadata output.
type cargoDependency struct {
	Name     string  `json:"name"`
	Source   *string `json:"source"` // null for path deps
	Req      string  `json:"req"`    // version requirement
	Kind     *string `json:"kind"`   // null for normal, "dev", "build"
	Rename   *string `json:"rename"` // alias if renamed
	Optional bool    `json:"optional"`
	Path     *string `json:"path"` // for path dependencies
}

// cargoTarget represents a build target in cargo metadata output.
type cargoTarget struct {
	Kind    []string `json:"kind"` // "lib", "bin", "test", "bench", etc.
	Name    string   `json:"name"`
	SrcPath string   `json:"src_path"`
}

// extract runs cargo metadata and converts the output to the extraction protocol.
func extract(dir string, excludePatterns []string) (*extraction.Result, error) {
	result := extraction.NewResult("rust")

	// Get absolute path
	absDir, err := filepath.Abs(dir)
	if err != nil {
		return nil, fmt.Errorf("getting absolute path: %w", err)
	}

	// Run cargo metadata
	cmd := exec.Command("cargo", "metadata", "--format-version", "1", "--no-deps")
	cmd.Dir = absDir

	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("cargo metadata failed: %s", string(exitErr.Stderr))
		}
		return nil, fmt.Errorf("running cargo metadata: %w", err)
	}

	var metadata cargoMetadata
	if err := json.Unmarshal(output, &metadata); err != nil {
		return nil, fmt.Errorf("parsing cargo metadata: %w", err)
	}

	// Build set of workspace member IDs for quick lookup
	workspaceMembers := make(map[string]bool)
	for _, member := range metadata.WorkspaceMembers {
		workspaceMembers[member] = true
	}

	// Build set of workspace package names (for internal dep detection)
	workspacePackages := make(map[string]string) // name -> relative path
	for _, pkg := range metadata.Packages {
		if workspaceMembers[pkg.ID] {
			// Calculate relative path from workspace root
			pkgDir := filepath.Dir(pkg.ManifestPath)
			relPath, err := filepath.Rel(metadata.WorkspaceRoot, pkgDir)
			if err != nil {
				relPath = pkgDir
			}
			workspacePackages[pkg.Name] = relPath
		}
	}

	// Process each workspace member
	for _, pkg := range metadata.Packages {
		if !workspaceMembers[pkg.ID] {
			continue
		}

		// Calculate relative path from workspace root
		pkgDir := filepath.Dir(pkg.ManifestPath)
		relPath, err := filepath.Rel(metadata.WorkspaceRoot, pkgDir)
		if err != nil {
			relPath = pkgDir
		}

		// Skip excluded directories
		if shouldExclude(relPath, excludePatterns) {
			continue
		}

		// Collect source files from targets
		var files []string
		for _, target := range pkg.Targets {
			srcPath, err := filepath.Rel(metadata.WorkspaceRoot, target.SrcPath)
			if err != nil {
				srcPath = target.SrcPath
			}
			files = append(files, srcPath)
		}

		// Process dependencies
		var imports []extraction.Import
		var testImports []extraction.Import

		for _, dep := range pkg.Dependencies {
			imp := classifyDependency(dep, workspacePackages)

			// Skip optional deps by default
			if dep.Optional {
				continue
			}

			// Determine if this is a dev/test dependency
			isDevDep := dep.Kind != nil && *dep.Kind == "dev"

			if isDevDep {
				testImports = append(testImports, imp)
			} else {
				imports = append(imports, imp)
			}
		}

		// Sort imports for consistent output
		sortImports(imports)
		sortImports(testImports)

		extractionPkg := extraction.Package{
			Path:        relPath,
			Files:       files,
			Imports:     imports,
			TestImports: testImports,
		}

		result.AddPackage(extractionPkg)
	}

	// Sort packages by path for consistent output
	sort.Slice(result.Packages, func(i, j int) bool {
		return result.Packages[i].Path < result.Packages[j].Path
	})

	return result, nil
}

// classifyDependency determines if a dependency is internal, external, or stdlib.
func classifyDependency(dep cargoDependency, workspacePackages map[string]string) extraction.Import {
	name := dep.Name
	if dep.Rename != nil {
		name = *dep.Rename
	}

	// Check if this is a workspace (internal) dependency
	if dep.Source == nil {
		// Path dependency - check if it's in our workspace
		if _, isWorkspace := workspacePackages[dep.Name]; isWorkspace {
			return extraction.Import{
				Path:  dep.Name,
				Kind:  extraction.ImportKindInternal,
				Alias: aliasIfDifferent(name, dep.Name),
			}
		}
		// Path dep outside workspace - still treat as internal
		return extraction.Import{
			Path:  dep.Name,
			Kind:  extraction.ImportKindInternal,
			Alias: aliasIfDifferent(name, dep.Name),
		}
	}

	// External dependency (from crates.io or other registry)
	return extraction.Import{
		Path:  dep.Name,
		Kind:  extraction.ImportKindExternal,
		Alias: aliasIfDifferent(name, dep.Name),
	}
}

// aliasIfDifferent returns the alias if it differs from the original name.
func aliasIfDifferent(alias, original string) string {
	if alias != original {
		return alias
	}
	return ""
}

// shouldExclude returns true if the path matches any exclusion pattern.
func shouldExclude(path string, patterns []string) bool {
	for _, pattern := range patterns {
		if pattern == "" {
			continue
		}
		if strings.Contains(path, pattern) {
			return true
		}
	}
	return false
}

// sortImports sorts imports by path.
func sortImports(imports []extraction.Import) {
	sort.Slice(imports, func(i, j int) bool {
		return imports[i].Path < imports[j].Path
	})
}
