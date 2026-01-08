package godeps

import (
	"encoding/json"
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

// ChainPrefetcher tries multiple prefetchers in order until one succeeds.
type ChainPrefetcher []Prefetcher

// Supports returns true if any prefetcher in the chain supports the import path.
func (c ChainPrefetcher) Supports(importPath string) bool {
	for _, p := range c {
		if p.Supports(importPath) {
			return true
		}
	}
	return false
}

// Prefetch tries each prefetcher in order until one succeeds.
func (c ChainPrefetcher) Prefetch(importPath, version string) (string, error) {
	var lastErr error
	for _, p := range c {
		if !p.Supports(importPath) {
			continue
		}
		hash, err := p.Prefetch(importPath, version)
		if err == nil {
			return hash, nil
		}
		lastErr = err
	}
	if lastErr != nil {
		return "", lastErr
	}
	return "", fmt.Errorf("no prefetcher supports %s", importPath)
}

// GitHubPrefetcher fetches hashes for github.com modules using nix-prefetch-github.
type GitHubPrefetcher struct {
	// Logger receives progress messages. If nil, no logging is done.
	Logger io.Writer
}

// Supports returns true for github.com import paths.
func (p *GitHubPrefetcher) Supports(importPath string) bool {
	return strings.HasPrefix(importPath, "github.com/")
}

// Prefetch uses nix-prefetch-github to get the hash.
func (p *GitHubPrefetcher) Prefetch(importPath, version string) (string, error) {
	owner, repo, err := parseGitHubPath(importPath)
	if err != nil {
		return "", err
	}

	if p.Logger != nil {
		fmt.Fprintf(p.Logger, "prefetching %s/%s@%s...\n", owner, repo, version)
	}

	return runNixPrefetchGitHub(owner, repo, version)
}

// GolangOrgPrefetcher handles golang.org/x/* modules by mapping to github.com/golang/*.
type GolangOrgPrefetcher struct {
	// Logger receives progress messages. If nil, no logging is done.
	Logger io.Writer
}

// Supports returns true for golang.org/x/* import paths.
func (p *GolangOrgPrefetcher) Supports(importPath string) bool {
	return strings.HasPrefix(importPath, "golang.org/x/")
}

// Prefetch maps golang.org/x/foo to github.com/golang/foo and fetches.
func (p *GolangOrgPrefetcher) Prefetch(importPath, version string) (string, error) {
	// golang.org/x/mod -> github.com/golang/mod
	parts := strings.Split(importPath, "/")
	if len(parts) < 3 {
		return "", fmt.Errorf("invalid golang.org/x path: %s", importPath)
	}
	repo := parts[2]

	if p.Logger != nil {
		fmt.Fprintf(p.Logger, "prefetching golang/%s@%s (from %s)...\n", repo, version, importPath)
	}

	return runNixPrefetchGitHub("golang", repo, version)
}

// parseGitHubPath extracts owner and repo from a GitHub import path.
func parseGitHubPath(importPath string) (owner, repo string, err error) {
	parts := strings.Split(importPath, "/")
	if len(parts) < 3 || parts[0] != "github.com" {
		return "", "", fmt.Errorf("not a github.com import path: %s", importPath)
	}
	return parts[1], parts[2], nil
}

// runNixPrefetchGitHub runs nix-prefetch-github and returns the hash.
func runNixPrefetchGitHub(owner, repo, version string) (string, error) {
	cmd := exec.Command("nix-prefetch-github", owner, repo, "--rev", version, "--json")
	output, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("nix-prefetch-github failed: %w", err)
	}

	var result struct {
		Hash string `json:"hash"`
	}
	if err := json.Unmarshal(output, &result); err != nil {
		return "", fmt.Errorf("failed to parse nix-prefetch-github output: %w", err)
	}

	return result.Hash, nil
}

// DefaultPrefetcher returns a ChainPrefetcher with the standard prefetchers.
func DefaultPrefetcher(logger io.Writer) Prefetcher {
	return ChainPrefetcher{
		&GolangOrgPrefetcher{Logger: logger},
		&GitHubPrefetcher{Logger: logger},
		// TODO: Add GoProxyPrefetcher as fallback
	}
}

// PrefetchAll fetches Nix hashes for all dependencies using the given prefetcher.
// Errors are reported via the errHandler callback; processing continues on error.
func PrefetchAll(deps []Dependency, p Prefetcher, errHandler func(dep Dependency, err error)) {
	for i := range deps {
		if !p.Supports(deps[i].ImportPath) {
			if errHandler != nil {
				errHandler(deps[i], fmt.Errorf("no prefetcher supports %s", deps[i].ImportPath))
			}
			continue
		}

		hash, err := p.Prefetch(deps[i].ImportPath, deps[i].Version)
		if err != nil {
			if errHandler != nil {
				errHandler(deps[i], err)
			}
			continue
		}
		deps[i].NixHash = hash
	}
}
