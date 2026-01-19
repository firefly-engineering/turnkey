package staleness

import (
	"os"
	"path/filepath"
	"strings"
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
	cache.entries["test/rules.star"] = &CacheEntry{
		BuckFile:     "test/rules.star",
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

	entry := loaded.Get("test/rules.star")
	if entry == nil {
		t.Fatal("expected entry for test/rules.star")
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

	// Create a rules.star file
	buckFile := filepath.Join(dir, "rules.star")
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

	// Modified rules.star file should need checking
	if err := os.WriteFile(buckFile, []byte("modified"), 0644); err != nil {
		t.Fatal(err)
	}
	if !cache.NeedsCheck(buckFile, srcFiles, imports) {
		t.Error("expected NeedsCheck=true when rules.star file changed")
	}
}

func TestCacheRemove(t *testing.T) {
	cache := NewCache()
	cache.entries["test/rules.star"] = &CacheEntry{BuckFile: "test/rules.star"}

	if cache.Get("test/rules.star") == nil {
		t.Fatal("expected entry before remove")
	}

	cache.Remove("test/rules.star")

	if cache.Get("test/rules.star") != nil {
		t.Error("expected nil after remove")
	}
}

func TestCacheClear(t *testing.T) {
	cache := NewCache()
	cache.entries["a/rules.star"] = &CacheEntry{BuckFile: "a/rules.star"}
	cache.entries["b/rules.star"] = &CacheEntry{BuckFile: "b/rules.star"}

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

	// Create rules.star file
	buckFile := filepath.Join(dir, "rules.star")
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

	// Create rules.star files
	buckPaths := []string{
		"rules.star",
		"pkg/a/rules.star",
		"pkg/b/rules.star",
		".hidden/rules.star", // Should be skipped
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
		t.Errorf("expected 3 rules.star files, got %d: %v", len(found), found)
	}
}

func TestDefaultCachePath(t *testing.T) {
	// Save original env vars
	origTurnkey := os.Getenv("TURNKEY_CACHE_DIR")
	origXDG := os.Getenv("XDG_CACHE_HOME")
	defer func() {
		os.Setenv("TURNKEY_CACHE_DIR", origTurnkey)
		os.Setenv("XDG_CACHE_HOME", origXDG)
	}()

	// Test with TURNKEY_CACHE_DIR (highest priority)
	os.Setenv("TURNKEY_CACHE_DIR", "/custom/cache")
	os.Setenv("XDG_CACHE_HOME", "/xdg/cache")
	path := DefaultCachePath()
	if path != "/custom/cache/staleness-cache.json" {
		t.Errorf("expected /custom/cache/staleness-cache.json, got %s", path)
	}

	// Test with XDG_CACHE_HOME (when TURNKEY_CACHE_DIR not set)
	os.Unsetenv("TURNKEY_CACHE_DIR")
	path = DefaultCachePath()
	if path != "/xdg/cache/turnkey/staleness-cache.json" {
		t.Errorf("expected /xdg/cache/turnkey/staleness-cache.json, got %s", path)
	}

	// Test with default (~/.cache when XDG_CACHE_HOME not set)
	os.Unsetenv("XDG_CACHE_HOME")
	path = DefaultCachePath()
	if !filepath.IsAbs(path) {
		t.Errorf("expected absolute path, got %s", path)
	}
	if filepath.Base(path) != "staleness-cache.json" {
		t.Errorf("expected staleness-cache.json, got %s", filepath.Base(path))
	}
	// Should contain .cache/turnkey
	if !strings.Contains(path, ".cache/turnkey") && !strings.Contains(path, ".cache\\turnkey") {
		t.Errorf("expected path to contain .cache/turnkey, got %s", path)
	}
}
