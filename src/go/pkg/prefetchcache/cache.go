// Package prefetchcache provides a shared cache for Nix hash lookups.
//
// This package is compatible with the Rust prefetch-cache library,
// sharing the same JSON file format and cache location. It's used by
// godeps-gen to avoid redundant prefetching of already-known hashes.
//
// Cache location (in order of precedence):
//  1. TURNKEY_CACHE_DIR environment variable
//  2. ~/.cache/turnkey/ (default)
//
// The cache file is `prefetch-cache.json`.
package prefetchcache

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

const (
	// CacheVersion is the current cache format version.
	// Must match the Rust prefetch-cache library.
	CacheVersion = 1

	// CacheFilename is the name of the cache file.
	CacheFilename = "prefetch-cache.json"

	// CacheDirEnv is the environment variable for cache directory override.
	CacheDirEnv = "TURNKEY_CACHE_DIR"
)

// CacheEntry stores a prefetched hash.
type CacheEntry struct {
	// Hash is the Nix SRI hash (e.g., "sha256-...")
	Hash string `json:"hash"`
	// FetchedAt is when this entry was fetched (RFC3339 format)
	FetchedAt time.Time `json:"fetched_at"`
}

// cacheFile is the JSON file format.
type cacheFile struct {
	Version uint32                `json:"version"`
	Entries map[string]CacheEntry `json:"entries"`
}

// Cache provides thread-safe access to the prefetch cache.
type Cache struct {
	mu        sync.RWMutex
	path      string
	data      cacheFile
	dirty     bool
	autoSave  bool
}

// New creates a new cache with the default location.
func New() (*Cache, error) {
	dir, err := DefaultCacheDir()
	if err != nil {
		return nil, err
	}
	return WithDir(dir)
}

// WithDir creates a new cache with a custom directory.
func WithDir(cacheDir string) (*Cache, error) {
	cachePath := filepath.Join(cacheDir, CacheFilename)

	// Create cache directory if it doesn't exist
	if err := os.MkdirAll(cacheDir, 0755); err != nil {
		return nil, fmt.Errorf("failed to create cache directory: %w", err)
	}

	c := &Cache{
		path:     cachePath,
		autoSave: true,
		data: cacheFile{
			Version: CacheVersion,
			Entries: make(map[string]CacheEntry),
		},
	}

	// Load existing cache if it exists
	if _, err := os.Stat(cachePath); err == nil {
		if err := c.load(); err != nil {
			// Log warning but continue with empty cache
			fmt.Fprintf(os.Stderr, "prefetch-cache: %v, starting fresh\n", err)
		}
	}

	return c, nil
}

// DefaultCacheDir returns the default cache directory.
func DefaultCacheDir() (string, error) {
	// Check environment variable first
	if dir := os.Getenv(CacheDirEnv); dir != "" {
		return dir, nil
	}

	// Use platform-specific cache directory
	cacheDir, err := os.UserCacheDir()
	if err != nil {
		return "", fmt.Errorf("could not determine cache directory: %w", err)
	}

	return filepath.Join(cacheDir, "turnkey"), nil
}

// MakeKey builds a cache key for a package.
//
// Format: {source}/{name}/{version}
// Examples:
//   - github.com/spf13/cobra/v1.8.0
//   - proxy.golang.org/github.com/foo/bar/v1.0.0
func MakeKey(source, name, version string) string {
	return fmt.Sprintf("%s/%s/%s", source, name, version)
}

// Get retrieves a cached hash for a key.
// Returns the entry and true if found, nil and false otherwise.
func (c *Cache) Get(key string) (*CacheEntry, bool) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	entry, ok := c.data.Entries[key]
	if !ok {
		return nil, false
	}
	return &entry, true
}

// Set stores a hash in the cache.
func (c *Cache) Set(key, hash string) {
	c.mu.Lock()
	defer c.mu.Unlock()

	c.data.Entries[key] = CacheEntry{
		Hash:      hash,
		FetchedAt: time.Now().UTC(),
	}
	c.dirty = true
}

// Contains checks if a key exists in the cache.
func (c *Cache) Contains(key string) bool {
	c.mu.RLock()
	defer c.mu.RUnlock()

	_, ok := c.data.Entries[key]
	return ok
}

// Len returns the number of entries in the cache.
func (c *Cache) Len() int {
	c.mu.RLock()
	defer c.mu.RUnlock()

	return len(c.data.Entries)
}

// Path returns the path to the cache file.
func (c *Cache) Path() string {
	return c.path
}

// Save writes the cache to disk if modified.
func (c *Cache) Save() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	return c.saveLocked()
}

// saveLocked saves the cache (caller must hold lock).
func (c *Cache) saveLocked() error {
	if !c.dirty {
		return nil
	}

	data, err := json.MarshalIndent(c.data, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to serialize cache: %w", err)
	}

	if err := os.WriteFile(c.path, data, 0644); err != nil {
		return fmt.Errorf("failed to write cache file: %w", err)
	}

	c.dirty = false
	return nil
}

// load reads the cache from disk.
func (c *Cache) load() error {
	data, err := os.ReadFile(c.path)
	if err != nil {
		return fmt.Errorf("failed to read cache file: %w", err)
	}

	var cf cacheFile
	if err := json.Unmarshal(data, &cf); err != nil {
		return fmt.Errorf("failed to parse cache: %w", err)
	}

	// Check version compatibility
	if cf.Version != CacheVersion {
		return fmt.Errorf("cache version mismatch (found %d, expected %d)", cf.Version, CacheVersion)
	}

	if cf.Entries == nil {
		cf.Entries = make(map[string]CacheEntry)
	}

	c.data = cf
	return nil
}

// Close saves the cache if modified. Should be called when done with the cache.
func (c *Cache) Close() error {
	if c.autoSave {
		return c.Save()
	}
	return nil
}
