// Package extraction defines the protocol for language-specific dependency extractors.
//
// Extractors are standalone tools that analyze source files in a directory and output
// a JSON document describing the imports found. This package defines the shared types
// for that protocol.
//
// # Protocol Version
//
// The current protocol version is "1". The version field allows for future evolution
// of the protocol while maintaining backwards compatibility.
//
// # Example Output
//
//	{
//	  "version": "1",
//	  "language": "go",
//	  "packages": [
//	    {
//	      "path": "src/cmd/tk",
//	      "files": ["main.go", "sync.go"],
//	      "imports": [
//	        {"path": "github.com/foo/bar", "kind": "external"},
//	        {"path": "github.com/firefly-engineering/turnkey/src/go/pkg/rules", "kind": "internal"}
//	      ]
//	    }
//	  ]
//	}
package extraction

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
)

// ProtocolVersion is the current protocol version.
const ProtocolVersion = "1"

// Result is the output of a dependency extractor.
type Result struct {
	// Version is the protocol version. Currently "1".
	Version string `json:"version"`

	// Language identifies the source language (e.g., "go", "rust", "python").
	Language string `json:"language"`

	// Packages contains the extracted package information.
	Packages []Package `json:"packages"`

	// Errors contains any non-fatal errors encountered during extraction.
	// Fatal errors should cause the extractor to exit with a non-zero status.
	Errors []string `json:"errors,omitempty"`
}

// Package represents a single package/module and its imports.
type Package struct {
	// Path is the relative path to the package directory from the repository root.
	Path string `json:"path"`

	// Files lists the source files that were analyzed.
	Files []string `json:"files"`

	// Imports contains the imports extracted from the source files.
	Imports []Import `json:"imports"`

	// TestImports contains imports that are only used in test files.
	// These should be added to test target deps, not library deps.
	TestImports []Import `json:"test_imports,omitempty"`

	// BuildTags lists any build tags/constraints found in the package.
	// This helps the mapper understand conditional compilation.
	BuildTags []string `json:"build_tags,omitempty"`
}

// ImportKind classifies an import as internal or external.
type ImportKind string

const (
	// ImportKindInternal is for imports within the same repository.
	ImportKindInternal ImportKind = "internal"

	// ImportKindExternal is for imports from external dependencies.
	ImportKindExternal ImportKind = "external"

	// ImportKindStdlib is for imports from the language's standard library.
	ImportKindStdlib ImportKind = "stdlib"
)

// Import represents a single import statement.
type Import struct {
	// Path is the import path as written in the source code.
	// For Go: "github.com/foo/bar"
	// For Rust: "serde" or "crate::module"
	// For Python: "requests" or "mypackage.submodule"
	Path string `json:"path"`

	// Kind classifies the import as internal, external, or stdlib.
	Kind ImportKind `json:"kind"`

	// Alias is the import alias if one was used (e.g., import alias "package").
	// Empty string if no alias was used.
	Alias string `json:"alias,omitempty"`

	// Files lists which source files contain this import.
	// Useful for debugging and understanding import usage.
	Files []string `json:"files,omitempty"`
}

// NewResult creates a new Result with the current protocol version.
func NewResult(language string) *Result {
	return &Result{
		Version:  ProtocolVersion,
		Language: language,
		Packages: []Package{},
	}
}

// AddPackage adds a package to the result.
func (r *Result) AddPackage(pkg Package) {
	r.Packages = append(r.Packages, pkg)
}

// AddError adds a non-fatal error to the result.
func (r *Result) AddError(err string) {
	r.Errors = append(r.Errors, err)
}

// Write serializes the result to JSON and writes it to the writer.
func (r *Result) Write(w io.Writer) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	return enc.Encode(r)
}

// WriteFile serializes the result to JSON and writes it to a file.
func (r *Result) WriteFile(path string) error {
	f, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("creating output file: %w", err)
	}
	defer f.Close()

	if err := r.Write(f); err != nil {
		return fmt.Errorf("writing result: %w", err)
	}
	return nil
}

// Parse reads a Result from a reader.
func Parse(r io.Reader) (*Result, error) {
	var result Result
	if err := json.NewDecoder(r).Decode(&result); err != nil {
		return nil, fmt.Errorf("decoding result: %w", err)
	}

	if result.Version != ProtocolVersion {
		return nil, fmt.Errorf("unsupported protocol version: %s (expected %s)", result.Version, ProtocolVersion)
	}

	return &result, nil
}

// ParseFile reads a Result from a file.
func ParseFile(path string) (*Result, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("opening file: %w", err)
	}
	defer f.Close()

	return Parse(f)
}
