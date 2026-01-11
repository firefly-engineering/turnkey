package staleness

import (
	"os"
	"path/filepath"
	"testing"
)

func TestNewCache(t *testing.T) {
	cache := NewCache()
	if cache == nil {
		t.Fatal("NewCache returned nil")
	}
	if cache.entries == nil {
		t.Fatal("cache.entries is nil")
	}
}

func TestCacheLoadSave(t *testing.T) {
	dir := t.TempDir()
	cachePath := filepath.Join(dir, "cache.json")

	// Create and populate cache
	cache := NewCache()
	cache.entries["test/BUCK"] = &CacheEntry{
		BuckFile:     "test/BUCK",
		SrcListHash:  "abc123",
		ImportHash:   "def456",
		BuckFileHash: "ghi789",
		WasStale:     false,
	}
	cache.dirty = true

	// Save
	if err := cache.Save(cachePath); err != nil {
		t.Fatal(err)
	}

	// Load
	loaded, err := LoadCache(cachePath)
	if err != nil {
		t.Fatal(err)
	}

	entry := loaded.Get("test/BUCK")
	if entry == nil {
		t.Fatal("expected entry for test/BUCK")
	}

	if entry.SrcListHash != "abc123" {
		t.Errorf("expected SrcListHash=abc123, got %s", entry.SrcListHash)
	}
}

func TestCacheLoadNonExistent(t *testing.T) {
	dir := t.TempDir()
	cachePath := filepath.Join(dir, "nonexistent.json")

	cache, err := LoadCache(cachePath)
	if err != nil {
		t.Fatal(err)
	}

	if len(cache.entries) != 0 {
		t.Errorf("expected empty cache, got %d entries", len(cache.entries))
	}
}

func TestCacheNeedsCheck(t *testing.T) {
	dir := t.TempDir()

	// Create a BUCK file
	buckFile := filepath.Join(dir, "BUCK")
	if err := os.WriteFile(buckFile, []byte("content"), 0644); err != nil {
		t.Fatal(err)
	}

	cache := NewCache()
	srcFiles := []string{"main.go", "helper.go"}
	imports := []string{"github.com/google/uuid"}

	// First check should always need checking
	if !cache.NeedsCheck(buckFile, srcFiles, imports) {
		t.Error("expected NeedsCheck=true for uncached entry")
	}

	// Update cache
	if err := cache.Update(buckFile, srcFiles, imports, false); err != nil {
		t.Fatal(err)
	}

	// Same inputs should not need checking
	if cache.NeedsCheck(buckFile, srcFiles, imports) {
		t.Error("expected NeedsCheck=false for cached entry with same inputs")
	}

	// Different src files should need checking
	if !cache.NeedsCheck(buckFile, []string{"main.go"}, imports) {
		t.Error("expected NeedsCheck=true when src files changed")
	}

	// Different imports should need checking
	if !cache.NeedsCheck(buckFile, srcFiles, []string{"github.com/other/pkg"}) {
		t.Error("expected NeedsCheck=true when imports changed")
	}

	// Modified BUCK file should need checking
	if err := os.WriteFile(buckFile, []byte("modified"), 0644); err != nil {
		t.Fatal(err)
	}
	if !cache.NeedsCheck(buckFile, srcFiles, imports) {
		t.Error("expected NeedsCheck=true when BUCK file changed")
	}
}

func TestCacheRemove(t *testing.T) {
	cache := NewCache()
	cache.entries["test/BUCK"] = &CacheEntry{BuckFile: "test/BUCK"}

	if cache.Get("test/BUCK") == nil {
		t.Fatal("expected entry before remove")
	}

	cache.Remove("test/BUCK")

	if cache.Get("test/BUCK") != nil {
		t.Error("expected nil after remove")
	}
}

