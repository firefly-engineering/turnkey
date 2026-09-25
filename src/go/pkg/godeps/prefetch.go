package godeps

import (
	"fmt"
	"io"
	"os/exec"
	"strings"
)

// Prefetcher fetches Nix-compatible hashes for Go module sources.
type Prefetcher interface {
	// Supports returns true if this prefetcher can handle the given import path.
	Supports(importPath string) bool

	// Prefetch fetches the Nix hash for the given module at the specified version.
	// Returns the SRI hash (e.g., "sha256-abc123...") or an error.
	Prefetch(importPath, version string) (string, error)
}

// PrefetchFunc is an adapter to allow ordinary functions to be used as Prefetchers.
type PrefetchFunc func(importPath, version string) (string, error)

// Supports always returns true for PrefetchFunc.
func (f PrefetchFunc) Supports(importPath string) bool {
	return true
}

// Prefetch calls the underlying function.
func (f PrefetchFunc) Prefetch(importPath, version string) (string, error) {
	return f(importPath, version)
}

// GoProxyPrefetcher hashes a module's zip from proxy.golang.org, unpacked as
// Nix fetchzip unpacks it. The godeps cell fetches every module from the
// proxy (nix/lib/deps-cell/adapters/go.nix), so its hash is the only one that
// matches; a GitHub archive of the same version hashes differently.
type GoProxyPrefetcher struct {
	Logger io.Writer
	// NoCache fetches afresh instead of reusing a hash nix-prefetch-cached
	// has already computed.
	NoCache bool
}

// Supports returns true for any import path: every public module is on the
// proxy.
func (p *GoProxyPrefetcher) Supports(importPath string) bool {
	return true
}

// Prefetch downloads the module zip from proxy.golang.org and computes the hash.
// The hash is computed on the unpacked content to match Nix fetchzip behavior.
func (p *GoProxyPrefetcher) Prefetch(importPath, version string) (string, error) {
	// URL encode the module path for proxy.golang.org
	// Handles / -> ! conversion and uppercase -> !lowercase per module proxy protocol
	escapedPath := escapeModulePath(importPath)

	url := fmt.Sprintf("https://proxy.golang.org/%s/@v/%s.zip", escapedPath, version)

	if p.Logger != nil {
		_, _ = fmt.Fprintf(p.Logger, "prefetching %s@%s from proxy.golang.org...\n", importPath, version)
	}

	// Use unpack=true to match Nix fetchzip behavior (unpacks zip and strips root)
	return runNixPrefetchCached(url, true, p.NoCache)
}

// escapeModulePath escapes a module path for use in proxy.golang.org URLs.
// Uppercase letters become !(lowercase) per the module proxy protocol.
func escapeModulePath(path string) string {
	var result strings.Builder
	for _, r := range path {
		if r >= 'A' && r <= 'Z' {
			result.WriteByte('!')
			result.WriteRune(r + 32) // lowercase
		} else {
			result.WriteRune(r)
		}
	}
	return result.String()
}

// runNixPrefetchCached hashes url with nix-prefetch-cached, which keeps
// turnkey's prefetch cache (src/rust/prefetch-cache) and returns an SRI hash.
// Use unpack=true for archives that will be extracted by fetchzip.
func runNixPrefetchCached(url string, unpack, noCache bool) (string, error) {
	var args []string
	if unpack {
		args = append(args, "--unpack")
	}
	if noCache {
		args = append(args, "--no-cache")
	}
	output, err := exec.Command("nix-prefetch-cached", append(args, url)...).Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return "", fmt.Errorf("nix-prefetch-cached %s: %s", url, strings.TrimSpace(string(exitErr.Stderr)))
		}
		return "", fmt.Errorf("nix-prefetch-cached %s: %w", url, err)
	}
	return strings.TrimSpace(string(output)), nil
}

// DefaultPrefetcher returns the prefetcher godeps-gen uses: the Go proxy,
// the source the godeps cell fetches from.
func DefaultPrefetcher(logger io.Writer, noCache bool) Prefetcher {
	return &GoProxyPrefetcher{Logger: logger, NoCache: noCache}
}

// PrefetchAll fetches Nix hashes for all dependencies using the given prefetcher.
// Errors are reported via the errHandler callback; processing continues on error.
func PrefetchAll(deps []Dependency, p Prefetcher, errHandler func(dep Dependency, err error)) {
	for i := range deps {
		// Use EffectiveFetchPath for the actual fetch, but keep ImportPath for storage
		fetchPath := deps[i].EffectiveFetchPath()

		if !p.Supports(fetchPath) {
			if errHandler != nil {
				errHandler(deps[i], fmt.Errorf("no prefetcher supports %s", fetchPath))
			}
			continue
		}

		hash, err := p.Prefetch(fetchPath, deps[i].Version)
		if err != nil {
			if errHandler != nil {
				errHandler(deps[i], err)
			}
			continue
		}
		deps[i].NixHash = hash
	}
}
