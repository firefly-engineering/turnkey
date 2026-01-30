package godeps

import (
	"bufio"
	"bytes"
	"fmt"
	"sort"
	"strings"

	"golang.org/x/mod/modfile"
)

// ParseGoMod parses go.mod content and extracts dependencies.
// It takes raw bytes for testability (no file I/O).
func ParseGoMod(data []byte, opts ParseOptions) ([]Dependency, error) {
	f, err := modfile.Parse("go.mod", data, nil)
	if err != nil {
		return nil, fmt.Errorf("parsing go.mod: %w", err)
	}

	var deps []Dependency
	for _, req := range f.Require {
		// Skip indirect dependencies if not requested
		if req.Indirect && !opts.IncludeIndirect {
			continue
		}

		deps = append(deps, Dependency{
			ImportPath: req.Mod.Path,
			Version:    req.Mod.Version,
			Indirect:   req.Indirect,
		})
	}

	// Sort by import path for consistent output
	sort.Slice(deps, func(i, j int) bool {
		return deps[i].ImportPath < deps[j].ImportPath
	})

	return deps, nil
}

// ParseGoSum parses go.sum content and returns a map of "path version" -> h1:hash.
// It takes raw bytes for testability (no file I/O).
func ParseGoSum(data []byte) (map[string]string, error) {
	hashes := make(map[string]string)
	scanner := bufio.NewScanner(bytes.NewReader(data))

	// go.sum format: github.com/foo/bar v1.0.0 h1:abc123...
	// We want the h1: hash, not the /go.mod hash
	for scanner.Scan() {
		line := scanner.Text()
		parts := strings.Fields(line)
		if len(parts) != 3 {
			continue
		}

		importPath := parts[0]
		version := parts[1]
		hash := parts[2]

		// Skip go.mod hashes (we want source hashes)
		if strings.HasSuffix(version, "/go.mod") {
			continue
		}

		// Only take h1: hashes
		if strings.HasPrefix(hash, "h1:") {
			key := importPath + " " + version
			hashes[key] = hash
		}
	}

	return hashes, scanner.Err()
}

// MergeHashes merges go.sum hashes into the dependency list.
// The hashes map should be keyed by "importPath version".
func MergeHashes(deps []Dependency, hashes map[string]string) {
	for i, dep := range deps {
		key := dep.ImportPath + " " + dep.Version
		if hash, ok := hashes[key]; ok {
			deps[i].GoSumHash = hash
		}
	}
}

// ParseReplaces extracts replace directives from go.mod content.
// It takes raw bytes for testability (no file I/O).
func ParseReplaces(data []byte) ([]Replace, error) {
	f, err := modfile.Parse("go.mod", data, nil)
	if err != nil {
		return nil, fmt.Errorf("parsing go.mod: %w", err)
	}

	var replaces []Replace
	for _, rep := range f.Replace {
		replaces = append(replaces, Replace{
			Old:        rep.Old.Path,
			OldVersion: rep.Old.Version,
			NewPath:    rep.New.Path,
			NewVersion: rep.New.Version,
		})
	}

	return replaces, nil
}

// FilterLocalReplaces returns only the replace directives that point to local paths.
func FilterLocalReplaces(replaces []Replace) []Replace {
	var local []Replace
	for _, r := range replaces {
		if r.IsLocal() {
			local = append(local, r)
		}
	}
	return local
}
