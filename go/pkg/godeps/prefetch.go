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
		_, _ = fmt.Fprintf(p.Logger, "prefetching %s/%s@%s...\n", owner, repo, version)
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
		_, _ = fmt.Fprintf(p.Logger, "prefetching golang/%s@%s (from %s)...\n", repo, version, importPath)
	}

	return runNixPrefetchGitHub("golang", repo, version)
}

// GopkgInPrefetcher handles gopkg.in/* modules by mapping to GitHub.
// gopkg.in/yaml.v3 -> github.com/go-yaml/yaml
// gopkg.in/user/pkg.v3 -> github.com/user/pkg
type GopkgInPrefetcher struct {
	Logger io.Writer
}

// Supports returns true for gopkg.in/* import paths.
func (p *GopkgInPrefetcher) Supports(importPath string) bool {
	return strings.HasPrefix(importPath, "gopkg.in/")
}

// Prefetch maps gopkg.in paths to GitHub and fetches.
func (p *GopkgInPrefetcher) Prefetch(importPath, version string) (string, error) {
	owner, repo, err := parseGopkgInPath(importPath)
	if err != nil {
		return "", err
	}

	if p.Logger != nil {
		_, _ = fmt.Fprintf(p.Logger, "prefetching %s/%s@%s (from %s)...\n", owner, repo, version, importPath)
	}

	return runNixPrefetchGitHub(owner, repo, version)
}

// parseGopkgInPath parses gopkg.in import paths into GitHub owner/repo.
// gopkg.in/yaml.v3 -> go-yaml/yaml
// gopkg.in/user/pkg.v3 -> user/pkg
func parseGopkgInPath(importPath string) (owner, repo string, err error) {
	// Remove gopkg.in/ prefix
	path := strings.TrimPrefix(importPath, "gopkg.in/")
	parts := strings.Split(path, "/")

	if len(parts) == 1 {
		// gopkg.in/yaml.v3 -> go-yaml/yaml
		// Strip version suffix (.v3, .v2, etc.)
		repo = stripVersionSuffix(parts[0])
		owner = "go-" + repo
		return owner, repo, nil
	}

	if len(parts) >= 2 {
		// gopkg.in/user/pkg.v3 -> user/pkg
		owner = parts[0]
		repo = stripVersionSuffix(parts[1])
		return owner, repo, nil
	}

	return "", "", fmt.Errorf("invalid gopkg.in path: %s", importPath)
}

// stripVersionSuffix removes .v1, .v2, etc. from package names.
func stripVersionSuffix(name string) string {
	// Find last dot followed by 'v' and digits
	for i := len(name) - 1; i >= 0; i-- {
		if name[i] == '.' && i+1 < len(name) && name[i+1] == 'v' {
			// Check remaining chars are digits
			allDigits := true
			for j := i + 2; j < len(name); j++ {
				if name[j] < '0' || name[j] > '9' {
					allDigits = false
					break
				}
			}
			if allDigits && i+2 < len(name) {
				return name[:i]
			}
		}
	}
	return name
}

// UberGoPrefetcher handles go.uber.org/* modules by mapping to github.com/uber-go/*.
type UberGoPrefetcher struct {
	Logger io.Writer
}

// Supports returns true for go.uber.org/* import paths.
func (p *UberGoPrefetcher) Supports(importPath string) bool {
	return strings.HasPrefix(importPath, "go.uber.org/")
}

// Prefetch maps go.uber.org/foo to github.com/uber-go/foo and fetches.
func (p *UberGoPrefetcher) Prefetch(importPath, version string) (string, error) {
	parts := strings.Split(importPath, "/")
	if len(parts) < 2 {
		return "", fmt.Errorf("invalid go.uber.org path: %s", importPath)
	}
	repo := parts[1]

	if p.Logger != nil {
		_, _ = fmt.Fprintf(p.Logger, "prefetching uber-go/%s@%s (from %s)...\n", repo, version, importPath)
	}

	return runNixPrefetchGitHub("uber-go", repo, version)
}

// GoProxyPrefetcher fetches modules from proxy.golang.org as a fallback.
// This works for any public Go module but produces zip-based hashes.
type GoProxyPrefetcher struct {
	Logger io.Writer
}

// Supports returns true for any import path (fallback prefetcher).
func (p *GoProxyPrefetcher) Supports(importPath string) bool {
	return true
}

// Prefetch downloads the module zip from proxy.golang.org and computes the hash.
func (p *GoProxyPrefetcher) Prefetch(importPath, version string) (string, error) {
	// URL encode the module path for proxy.golang.org
	// Handles / -> ! conversion and uppercase -> !lowercase per module proxy protocol
	escapedPath := escapeModulePath(importPath)

	url := fmt.Sprintf("https://proxy.golang.org/%s/@v/%s.zip", escapedPath, version)

	if p.Logger != nil {
		_, _ = fmt.Fprintf(p.Logger, "prefetching %s@%s from proxy.golang.org...\n", importPath, version)
	}

	return runNixPrefetchURL(url)
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

// runNixPrefetchURL runs nix-prefetch-url (or nix-prefetch-cached) and returns the SRI hash.
func runNixPrefetchURL(url string) (string, error) {
	// Try nix-prefetch-cached first (with caching), fall back to nix-prefetch-url
	cmd := exec.Command("nix-prefetch-cached", url)
	output, err := cmd.Output()
	if err != nil {
		// Fallback to nix-prefetch-url if cached version not available
		cmd = exec.Command("nix-prefetch-url", "--type", "sha256", url)
		output, err = cmd.Output()
		if err != nil {
			return "", fmt.Errorf("nix-prefetch-url failed: %w", err)
		}
	}

	hash := strings.TrimSpace(string(output))

	// nix-prefetch-cached returns SRI format, nix-prefetch-url returns base32
	if strings.HasPrefix(hash, "sha256-") {
		return hash, nil
	}

	// Convert base32 to SRI format using nix hash to-sri
	cmd = exec.Command("nix", "hash", "to-sri", "--type", "sha256", hash)
	sriOutput, err := cmd.Output()
	if err != nil {
		// Fallback: return the base32 hash if conversion fails
		return hash, nil
	}

	return strings.TrimSpace(string(sriOutput)), nil
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
// Order matters: more specific prefetchers come first, with GoProxy as fallback.
func DefaultPrefetcher(logger io.Writer) Prefetcher {
	return ChainPrefetcher{
		&GolangOrgPrefetcher{Logger: logger}, // golang.org/x/* -> github.com/golang/*
		&GopkgInPrefetcher{Logger: logger},   // gopkg.in/* -> github.com/*
		&UberGoPrefetcher{Logger: logger},    // go.uber.org/* -> github.com/uber-go/*
		&GitHubPrefetcher{Logger: logger},    // github.com/*
		&GoProxyPrefetcher{Logger: logger},   // Fallback for any public module
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

