// Package staleness provides hash-based caching for staleness detection.
package staleness

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// CacheEntry represents a cached staleness check result for a package.
type CacheEntry struct {
	// BuckFile is the path to the rules.star file.
	BuckFile string `json:"buck_file"`

	// SrcListHash is the hash of the sorted source file list.
	SrcListHash string `json:"src_list_hash"`

	// ImportHash is the hash of the sorted import list.
	ImportHash string `json:"import_hash"`

	// BuckFileHash is the hash of the rules.star file contents.
	BuckFileHash string `json:"buck_file_hash"`

	// LastCheck indicates whether the last check found staleness.
	WasStale bool `json:"was_stale"`
}

// Cache stores staleness check results to avoid redundant parsing.
type Cache struct {
	// entries maps rules.star file paths to their cache entries.
	entries map[string]*CacheEntry

	// dirty tracks whether the cache has been modified.
	dirty bool
}

// NewCache creates a new empty cache.
func NewCache() *Cache {
	return &Cache{
		entries: make(map[string]*CacheEntry),
	}
}

// LoadCache loads a cache from a JSON file.
// Returns a new empty cache if the file doesn't exist or can't be read.
func LoadCache(path string) (*Cache, error) {
	cache := NewCache()

	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return cache, nil
	}
	if err != nil {
		return cache, err
	}

	var entries map[string]*CacheEntry
	if err := json.Unmarshal(data, &entries); err != nil {
		// Return empty cache on parse error
		return cache, nil
	}

	cache.entries = entries
	return cache, nil
}

// Save persists the cache to a JSON file.
func (c *Cache) Save(path string) error {
	if !c.dirty {
		return nil
	}

	// Ensure parent directory exists
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}

	data, err := json.MarshalIndent(c.entries, "", "  ")
	if err != nil {
		return err
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return err
	}

	c.dirty = false
	return nil
}

// NeedsCheck returns true if the given rules.star file needs a staleness check.
// This compares the current source files and imports against cached hashes.
func (c *Cache) NeedsCheck(buckFile string, srcFiles, imports []string) bool {
	entry, ok := c.entries[buckFile]
	if !ok {
		return true
	}

	// Check if source file list has changed
	srcHash := hashStrings(srcFiles)
	if srcHash != entry.SrcListHash {
		return true
	}

	// Check if imports have changed
	importHash := hashStrings(imports)
	if importHash != entry.ImportHash {
		return true
	}

	// Check if rules.star file has changed
	buckHash, err := hashFile(buckFile)
	if err != nil {
		return true
	}
	if buckHash != entry.BuckFileHash {
		return true
	}

	return false
}

// Update stores a new cache entry for the given rules.star file.
func (c *Cache) Update(buckFile string, srcFiles, imports []string, wasStale bool) error {
	buckHash, err := hashFile(buckFile)
	if err != nil {
		return err
	}

	c.entries[buckFile] = &CacheEntry{
		BuckFile:     buckFile,
		SrcListHash:  hashStrings(srcFiles),
		ImportHash:   hashStrings(imports),
		BuckFileHash: buckHash,
		WasStale:     wasStale,
	}

	c.dirty = true
	return nil
}

// Get returns the cached entry for a rules.star file, or nil if not cached.
func (c *Cache) Get(buckFile string) *CacheEntry {
	return c.entries[buckFile]
}

// Remove deletes the cache entry for a rules.star file.
func (c *Cache) Remove(buckFile string) {
	if _, ok := c.entries[buckFile]; ok {
		delete(c.entries, buckFile)
		c.dirty = true
	}
}

// Clear removes all cache entries.
func (c *Cache) Clear() {
	c.entries = make(map[string]*CacheEntry)
	c.dirty = true
}

// hashStrings computes a hash of a sorted list of strings.
func hashStrings(strs []string) string {
	// Sort for determinism
	sorted := make([]string, len(strs))
	copy(sorted, strs)
	sort.Strings(sorted)

	h := sha256.New()
	for _, s := range sorted {
		h.Write([]byte(s))
		h.Write([]byte{0}) // Separator
	}

	return hex.EncodeToString(h.Sum(nil))
}

