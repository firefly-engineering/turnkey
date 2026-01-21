package rules

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// SolidityImportDetector detects imports from Solidity source files.
type SolidityImportDetector struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string
}

// NewSolidityImportDetector creates a new Solidity import detector.
func NewSolidityImportDetector(projectRoot string) (*SolidityImportDetector, error) {
	d := &SolidityImportDetector{
		ProjectRoot: projectRoot,
	}

	return d, nil
}

// DetectImports detects all imports from Solidity source files in a directory.
func (d *SolidityImportDetector) DetectImports(dir string) ([]Import, error) {
	var imports []Import

	// Find all Solidity files (including in src/ subdirectory)
	patterns := []string{
		filepath.Join(dir, "*.sol"),
		filepath.Join(dir, "src", "*.sol"),
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

// solImportPattern matches Solidity import statements.
// Examples:
//   - import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
//   - import "forge-std/Test.sol";
//   - import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
//   - import * as Math from "./Math.sol";
var solImportPattern = regexp.MustCompile(`^\s*import\s+(?:\{[^}]*\}\s+from\s+)?(?:\*\s+as\s+\w+\s+from\s+)?["']([^"']+)["']\s*;`)

// detectFileImports detects imports from a single Solidity file.
func (d *SolidityImportDetector) detectFileImports(path string) ([]Import, error) {
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

		// Skip comments
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "//") || strings.HasPrefix(trimmed, "/*") {
			continue
		}

		// Check for import statements
		if matches := solImportPattern.FindStringSubmatch(line); len(matches) > 1 {
			importPath := matches[1]
			if !d.isRelativeImport(importPath) {
				imports = append(imports, Import{
					Path:       d.normalizeImportPath(importPath),
					SourceFile: relPath,
					Line:       lineNum,
					IsStdLib:   false, // Solidity has no stdlib in the traditional sense
				})
			}
		}
	}

	return imports, scanner.Err()
}

// isRelativeImport checks if an import is relative.
func (d *SolidityImportDetector) isRelativeImport(importPath string) bool {
	return strings.HasPrefix(importPath, "./") || strings.HasPrefix(importPath, "../")
}

// normalizeImportPath extracts the package name from an import path.
// Examples:
//   - @openzeppelin/contracts/token/ERC20/ERC20.sol -> @openzeppelin/contracts
//   - forge-std/Test.sol -> forge-std
func (d *SolidityImportDetector) normalizeImportPath(importPath string) string {
	// Handle scoped packages (@org/pkg)
	if strings.HasPrefix(importPath, "@") {
		parts := strings.SplitN(importPath, "/", 3)
		if len(parts) >= 2 {
			return parts[0] + "/" + parts[1]
		}
		return importPath
	}

	// For regular packages (forge-std/...), return just the first component
	parts := strings.SplitN(importPath, "/", 2)
	return parts[0]
}

// IsInternalImport checks if an import is internal to the project.
func (d *SolidityImportDetector) IsInternalImport(importPath string) bool {
	// Check if it's in the project's src directory
	return strings.HasPrefix(importPath, "src/")
}

// DetectTestImports detects imports from Solidity test files.
func (d *SolidityImportDetector) DetectTestImports(dir string) ([]Import, error) {
	var imports []Import

	// Find test files (Foundry convention: test/ directory, *.t.sol files)
	patterns := []string{
		filepath.Join(dir, "test", "*.sol"),
		filepath.Join(dir, "test", "**", "*.sol"),
		filepath.Join(dir, "*.t.sol"),
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
