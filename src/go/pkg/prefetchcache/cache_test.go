package prefetchcache

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestCacheRoundtrip(t *testing.T) {
	dir := t.TempDir()
	cache, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to create cache: %v", err)
	}

	// Add some entries
	key := MakeKey("github.com", "spf13/cobra", "v1.8.0")
	cache.Set(key, "sha256-abc123")

	if !cache.Contains(key) {
		t.Error("expected cache to contain key")
	}

	entry, ok := cache.Get(key)
	if !ok {
		t.Fatal("expected to get entry")
	}
	if entry.Hash != "sha256-abc123" {
		t.Errorf("expected hash sha256-abc123, got %s", entry.Hash)
	}

	// Save and reload
	if err := cache.Save(); err != nil {
		t.Fatalf("failed to save cache: %v", err)
	}

	cache2, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to reload cache: %v", err)
	}

	if !cache2.Contains(key) {
		t.Error("expected reloaded cache to contain key")
	}

	entry2, ok := cache2.Get(key)
	if !ok {
		t.Fatal("expected to get entry from reloaded cache")
	}
	if entry2.Hash != "sha256-abc123" {
		t.Errorf("expected hash sha256-abc123, got %s", entry2.Hash)
	}
}

func TestMakeKey(t *testing.T) {
	tests := []struct {
		source   string
		name     string
		version  string
		expected string
	}{
		{"github.com", "spf13/cobra", "v1.8.0", "github.com/spf13/cobra/v1.8.0"},
		{"proxy.golang.org", "github.com/foo/bar", "v1.0.0", "proxy.golang.org/github.com/foo/bar/v1.0.0"},
		{"crates.io", "serde", "1.0.228", "crates.io/serde/1.0.228"},
	}

	for _, tc := range tests {
		result := MakeKey(tc.source, tc.name, tc.version)
		if result != tc.expected {
			t.Errorf("MakeKey(%q, %q, %q) = %q, want %q",
				tc.source, tc.name, tc.version, result, tc.expected)
		}
	}
}

func TestCacheVersionMismatch(t *testing.T) {
	dir := t.TempDir()
	cachePath := filepath.Join(dir, CacheFilename)

	// Write a cache with wrong version
	oldCache := map[string]interface{}{
		"version": 999,
		"entries": map[string]interface{}{
			"test/key": map[string]interface{}{
				"hash":       "sha256-old",
				"fetched_at": "2025-01-01T00:00:00Z",
			},
		},
	}
	data, _ := json.Marshal(oldCache)
	os.WriteFile(cachePath, data, 0644)

	// Load should start fresh due to version mismatch
	cache, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to create cache: %v", err)
	}

	// Old entry should not be present
	if cache.Contains("test/key") {
		t.Error("expected cache to be empty after version mismatch")
	}
}

func TestCacheEmptyFile(t *testing.T) {
	dir := t.TempDir()
	cachePath := filepath.Join(dir, CacheFilename)

	// Write empty file
	os.WriteFile(cachePath, []byte{}, 0644)

	// Should handle gracefully
	cache, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to create cache: %v", err)
	}

	if cache.Len() != 0 {
		t.Errorf("expected empty cache, got %d entries", cache.Len())
	}
}

func TestCacheCorruptedFile(t *testing.T) {
	dir := t.TempDir()
	cachePath := filepath.Join(dir, CacheFilename)

	// Write corrupted JSON
	os.WriteFile(cachePath, []byte("not valid json{{{"), 0644)

	// Should handle gracefully and start fresh
	cache, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to create cache: %v", err)
	}

	if cache.Len() != 0 {
		t.Errorf("expected empty cache after corruption, got %d entries", cache.Len())
	}
}

func TestCacheNoSaveIfNotDirty(t *testing.T) {
	dir := t.TempDir()
	cache, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to create cache: %v", err)
	}

	cachePath := filepath.Join(dir, CacheFilename)

	// Save without changes - should not create file
	cache.Save()

	if _, err := os.Stat(cachePath); err == nil {
		t.Error("expected no cache file when nothing was written")
	}

	// Now add something
	cache.Set("test/key", "sha256-test")
	cache.Save()

	if _, err := os.Stat(cachePath); err != nil {
		t.Error("expected cache file after write")
	}
}

func TestRustCompatibility(t *testing.T) {
	// Test that our format is compatible with the Rust prefetch-cache
	dir := t.TempDir()
	cachePath := filepath.Join(dir, CacheFilename)

	// Write a cache in the exact Rust format
	rustCache := `{
  "version": 1,
  "entries": {
    "crates.io/serde/1.0.228": {
      "hash": "sha256-fromrust",
      "fetched_at": "2025-01-30T12:34:56.789Z"
    },
    "github.com/spf13/cobra/v1.8.0": {
      "hash": "sha256-gomodule",
      "fetched_at": "2025-01-30T12:35:00Z"
    }
  }
}`
	os.WriteFile(cachePath, []byte(rustCache), 0644)

	// Load with Go implementation
	cache, err := WithDir(dir)
	if err != nil {
		t.Fatalf("failed to load Rust-formatted cache: %v", err)
	}

	// Verify entries
	if !cache.Contains("crates.io/serde/1.0.228") {
		t.Error("missing crates.io entry")
	}
	if !cache.Contains("github.com/spf13/cobra/v1.8.0") {
		t.Error("missing github.com entry")
	}

	entry, _ := cache.Get("crates.io/serde/1.0.228")
	if entry.Hash != "sha256-fromrust" {
		t.Errorf("expected sha256-fromrust, got %s", entry.Hash)
	}

	// Add a new entry and save
	cache.Set("pypi.org/requests/2.31.0", "sha256-frompython")
	cache.Save()

	// Reload and verify all entries
	cache2, _ := WithDir(dir)
	if cache2.Len() != 3 {
		t.Errorf("expected 3 entries, got %d", cache2.Len())
	}
}
