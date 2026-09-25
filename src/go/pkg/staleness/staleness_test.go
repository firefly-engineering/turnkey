package staleness

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

// base is a fixed reference time. Tests order file modification times
// explicitly relative to it instead of sleeping, so the outcome never
// depends on filesystem timestamp resolution or scheduling.
var base = time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)

// writeAt writes content to path and sets its modification time to mtime.
func writeAt(t *testing.T, path, content string, mtime time.Time) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(path, mtime, mtime); err != nil {
		t.Fatal(err)
	}
}

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

	source := filepath.Join(dir, "source.txt")
	writeAt(t, source, "content", base)

	// Target is newer than the source
	target := filepath.Join(dir, "target.txt")
	writeAt(t, target, "generated", base.Add(time.Second))

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

	target := filepath.Join(dir, "target.txt")
	writeAt(t, target, "generated", base)

	// Source is newer than the target
	source := filepath.Join(dir, "source.txt")
	writeAt(t, source, "content", base.Add(time.Second))

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

	target := filepath.Join(dir, "target.txt")
	writeAt(t, target, "generated", base)

	// Both sources are newer than the target
	source1 := filepath.Join(dir, "source1.txt")
	writeAt(t, source1, "content1", base.Add(time.Second))
	source2 := filepath.Join(dir, "source2.txt")
	writeAt(t, source2, "content2", base.Add(time.Second))

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

	target := filepath.Join(dir, "target.txt")
	writeAt(t, target, "generated", base)

	// Go files newer than the target
	for _, name := range []string{"a.go", "b.go", "c.go"} {
		writeAt(t, filepath.Join(dir, name), "package main", base.Add(time.Second))
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

	target := filepath.Join(dir, "target.txt")
	writeAt(t, target, "generated", base)

	// Two sources newer than the target; source2 is the newest
	source1 := filepath.Join(dir, "source1.txt")
	writeAt(t, source1, "content1", base.Add(time.Second))
	source2 := filepath.Join(dir, "source2.txt")
	writeAt(t, source2, "content2", base.Add(2*time.Second))

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
