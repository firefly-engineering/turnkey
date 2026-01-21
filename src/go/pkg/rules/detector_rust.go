package rules

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// RustImportDetector detects imports from Rust source files.
type RustImportDetector struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// CrateName is the name of the current crate (from Cargo.toml).
	CrateName string

	// WorkspaceDeps are dependencies declared in the workspace Cargo.toml.
	WorkspaceDeps map[string]bool
}

// CargoToml represents relevant parts of a Cargo.toml file.
type CargoToml struct {
	Package struct {
		Name string `toml:"name"`
	} `toml:"package"`
	Dependencies map[string]interface{} `toml:"dependencies"`
	Workspace    struct {
		Dependencies map[string]interface{} `toml:"dependencies"`
	} `toml:"workspace"`
}

// NewRustImportDetector creates a new Rust import detector.
func NewRustImportDetector(projectRoot string) (*RustImportDetector, error) {
	d := &RustImportDetector{
		ProjectRoot:   projectRoot,
		WorkspaceDeps: make(map[string]bool),
	}

	// Try to read workspace dependencies from root Cargo.toml
	cargoPath := filepath.Join(projectRoot, "Cargo.toml")
	if content, err := os.ReadFile(cargoPath); err == nil {
		var cargo CargoToml
		if err := toml.Unmarshal(content, &cargo); err == nil {
			for dep := range cargo.Workspace.Dependencies {
				d.WorkspaceDeps[dep] = true
			}
		}
	}

	return d, nil
}

// DetectImports detects all imports from Rust source files in a directory.
// For Rust, we primarily look at Cargo.toml dependencies since those are what
// matter for Buck2 builds.
func (d *RustImportDetector) DetectImports(dir string) ([]Import, error) {
	var imports []Import

	// First, check for local Cargo.toml to get declared dependencies
	cargoPath := filepath.Join(dir, "Cargo.toml")
	if content, err := os.ReadFile(cargoPath); err == nil {
		var cargo CargoToml
		if err := toml.Unmarshal(content, &cargo); err == nil {
			d.CrateName = cargo.Package.Name
			for dep := range cargo.Dependencies {
				imports = append(imports, Import{
					Path:       dep,
					SourceFile: "Cargo.toml",
					Line:       0,
					IsStdLib:   false,
				})
			}
		}
	}

	// Also scan .rs files for use statements to catch any additional crates
	files, err := filepath.Glob(filepath.Join(dir, "*.rs"))
	if err != nil {
		return imports, nil
	}

	for _, file := range files {
		// Skip test files for main library deps
		if strings.HasSuffix(file, "_test.rs") {
			continue
		}

		fileImports, err := d.detectFileImports(file)
		if err != nil {
			continue
		}

		imports = append(imports, fileImports...)
	}

	return deduplicateImports(imports), nil
}

// usePattern matches Rust use statements.
// Examples:
//   - use itoa::Buffer;
//   - use serde::{Deserialize, Serialize};
//   - use std::collections::HashMap;
var usePattern = regexp.MustCompile(`^\s*use\s+([a-zA-Z_][a-zA-Z0-9_]*)(::|\s*;)`)

// externCratePattern matches extern crate statements (older Rust style).
var externCratePattern = regexp.MustCompile(`^\s*extern\s+crate\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*;`)

// detectFileImports detects imports from a single Rust file.
func (d *RustImportDetector) detectFileImports(path string) ([]Import, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var imports []Import
	scanner := bufio.NewScanner(file)
	lineNum := 0

	relPath, _ := filepath.Rel(d.ProjectRoot, path)

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()

		// Check for use statements
		if matches := usePattern.FindStringSubmatch(line); len(matches) > 1 {
			crateName := matches[1]
			if !d.isStdLib(crateName) {
				imports = append(imports, Import{
					Path:       crateName,
					SourceFile: relPath,
					Line:       lineNum,
					IsStdLib:   false,
				})
			}
		}

		// Check for extern crate statements
		if matches := externCratePattern.FindStringSubmatch(line); len(matches) > 1 {
			crateName := matches[1]
			if !d.isStdLib(crateName) {
				imports = append(imports, Import{
					Path:       crateName,
					SourceFile: relPath,
					Line:       lineNum,
					IsStdLib:   false,
				})
			}
		}
	}

	return imports, scanner.Err()
}

// isStdLib checks if a crate is part of the Rust standard library.
func (d *RustImportDetector) isStdLib(crateName string) bool {
	stdLibCrates := map[string]bool{
		"std":        true,
		"core":       true,
		"alloc":      true,
		"collections": true,
		"test":       true,
		"proc_macro": true,
		// Also self/crate/super references
		"self":  true,
		"crate": true,
		"super": true,
	}
	return stdLibCrates[crateName]
}

// IsInternalImport checks if a crate is internal to the monorepo.
func (d *RustImportDetector) IsInternalImport(crateName string) bool {
	// For now, we consider a crate internal if it's in the monorepo's Cargo.toml workspace
	// but not in rust-deps.toml. This is a heuristic that may need refinement.
	return false
}

// DetectTestImports detects imports from Rust test files.
func (d *RustImportDetector) DetectTestImports(dir string) ([]Import, error) {
	var imports []Import

	// Find test files
	patterns := []string{
		filepath.Join(dir, "*_test.rs"),
		filepath.Join(dir, "tests", "*.rs"),
	}

	for _, pattern := range patterns {
		files, err := filepath.Glob(pattern)
		if err != nil {
			continue
		}

		for _, file := range files {
			fileImports, err := d.detectFileImports(file)
			if err != nil {
				continue
			}
			imports = append(imports, fileImports...)
		}
	}

	return deduplicateImports(imports), nil
}