// hashFile computes a SHA256 hash of a file's contents.
func hashFile(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}

	h := sha256.Sum256(data)
	return hex.EncodeToString(h[:]), nil
}

// CachedCheck performs a staleness check with caching.
// If the cache indicates no changes, returns the cached result immediately.
// Otherwise performs the full check and updates the cache.
type CachedCheck struct {
	Cache    *Cache
	BuckFile string
}

// CheckGoPackage performs a cached staleness check on a Go package.
// Returns whether the rules.star file is stale (needs regeneration).
func (cc *CachedCheck) CheckGoPackage() (*GoPackageResult, error) {
	dir := filepath.Dir(cc.BuckFile)

	// Get current source files
	srcFiles, err := globGoSrcs(dir, false)
	if err != nil {
		return nil, err
	}

	// Get current imports
	imports, err := parseGoImports(dir, false)
	if err != nil {
		return nil, err
	}

	// Filter to external imports
	pkgName, _ := parseBuckPackageName(cc.BuckFile, "go_library")
	externalImports := filterExternalImports(imports, pkgName)

	// Check if we can use cached result
	if !cc.Cache.NeedsCheck(cc.BuckFile, srcFiles, externalImports) {
		entry := cc.Cache.Get(cc.BuckFile)
		return &GoPackageResult{
			BuckFile:   cc.BuckFile,
			Stale:      entry.WasStale,
			FromCache:  true,
			SrcFiles:   srcFiles,
			Imports:    externalImports,
		}, nil
	}

	// Perform full check
	srcResult, err := CheckGoSrcList(cc.BuckFile)
	if err != nil {
		return nil, err
	}

	importResult, err := CheckGoImports(cc.BuckFile)
	if err != nil {
		return nil, err
	}

	stale := srcResult.Stale || importResult.Stale

	// Update cache
	if err := cc.Cache.Update(cc.BuckFile, srcFiles, externalImports, stale); err != nil {
		// Log error but don't fail the check
	}

	return &GoPackageResult{
		BuckFile:     cc.BuckFile,
		Stale:        stale,
		FromCache:    false,
		SrcFiles:     srcFiles,
		Imports:      externalImports,
		SrcResult:    srcResult,
		ImportResult: importResult,
	}, nil
}

// GoPackageResult contains the result of a Go package staleness check.
type GoPackageResult struct {
	// BuckFile is the path to the rules.star file.
	BuckFile string

	// Stale is true if the rules.star file needs regeneration.
	Stale bool

	// FromCache is true if the result came from cache.
	FromCache bool

	// SrcFiles is the list of source files found.
	SrcFiles []string

	// Imports is the list of external imports found.
	Imports []string

	// SrcResult is the detailed source list result (nil if from cache).
	SrcResult *SrcListResult

	// ImportResult is the detailed import result (nil if from cache).
	ImportResult *ImportResult
}

// DefaultCachePath returns the default path for the staleness cache file.
// Uses TURNKEY_CACHE_DIR if set, otherwise $XDG_CACHE_HOME/turnkey (or ~/.cache/turnkey).
func DefaultCachePath() string {
	cacheDir := os.Getenv("TURNKEY_CACHE_DIR")
	if cacheDir == "" {
		// Follow XDG Base Directory Specification
		xdgCache := os.Getenv("XDG_CACHE_HOME")
		if xdgCache == "" {
			home, err := os.UserHomeDir()
			if err != nil {
				home = "."
			}
			xdgCache = filepath.Join(home, ".cache")
		}
		cacheDir = filepath.Join(xdgCache, "turnkey")
	}
	return filepath.Join(cacheDir, "staleness-cache.json")
}

// FindBuckFiles finds all rules.star files in a directory tree.
func FindBuckFiles(root string) ([]string, error) {
	var buckFiles []string

	err := filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // Skip errors
		}

		// Skip hidden directories and common non-source directories
		if info.IsDir() {
			name := info.Name()
			if strings.HasPrefix(name, ".") || name == "buck-out" || name == "node_modules" {
				return filepath.SkipDir
			}
			return nil
		}

		if info.Name() == "rules.star" {
			buckFiles = append(buckFiles, path)
		}

		return nil
	})

	return buckFiles, err
}
