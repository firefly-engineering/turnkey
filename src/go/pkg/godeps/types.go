// Package godeps provides utilities for parsing Go module files and generating
// dependency declarations for Nix/Buck2 integration.
package godeps

// Dependency represents a Go module dependency with its metadata.
type Dependency struct {
	// ImportPath is the Go module import path (e.g., "github.com/google/uuid")
	// This is the path used in import statements and for the vendor directory structure.
	ImportPath string

	// FetchPath is the actual module path to fetch from, when different from ImportPath.
	// This is used when a replace directive points to an external fork.
	// Example: ImportPath="github.com/original/pkg", FetchPath="github.com/myfork/pkg"
	// If empty, FetchPath is the same as ImportPath.
	FetchPath string

	// Version is the module version (e.g., "v1.6.0")
	Version string

	// Indirect is true if this is a transitive (indirect) dependency
	Indirect bool

	// GoSumHash is the h1: hash from go.sum (for reference only, not usable by Nix)
	GoSumHash string

	// NixHash is the SRI hash for Nix fetchFromGitHub or similar
	NixHash string
}

// EffectiveFetchPath returns the path to fetch from.
// If FetchPath is set, returns FetchPath; otherwise returns ImportPath.
func (d Dependency) EffectiveFetchPath() string {
	if d.FetchPath != "" {
		return d.FetchPath
	}
	return d.ImportPath
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
