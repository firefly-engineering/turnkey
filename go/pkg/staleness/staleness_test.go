package staleness

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestIsStale_TargetMissing(t *testing.T) {
	// Create a temporary source file
	dir := t.TempDir()
	source := filepath.Join(dir, "source.txt")
	if err := os.WriteFile(source, []byte("content"), 0644); err != nil {
		t.Fatal(err)
	}

	target := filepath.Join(dir, "target.txt") // Does not exist

	stale, err := IsStale([]string{source}, target)
	if err != nil {
		t.Fatal(err)
	}

	if !stale {
		t.Error("expected stale=true when target is missing")
	}
}

func TestIsStale_TargetNewer(t *testing.T) {
	dir := t.TempDir()

	// Create source file
	source := filepath.Join(dir, "source.txt")
	if err := os.WriteFile(source, []byte("content"), 0644); err != nil {
		t.Fatal(err)
	}

	// Wait a bit to ensure different timestamps
	time.Sleep(10 * time.Millisecond)

	// Create target file (newer)
	target := filepath.Join(dir, "target.txt")
	if err := os.WriteFile(target, []byte("generated"), 0644); err != nil {
		t.Fatal(err)
	}

	stale, err := IsStale([]string{source}, target)
	if err != nil {
		t.Fatal(err)
	}

	if stale {
		t.Error("expected stale=false when target is newer than source")
	}
}

func TestIsStale_SourceNewer(t *testing.T) {
	dir := t.TempDir()

	// Create target file first
	target := filepath.Join(dir, "target.txt")
	if err := os.WriteFile(target, []byte("generated"), 0644); err != nil {
		t.Fatal(err)
	}

	// Wait a bit to ensure different timestamps
	time.Sleep(10 * time.Millisecond)

	// Create source file (newer)
	source := filepath.Join(dir, "source.txt")
	if err := os.WriteFile(source, []byte("content"), 0644); err != nil {
		t.Fatal(err)
	}

	stale, err := IsStale([]string{source}, target)
	if err != nil {
		t.Fatal(err)
	}

	if !stale {
		t.Error("expected stale=true when source is newer than target")
	}
}

func TestIsStale_MultipleSources(t *testing.T) {
	dir := t.TempDir()

	// Create target file first
	target := filepath.Join(dir, "target.txt")
	if err := os.WriteFile(target, []byte("generated"), 0644); err != nil {
		t.Fatal(err)
	}

	time.Sleep(10 * time.Millisecond)

	// Create first source (newer than target)
	source1 := filepath.Join(dir, "source1.txt")
	if err := os.WriteFile(source1, []byte("content1"), 0644); err != nil {
		t.Fatal(err)
	}

	// Create second source (also newer)
	source2 := filepath.Join(dir, "source2.txt")
	if err := os.WriteFile(source2, []byte("content2"), 0644); err != nil {
		t.Fatal(err)
	}

	stale, err := IsStale([]string{source1, source2}, target)
	if err != nil {
		t.Fatal(err)
	}

	if !stale {
		t.Error("expected stale=true when any source is newer than target")
	}
}

func TestIsStale_GlobPattern(t *testing.T) {
	dir := t.TempDir()

	// Create target file first
	target := filepath.Join(dir, "target.txt")
	if err := os.WriteFile(target, []byte("generated"), 0644); err != nil {
		t.Fatal(err)
	}

	time.Sleep(10 * time.Millisecond)

	// Create some Go files
	for _, name := range []string{"a.go", "b.go", "c.go"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("package main"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	pattern := filepath.Join(dir, "*.go")
	stale, err := IsStale([]string{pattern}, target)
	if err != nil {
		t.Fatal(err)
	}

	if !stale {
		t.Error("expected stale=true when glob-matched sources are newer")
	}
}

func TestCheck_DetailedResult(t *testing.T) {
	dir := t.TempDir()

	// Create target file first
	target := filepath.Join(dir, "target.txt")
	if err := os.WriteFile(target, []byte("generated"), 0644); err != nil {
		t.Fatal(err)
	}

	time.Sleep(10 * time.Millisecond)

	// Create source files
	source1 := filepath.Join(dir, "source1.txt")
	if err := os.WriteFile(source1, []byte("content1"), 0644); err != nil {
		t.Fatal(err)
	}

	time.Sleep(10 * time.Millisecond)

	source2 := filepath.Join(dir, "source2.txt")
	if err := os.WriteFile(source2, []byte("content2"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := Check([]string{source1, source2}, target)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale=true")
	}

	if result.TargetMissing {
		t.Error("target should not be missing")
	}

	if len(result.Sources) != 2 {
		t.Errorf("expected 2 sources, got %d", len(result.Sources))
	}

	if result.NewestSource == nil {
		t.Error("expected NewestSource to be set")
	} else if result.NewestSource.Path != source2 {
		t.Errorf("expected newest source to be %s, got %s", source2, result.NewestSource.Path)
	}
}

func TestExpandDoubleStar(t *testing.T) {
	dir := t.TempDir()

	// Create nested structure
	subdir := filepath.Join(dir, "sub")
	if err := os.MkdirAll(subdir, 0755); err != nil {
		t.Fatal(err)
	}

	// Create files at different levels
	files := []string{
		filepath.Join(dir, "root.go"),
		filepath.Join(subdir, "nested.go"),
	}

	for _, f := range files {
		if err := os.WriteFile(f, []byte("package main"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	// Also create a non-Go file that shouldn't match
	if err := os.WriteFile(filepath.Join(dir, "readme.md"), []byte("# README"), 0644); err != nil {
		t.Fatal(err)
	}

	pattern := filepath.Join(dir, "**", "*.go")
	matches, err := expandDoubleStar(pattern)
	if err != nil {
		t.Fatal(err)
	}

	if len(matches) != 2 {
		t.Errorf("expected 2 matches, got %d: %v", len(matches), matches)
	}
}

func TestIsStale_MissingSource(t *testing.T) {
	dir := t.TempDir()

	// Create target file
	target := filepath.Join(dir, "target.txt")
	if err := os.WriteFile(target, []byte("generated"), 0644); err != nil {
		t.Fatal(err)
	}

	// Non-existent source
	source := filepath.Join(dir, "nonexistent.txt")

	result, err := Check([]string{source}, target)
	if err != nil {
		t.Fatal(err)
	}

	if len(result.Sources) != 1 {
		t.Errorf("expected 1 source, got %d", len(result.Sources))
	}

	if !result.Sources[0].Missing {
		t.Error("expected source to be marked as missing")
	}
}
