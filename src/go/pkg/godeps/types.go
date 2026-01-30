// Package godeps provides utilities for parsing Go module files and generating
// dependency declarations for Nix/Buck2 integration.
package godeps

import "strings"

// Dependency represents a Go module dependency with its metadata.
type Dependency struct {
	// ImportPath is the Go module import path (e.g., "github.com/google/uuid")
	ImportPath string

	// Version is the module version (e.g., "v1.6.0")
	Version string

	// Indirect is true if this is a transitive (indirect) dependency
	Indirect bool

	// GoSumHash is the h1: hash from go.sum (for reference only, not usable by Nix)
	GoSumHash string

	// NixHash is the SRI hash for Nix fetchFromGitHub or similar
	NixHash string
}

// Replace represents a go.mod replace directive.
type Replace struct {
	// Old is the module path being replaced (e.g., "github.com/foo/bar")
	Old string

	// OldVersion is the specific version being replaced (empty for all versions)
	OldVersion string

	// NewPath is the replacement path - either a local path or module path
	NewPath string

	// NewVersion is the replacement version (empty for local paths)
	NewVersion string
}

// IsLocal returns true if this replace directive points to a local path.
func (r Replace) IsLocal() bool {
	return strings.HasPrefix(r.NewPath, ".") || strings.HasPrefix(r.NewPath, "/")
}

// ParseOptions configures the behavior of ParseGoMod.
type ParseOptions struct {
	// IncludeIndirect controls whether indirect dependencies are included.
	// Default: true
	IncludeIndirect bool
}

// DefaultParseOptions returns the default parsing options.
func DefaultParseOptions() ParseOptions {
	return ParseOptions{
		IncludeIndirect: true,
	}
}
