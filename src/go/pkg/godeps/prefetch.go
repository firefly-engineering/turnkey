package godeps

import (
	"bytes"
	"fmt"
	"io"
	"os/exec"
	"strings"
)

// Module is one module version whose source is hashed.
type Module struct {
	Path    string
	Version string
}

// Prefetched is the Nix hash of one module's source, or why it has none.
type Prefetched struct {
	Hash string
	Err  error
}

// Prefetcher fetches Nix-compatible hashes for Go module sources, all of a
// module graph's at once.
type Prefetcher interface {
	// Prefetch returns one result per module, in order: an SRI hash (e.g.
	// "sha256-abc123...") or the error for that module alone.
	Prefetch(mods []Module) []Prefetched
}

// GoProxyPrefetcher hashes modules' zips from proxy.golang.org, unpacked as
// Nix fetchzip unpacks them. The godeps cell fetches every module from the
// proxy (nix/lib/deps-cell/adapters/go.nix), so its hash is the only one that
// matches; a GitHub archive of the same version hashes differently.
//
// All modules go to one nix-prefetch-cached --batch process, which loads
// turnkey's prefetch cache (src/rust/prefetch-cache) once and fetches the
// misses in parallel.
type GoProxyPrefetcher struct {
	// Command is the nix-prefetch-cached to run; "nix-prefetch-cached"
	// (looked up on PATH) when empty.
	Command string
	// Logger gets nix-prefetch-cached's progress and warnings.
	Logger io.Writer
	// NoCache fetches afresh instead of reusing a hash nix-prefetch-cached
	// has already computed.
	NoCache bool
}

// Prefetch hashes every module's proxy.golang.org zip in one batch.
func (p *GoProxyPrefetcher) Prefetch(mods []Module) []Prefetched {
	if len(mods) == 0 {
		return nil
	}
	urls := make([]string, len(mods))
	for i, m := range mods {
		urls[i] = proxyZipURL(m)
	}

	command := p.Command
	if command == "" {
		command = "nix-prefetch-cached"
	}
	// Use --unpack to match Nix fetchzip behavior (unpacks zip and strips root)
	args := []string{"--batch", "--unpack"}
	if p.NoCache {
		args = append(args, "--no-cache")
	}
	cmd := exec.Command(command, args...)
	cmd.Stdin = strings.NewReader(strings.Join(urls, "\n") + "\n")
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if p.Logger != nil {
		cmd.Stderr = io.MultiWriter(&stderr, p.Logger)
	}
	output, err := cmd.Output()

	lines := strings.Split(strings.TrimSuffix(string(output), "\n"), "\n")
	if err == nil && len(lines) != len(urls) {
		err = fmt.Errorf("%d results for %d URLs", len(lines), len(urls))
	}
	results := make([]Prefetched, len(mods))
	for i, url := range urls {
		switch {
		case err != nil:
			results[i].Err = fmt.Errorf("nix-prefetch-cached %s: %v: %s", url, err, strings.TrimSpace(stderr.String()))
		case strings.HasPrefix(lines[i], "error: "):
			results[i].Err = fmt.Errorf("nix-prefetch-cached %s: %s", url, strings.TrimPrefix(lines[i], "error: "))
		default:
			results[i].Hash = lines[i]
		}
	}
	return results
}

// proxyZipURL is the proxy.golang.org zip of a module version.
func proxyZipURL(m Module) string {
	// Handles uppercase -> !lowercase per module proxy protocol
	return fmt.Sprintf("https://proxy.golang.org/%s/@v/%s.zip", escapeModulePath(m.Path), m.Version)
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

// DefaultPrefetcher returns the prefetcher godeps-gen uses: the Go proxy,
// the source the godeps cell fetches from.
func DefaultPrefetcher(logger io.Writer, noCache bool) Prefetcher {
	return &GoProxyPrefetcher{Logger: logger, NoCache: noCache}
}

// PrefetchAll fetches Nix hashes for all dependencies using the given
// prefetcher, in one call. Errors are reported via the errHandler callback;
// the other dependencies still get their hashes.
func PrefetchAll(deps []Dependency, p Prefetcher, errHandler func(dep Dependency, err error)) {
	mods := make([]Module, len(deps))
	for i := range deps {
		// Use EffectiveFetchPath for the actual fetch, but keep ImportPath for storage
		mods[i] = Module{Path: deps[i].EffectiveFetchPath(), Version: deps[i].Version}
	}

	results := p.Prefetch(mods)
	for i := range deps {
		switch {
		case i >= len(results):
			if errHandler != nil {
				errHandler(deps[i], fmt.Errorf("no hash returned for %s", mods[i].Path))
			}
		case results[i].Err != nil:
			if errHandler != nil {
				errHandler(deps[i], results[i].Err)
			}
		default:
			deps[i].NixHash = results[i].Hash
		}
	}
}