func TestCacheClear(t *testing.T) {
	cache := NewCache()
	cache.entries["a/BUCK"] = &CacheEntry{BuckFile: "a/BUCK"}
	cache.entries["b/BUCK"] = &CacheEntry{BuckFile: "b/BUCK"}

	cache.Clear()

	if len(cache.entries) != 0 {
		t.Errorf("expected empty cache after clear, got %d entries", len(cache.entries))
	}
}

func TestHashStrings(t *testing.T) {
	// Same content, different order should produce same hash
	hash1 := hashStrings([]string{"a", "b", "c"})
	hash2 := hashStrings([]string{"c", "a", "b"})

	if hash1 != hash2 {
		t.Error("expected same hash for same content in different order")
	}

	// Different content should produce different hash
	hash3 := hashStrings([]string{"a", "b", "d"})
	if hash1 == hash3 {
		t.Error("expected different hash for different content")
	}
}

func TestCachedCheckGoPackage(t *testing.T) {
	dir := t.TempDir()

	// Create BUCK file
	buckFile := filepath.Join(dir, "BUCK")
	buckContent := `go_library(
    name = "lib",
    package_name = "github.com/example/lib",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Go file
	goFile := filepath.Join(dir, "main.go")
	goContent := `package lib

import (
	"fmt"
	"github.com/google/uuid"
)

func Do() { fmt.Println(uuid.New()) }
`
	if err := os.WriteFile(goFile, []byte(goContent), 0644); err != nil {
		t.Fatal(err)
	}

	cache := NewCache()
	cc := &CachedCheck{Cache: cache, BuckFile: buckFile}

	// First check should not be from cache
	result1, err := cc.CheckGoPackage()
	if err != nil {
		t.Fatal(err)
	}

	if result1.FromCache {
		t.Error("expected first check not from cache")
	}

	if result1.Stale {
		t.Errorf("expected not stale, got stale. SrcResult=%+v ImportResult=%+v",
			result1.SrcResult, result1.ImportResult)
	}

	// Second check should be from cache
	result2, err := cc.CheckGoPackage()
	if err != nil {
		t.Fatal(err)
	}

	if !result2.FromCache {
		t.Error("expected second check from cache")
	}

	if result2.Stale {
		t.Error("expected not stale on cached check")
	}
}

func TestFindBuckFiles(t *testing.T) {
	dir := t.TempDir()

	// Create directory structure
	dirs := []string{
		"pkg/a",
		"pkg/b",
		"pkg/c",
		".hidden",
	}

	for _, d := range dirs {
		if err := os.MkdirAll(filepath.Join(dir, d), 0755); err != nil {
			t.Fatal(err)
		}
	}

	// Create BUCK files
	buckPaths := []string{
		"BUCK",
		"pkg/a/BUCK",
		"pkg/b/BUCK",
		".hidden/BUCK", // Should be skipped
	}

	for _, p := range buckPaths {
		path := filepath.Join(dir, p)
		if err := os.WriteFile(path, []byte(""), 0644); err != nil {
			t.Fatal(err)
		}
	}

	found, err := FindBuckFiles(dir)
	if err != nil {
		t.Fatal(err)
	}

	// Should find 3 (excluding .hidden)
	if len(found) != 3 {
		t.Errorf("expected 3 BUCK files, got %d: %v", len(found), found)
	}
}

func TestDefaultCachePath(t *testing.T) {
	// Test with custom cache dir
	os.Setenv("TURNKEY_CACHE_DIR", "/custom/cache")
	defer os.Unsetenv("TURNKEY_CACHE_DIR")

	path := DefaultCachePath()
	if path != "/custom/cache/staleness-cache.json" {
		t.Errorf("expected /custom/cache/staleness-cache.json, got %s", path)
	}

	// Test with default
	os.Unsetenv("TURNKEY_CACHE_DIR")
	path = DefaultCachePath()
	if !filepath.IsAbs(path) {
		t.Errorf("expected absolute path, got %s", path)
	}
	if filepath.Base(path) != "staleness-cache.json" {
		t.Errorf("expected staleness-cache.json, got %s", filepath.Base(path))
	}
}
