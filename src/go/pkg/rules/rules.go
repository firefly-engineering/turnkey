// Package rules provides utilities for managing rules.star files,
// including parsing, dependency detection, and automatic synchronization.
package rules

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"
)

// Target represents a Buck2 target definition in a rules.star file.
type Target struct {
	// Name is the target name (from name = "...").
	Name string

	// Rule is the rule type (e.g., "go_binary", "rust_library").
	Rule string

	// Srcs lists the source files for this target.
	Srcs []string

	// Deps lists the dependencies (both auto-managed and preserved).
	Deps []string

	// AutoDeps are dependencies managed by turnkey (between auto markers).
	AutoDeps []string

	// PreservedDeps are manually managed dependencies (between preserve markers).
	PreservedDeps []string

	// RawContent is the original text of this target (for non-deps attributes).
	RawContent string

	// StartLine is the line number where this target starts (1-indexed).
	StartLine int

	// EndLine is the line number where this target ends (1-indexed).
	EndLine int
}

// RulesFile represents a parsed rules.star file.
type RulesFile struct {
	// Path is the absolute path to the rules.star file.
	Path string

	// Loads are the load() statements at the top of the file.
	Loads []string

	// Targets are the Buck2 targets defined in the file.
	Targets []*Target

	// Hash is the content hash stored in the file header (if any).
	Hash string

	// RawContent is the original file content.
	RawContent string
}

// Import represents a detected import from source code.
type Import struct {
	// Path is the import path (e.g., "github.com/google/uuid" or "fmt").
	Path string

	// SourceFile is the file containing this import.
	SourceFile string

	// Line is the line number of the import (1-indexed).
	Line int

	// IsStdLib is true if this is a standard library import.
	IsStdLib bool
}

// Dependency represents a resolved Buck2 dependency.
type Dependency struct {
	// Target is the Buck2 target path (e.g., "//src/go/pkg/foo:foo").
	Target string

	// Type indicates if this is internal or external.
	Type DependencyType

	// ImportPath is the original import path that resolved to this dep.
	ImportPath string
}

// DependencyType indicates the source of a dependency.
type DependencyType int

const (
	// DependencyInternal is a monorepo-internal dependency.
	DependencyInternal DependencyType = iota
	// DependencyExternal is an external vendored dependency.
	DependencyExternal
	// DependencyStdLib is a standard library dependency (usually ignored).
	DependencyStdLib
)

// Marker constants for preserved sections.
const (
	MarkerAutoStart     = "# turnkey:auto-start"
	MarkerAutoEnd       = "# turnkey:auto-end"
	MarkerPreserveStart = "# turnkey:preserve-start"
	MarkerPreserveEnd   = "# turnkey:preserve-end"
	MarkerHeader        = "# Auto-managed by turnkey."
)

// ComputeDepsHash computes a SHA256 hash of a sorted deps list.
// This is used to detect when deps have changed.
func ComputeDepsHash(deps []string) string {
	sorted := make([]string, len(deps))
	copy(sorted, deps)
	sort.Strings(sorted)

	h := sha256.New()
	for _, dep := range sorted {
		h.Write([]byte(dep))
		h.Write([]byte("\n"))
	}
	return hex.EncodeToString(h.Sum(nil))[:16] // Short hash for readability
}

// FormatDeps formats a list of dependencies for insertion into rules.star.
func FormatDeps(autoDeps, preservedDeps []string, indent string) string {
	var lines []string

	if len(autoDeps) > 0 || len(preservedDeps) > 0 {
		lines = append(lines, fmt.Sprintf("%s%s", indent, MarkerAutoStart))
		for _, dep := range autoDeps {
			lines = append(lines, fmt.Sprintf("%s\"%s\",", indent, dep))
		}
		lines = append(lines, fmt.Sprintf("%s%s", indent, MarkerAutoEnd))
	}

	if len(preservedDeps) > 0 {
		lines = append(lines, fmt.Sprintf("%s%s", indent, MarkerPreserveStart))
		for _, dep := range preservedDeps {
			lines = append(lines, fmt.Sprintf("%s\"%s\",", indent, dep))
		}
		lines = append(lines, fmt.Sprintf("%s%s", indent, MarkerPreserveEnd))
	}

	return strings.Join(lines, "\n")
}

// String returns the dependency type as a string.
func (t DependencyType) String() string {
	switch t {
	case DependencyInternal:
		return "internal"
	case DependencyExternal:
		return "external"
	case DependencyStdLib:
		return "stdlib"
	default:
		return "unknown"
	}
}
