// go-deps-extract extracts Go package imports and outputs the extraction protocol JSON.
//
// Usage:
//
//	go-deps-extract [flags] [dir]
//
// By default, it analyzes the current directory. If a directory is provided,
// it analyzes that directory and all subdirectories containing Go packages.
//
// Flags:
//
//	-o string
//	    Output file path (default: stdout)
//	-module-prefix string
//	    Module prefix for internal import detection (auto-detected from go.mod if not set)
//	-exclude string
//	    Comma-separated list of directory patterns to exclude (e.g., "vendor,testdata")
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
		output       = flag.String("o", "", "Output file path (default: stdout)")
		modulePrefix = flag.String("module-prefix", "", "Module prefix for internal import detection")
		exclude      = flag.String("exclude", "vendor,testdata", "Comma-separated list of directory patterns to exclude")
	)
	flag.Parse()

	dir := "."
	if flag.NArg() > 0 {
		dir = flag.Arg(0)
	}

	// Auto-detect module prefix from go.mod if not provided
	if *modulePrefix == "" {
		prefix, err := detectModulePrefix(dir)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Warning: could not detect module prefix: %v\n", err)
		} else {
			*modulePrefix = prefix
		}
	}

	excludePatterns := strings.Split(*exclude, ",")
	for i := range excludePatterns {
		excludePatterns[i] = strings.TrimSpace(excludePatterns[i])
	}

	result, err := extract(dir, *modulePrefix, excludePatterns)
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

// detectModulePrefix reads go.mod to get the module path.
func detectModulePrefix(dir string) (string, error) {
	// Find go.mod by walking up from dir
	absDir, err := filepath.Abs(dir)
	if err != nil {
		return "", err
	}

	for {
		goMod := filepath.Join(absDir, "go.mod")
		if _, err := os.Stat(goMod); err == nil {
			data, err := os.ReadFile(goMod)
			if err != nil {
				return "", err
			}
			// Parse module line
			for _, line := range strings.Split(string(data), "\n") {
				line = strings.TrimSpace(line)
				if strings.HasPrefix(line, "module ") {
					return strings.TrimSpace(strings.TrimPrefix(line, "module ")), nil
				}
			}
			return "", fmt.Errorf("no module declaration found in go.mod")
		}

		parent := filepath.Dir(absDir)
		if parent == absDir {
			break
		}
		absDir = parent
	}

	return "", fmt.Errorf("go.mod not found")
}

// goListPackage represents the JSON output from `go list -json`.
type goListPackage struct {
	Dir         string   // Directory containing package sources
	ImportPath  string   // Import path of package
	Name        string   // Package name
	GoFiles     []string // .go source files (excluding CgoFiles, TestGoFiles, XTestGoFiles)
	TestGoFiles []string // _test.go files in package
	Imports     []string // Import paths used by this package
	TestImports []string // Imports used by tests
	Standard    bool     // Is this package part of the standard library?
}

// extract runs go list and converts the output to the extraction protocol.
func extract(dir string, modulePrefix string, excludePatterns []string) (*extraction.Result, error) {
	result := extraction.NewResult("go")

	// Get the absolute path of the directory for relative path calculation
	absDir, err := filepath.Abs(dir)
	if err != nil {
		return nil, fmt.Errorf("getting absolute path: %w", err)
	}

	// Find repo root (where go.mod is)
	repoRoot := findRepoRoot(absDir)
	if repoRoot == "" {
		repoRoot = absDir
	}

	// Run go list -json ./...
	cmd := exec.Command("go", "list", "-json", "./...")
	cmd.Dir = dir
	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			// go list may fail for some packages but still output valid JSON for others
			result.AddError(fmt.Sprintf("go list warning: %s", string(exitErr.Stderr)))
		} else {
			return nil, fmt.Errorf("running go list: %w", err)
		}
	}

	// Parse JSON output (stream of objects, not an array)
	dec := json.NewDecoder(strings.NewReader(string(output)))
	for dec.More() {
		var pkg goListPackage
		if err := dec.Decode(&pkg); err != nil {
			result.AddError(fmt.Sprintf("decoding go list output: %v", err))
			continue
		}

		// Skip excluded directories
		if shouldExclude(pkg.Dir, excludePatterns) {
			continue
		}

		// Calculate relative path from repo root
		relPath, err := filepath.Rel(repoRoot, pkg.Dir)
		if err != nil {
			relPath = pkg.Dir
		}

		// Collect files
		var files []string
		files = append(files, pkg.GoFiles...)

		// Process imports
		imports := classifyImports(pkg.Imports, modulePrefix)
		testImports := classifyImports(pkg.TestImports, modulePrefix)

		// Remove duplicates between imports and testImports
		importSet := make(map[string]bool)
		for _, imp := range imports {
			importSet[imp.Path] = true
		}
		var uniqueTestImports []extraction.Import
		for _, imp := range testImports {
			if !importSet[imp.Path] {
				uniqueTestImports = append(uniqueTestImports, imp)
			}
		}

		// Skip stdlib-only packages with no external/internal deps
		hasNonStdlib := false
		for _, imp := range imports {
			if imp.Kind != extraction.ImportKindStdlib {
				hasNonStdlib = true
				break
			}
		}
		for _, imp := range uniqueTestImports {
			if imp.Kind != extraction.ImportKindStdlib {
				hasNonStdlib = true
				break
			}
		}
		if !hasNonStdlib && len(imports) > 0 {
			// Package only imports stdlib, skip it for deps purposes
			// but still include it if it has no imports at all (might be a leaf package)
		}

		extractionPkg := extraction.Package{
			Path:        relPath,
			Files:       files,
			Imports:     imports,
			TestImports: uniqueTestImports,
		}

		result.AddPackage(extractionPkg)
	}

	// Sort packages by path for consistent output
	sort.Slice(result.Packages, func(i, j int) bool {
		return result.Packages[i].Path < result.Packages[j].Path
	})

	return result, nil
}

// findRepoRoot finds the directory containing go.mod.
func findRepoRoot(dir string) string {
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return ""
		}
		dir = parent
	}
}

// shouldExclude returns true if the directory matches any exclusion pattern.
func shouldExclude(dir string, patterns []string) bool {
	for _, pattern := range patterns {
		if pattern == "" {
			continue
		}
		// Simple substring match for now
		if strings.Contains(dir, pattern) {
			return true
		}
	}
	return false
}

// classifyImports classifies imports as stdlib, external, or internal.
func classifyImports(imports []string, modulePrefix string) []extraction.Import {
	var result []extraction.Import

	for _, imp := range imports {
		kind := classifyImport(imp, modulePrefix)
		result = append(result, extraction.Import{
			Path: imp,
			Kind: kind,
		})
	}

	// Sort by path for consistent output
	sort.Slice(result, func(i, j int) bool {
		return result[i].Path < result[j].Path
	})

	return result
}

// classifyImport determines if an import is stdlib, external, or internal.
func classifyImport(imp string, modulePrefix string) extraction.ImportKind {
	// Standard library packages don't contain a dot in the first path element
	// (e.g., "fmt", "net/http", "encoding/json")
	firstSlash := strings.Index(imp, "/")
	firstElement := imp
	if firstSlash > 0 {
		firstElement = imp[:firstSlash]
	}

	if !strings.Contains(firstElement, ".") {
		return extraction.ImportKindStdlib
	}

	// Check if it's an internal import
	if modulePrefix != "" && strings.HasPrefix(imp, modulePrefix) {
		return extraction.ImportKindInternal
	}

	return extraction.ImportKindExternal
}
