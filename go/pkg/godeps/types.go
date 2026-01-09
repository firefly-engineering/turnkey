// Package godeps provides utilities for parsing Go module files and generating
// dependency declarations for Nix/Buck2 integration.
package godeps

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

// Meta contains metadata for the go-deps.toml file.
type Meta struct {
	// VendorHash is the combined hash of all dependencies for buildGoModule.
	// This is computed by `go mod download` + `nix hash path`.
	VendorHash string
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
