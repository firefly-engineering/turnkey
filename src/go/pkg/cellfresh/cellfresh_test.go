package cellfresh

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestReadSymlinkTargets(t *testing.T) {
	root := t.TempDir()

	// Create .turnkey directory
	tkDir := filepath.Join(root, ".turnkey")
	if err := os.Mkdir(tkDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a symlink pointing to /nix/store/ (should be captured)
	nixTarget := "/nix/store/abc123-godeps-cell"
	if err := os.Symlink(nixTarget, filepath.Join(tkDir, "godeps")); err != nil {
		t.Fatal(err)
	}

	// Create a symlink NOT pointing to /nix/store/ (should be ignored)
	if err := os.Symlink("/tmp/something", filepath.Join(tkDir, "localstuff")); err != nil {
		t.Fatal(err)
	}

	// Create a regular file (should be ignored)
	if err := os.WriteFile(filepath.Join(tkDir, "config.toml"), []byte("hi"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create .buckconfig symlink pointing to /nix/store/
	bcTarget := "/nix/store/def456-turnkey.buckconfig"
	if err := os.Symlink(bcTarget, filepath.Join(root, ".buckconfig")); err != nil {
		t.Fatal(err)
	}

	targets := readSymlinkTargets(root)

	if len(targets) != 2 {
		t.Fatalf("expected 2 targets, got %d: %v", len(targets), targets)
	}
	if targets[".buckconfig"] != bcTarget {
		t.Errorf(".buckconfig = %q, want %q", targets[".buckconfig"], bcTarget)
	}
	if targets[".turnkey/godeps"] != nixTarget {
		t.Errorf(".turnkey/godeps = %q, want %q", targets[".turnkey/godeps"], nixTarget)
	}
}

func TestReadSymlinkTargets_NoTurnkeyDir(t *testing.T) {
	root := t.TempDir()

	targets := readSymlinkTargets(root)
	if len(targets) != 0 {
		t.Fatalf("expected 0 targets, got %d: %v", len(targets), targets)
	}
}

func TestStateFileRoundTrip(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "state")

	original := map[string]string{
		".buckconfig":       "/nix/store/abc123-turnkey.buckconfig",
		".turnkey/godeps":   "/nix/store/def456-godeps-cell",
		".turnkey/rustdeps": "/nix/store/ghi789-rustdeps-cell",
	}

	if err := writeStateFile(path, original); err != nil {
		t.Fatalf("writeStateFile: %v", err)
	}

	loaded, err := readStateFile(path)
	if err != nil {
		t.Fatalf("readStateFile: %v", err)
	}

	if len(loaded) != len(original) {
		t.Fatalf("loaded %d entries, want %d", len(loaded), len(original))
	}
	for k, v := range original {
		if loaded[k] != v {
			t.Errorf("loaded[%q] = %q, want %q", k, loaded[k], v)
		}
	}
}

func TestReadStateFile_NotExist(t *testing.T) {
	_, err := readStateFile(filepath.Join(t.TempDir(), "nonexistent"))
	if !os.IsNotExist(err) {
		t.Errorf("expected os.IsNotExist, got %v", err)
	}
}

func TestDiffTargets(t *testing.T) {
	tests := []struct {
		name    string
		saved   map[string]string
		current map[string]string
		changed bool
	}{
		{
			name:    "identical",
			saved:   map[string]string{"a": "/nix/store/x", "b": "/nix/store/y"},
			current: map[string]string{"a": "/nix/store/x", "b": "/nix/store/y"},
			changed: false,
		},
		{
			name:    "target changed",
			saved:   map[string]string{"a": "/nix/store/old"},
			current: map[string]string{"a": "/nix/store/new"},
			changed: true,
		},
		{
			name:    "new entry",
			saved:   map[string]string{"a": "/nix/store/x"},
			current: map[string]string{"a": "/nix/store/x", "b": "/nix/store/y"},
			changed: true,
		},
		{
			name:    "removed entry",
			saved:   map[string]string{"a": "/nix/store/x", "b": "/nix/store/y"},
			current: map[string]string{"a": "/nix/store/x"},
			changed: true,
		},
		{
			name:    "both empty",
			saved:   map[string]string{},
			current: map[string]string{},
			changed: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			diff := diffTargets(tt.saved, tt.current)
			got := len(diff) > 0
			if got != tt.changed {
				t.Errorf("diffTargets changed = %v, want %v (diff: %v)", got, tt.changed, diff)
			}
		})
	}
}

func TestCheck_FirstRun(t *testing.T) {
	root := t.TempDir()
	tkDir := filepath.Join(root, ".turnkey")
	if err := os.Mkdir(tkDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a nix store symlink
	if err := os.Symlink("/nix/store/abc123-godeps", filepath.Join(tkDir, "godeps")); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	if err := Check(root, true, false, &buf); err != nil {
		t.Fatalf("Check: %v", err)
	}

	// State file should now exist
	stateFile := filepath.Join(tkDir, stateFileName)
	if _, err := os.Stat(stateFile); err != nil {
		t.Errorf("state file not created: %v", err)
	}

	// Output should mention first run
	if !bytes.Contains(buf.Bytes(), []byte("first run")) {
		t.Errorf("expected 'first run' in output, got: %s", buf.String())
	}
}

func TestCheck_Unchanged(t *testing.T) {
	root := t.TempDir()
	tkDir := filepath.Join(root, ".turnkey")
	if err := os.Mkdir(tkDir, 0o755); err != nil {
		t.Fatal(err)
	}

	target := "/nix/store/abc123-godeps"
	if err := os.Symlink(target, filepath.Join(tkDir, "godeps")); err != nil {
		t.Fatal(err)
	}

	// Write state matching current symlinks
	stateFile := filepath.Join(tkDir, stateFileName)
	if err := writeStateFile(stateFile, map[string]string{".turnkey/godeps": target}); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	if err := Check(root, true, false, &buf); err != nil {
		t.Fatalf("Check: %v", err)
	}

	if !bytes.Contains(buf.Bytes(), []byte("unchanged")) {
		t.Errorf("expected 'unchanged' in output, got: %s", buf.String())
	}
}

func TestCheck_Changed(t *testing.T) {
	root := t.TempDir()
	tkDir := filepath.Join(root, ".turnkey")
	if err := os.Mkdir(tkDir, 0o755); err != nil {
		t.Fatal(err)
	}

	newTarget := "/nix/store/def456-godeps"
	if err := os.Symlink(newTarget, filepath.Join(tkDir, "godeps")); err != nil {
		t.Fatal(err)
	}

	// Write state with OLD target
	stateFile := filepath.Join(tkDir, stateFileName)
	oldTarget := "/nix/store/abc123-godeps"
	if err := writeStateFile(stateFile, map[string]string{".turnkey/godeps": oldTarget}); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	// Check will try to run buck2 kill, which will fail — that's fine (best-effort).
	if err := Check(root, true, false, &buf); err != nil {
		t.Fatalf("Check: %v", err)
	}

	if !bytes.Contains(buf.Bytes(), []byte("restarting buck2 daemon")) {
		t.Errorf("expected restart message in output, got: %s", buf.String())
	}

	// State file should now have the new target
	saved, err := readStateFile(stateFile)
	if err != nil {
		t.Fatalf("readStateFile: %v", err)
	}
	if saved[".turnkey/godeps"] != newTarget {
		t.Errorf("state file target = %q, want %q", saved[".turnkey/godeps"], newTarget)
	}
}

func TestCheck_NoSymlinks(t *testing.T) {
	root := t.TempDir()

	var buf bytes.Buffer
	if err := Check(root, true, false, &buf); err != nil {
		t.Fatalf("Check: %v", err)
	}

	// Should skip silently when no nix store symlinks exist
	if !bytes.Contains(buf.Bytes(), []byte("no Nix store symlinks")) {
		t.Errorf("expected skip message, got: %s", buf.String())
	}
}
