// Package rustdeps provides utilities for parsing Cargo.lock files and generating
// dependency declarations for Nix/Buck2 integration.
package rustdeps

// Crate represents a Rust crate dependency with its metadata.
type Crate struct {
	// Name is the crate name (e.g., "serde")
	Name string

	// Version is the crate version (e.g., "1.0.152")
	Version string

	// Source is the crate source (e.g., "registry+https://github.com/rust-lang/crates.io-index")
	Source string

	// Checksum is the SHA256 checksum from Cargo.lock (hex-encoded)
	Checksum string

	// NixHash is the SRI hash for Nix fetchurl (e.g., "sha256-abc123...")
	NixHash string

	// Features is an optional list of enabled features
	Features []string
}

// ParseOptions configures the behavior of ParseCargoLock.
type ParseOptions struct {
	// IncludeDevDeps controls whether dev-dependencies are included.
	// Default: false (Cargo.lock doesn't distinguish, so this is a no-op)
	IncludeDevDeps bool
}

// DefaultParseOptions returns the default parsing options.
func DefaultParseOptions() ParseOptions {
	return ParseOptions{
		IncludeDevDeps: false,
	}
}
