package rustdeps

import (
	"fmt"
	"io"
	"os/exec"
	"strings"
)

// Prefetcher fetches Nix-compatible hashes for Rust crate sources.
type Prefetcher interface {
	// Prefetch fetches the Nix hash for the given crate at the specified version.
	// Returns the SRI hash (e.g., "sha256-abc123...") or an error.
	Prefetch(crateName, version string) (string, error)
}

// CratesIOPrefetcher fetches hashes for crates from crates.io using nix-prefetch-url.
type CratesIOPrefetcher struct {
	// Logger receives progress messages. If nil, no logging is done.
	Logger io.Writer
}

// Prefetch downloads the crate from crates.io and returns its SRI hash.
// Uses --unpack to match fetchzip's behavior (hash of unpacked contents).
func (p *CratesIOPrefetcher) Prefetch(crateName, version string) (string, error) {
	url := fmt.Sprintf("https://crates.io/api/v1/crates/%s/%s/download", crateName, version)

	if p.Logger != nil {
		fmt.Fprintf(p.Logger, "prefetching %s@%s from crates.io...\n", crateName, version)
	}

	return runNixPrefetchURL(url)
}

// runNixPrefetchURL runs nix-prefetch-url --unpack and returns the SRI hash.
// The --unpack flag is critical: it unpacks the archive and hashes the contents,
// which matches Nix's fetchzip behavior.
func runNixPrefetchURL(url string) (string, error) {
	// Use nix-prefetch-url with --unpack to match fetchzip behavior
	// fetchzip computes hash of unpacked contents, not the archive itself
	cmd := exec.Command("nix-prefetch-url", "--type", "sha256", "--unpack", url)
	output, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("nix-prefetch-url failed: %w", err)
	}

	// nix-prefetch-url outputs the hash in base32 format
	// Convert to SRI format using nix hash to-sri
	base32Hash := strings.TrimSpace(string(output))
	cmd = exec.Command("nix", "hash", "to-sri", "--type", "sha256", base32Hash)
	sriOutput, err := cmd.Output()
	if err != nil {
		// Fallback: return the base32 hash if conversion fails
		return base32Hash, nil
	}

	return strings.TrimSpace(string(sriOutput)), nil
}

// DefaultPrefetcher returns a CratesIOPrefetcher.
func DefaultPrefetcher(logger io.Writer) Prefetcher {
	return &CratesIOPrefetcher{Logger: logger}
}

// PrefetchAll fetches Nix hashes for all crates using the given prefetcher.
// This is useful when Cargo.lock doesn't have checksums (shouldn't happen normally).
// Errors are reported via the errHandler callback; processing continues on error.
func PrefetchAll(crates []Crate, p Prefetcher, errHandler func(crate Crate, err error)) {
	for i := range crates {
		// Skip if we already have a hash
		if crates[i].NixHash != "" {
			continue
		}

		hash, err := p.Prefetch(crates[i].Name, crates[i].Version)
		if err != nil {
			if errHandler != nil {
				errHandler(crates[i], err)
			}
			continue
		}
		crates[i].NixHash = hash
	}
}
