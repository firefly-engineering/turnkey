// Package snapshot provides file content hashing for change detection.
//
// It is used by the tw wrapper to detect when native tools (go, cargo, uv)
// modify dependency files, triggering an automatic sync operation.
package snapshot

import (
	"crypto/sha256"
	"encoding/hex"
	"io"
	"os"
	"path/filepath"
)

// FileSnapshot represents the state of a file at a point in time.
type FileSnapshot struct {
	// Path is the file path (relative to the capture root).
	Path string

	// Hash is the SHA256 hash of the file contents (empty if file doesn't exist).
	Hash string

	// Exists indicates whether the file existed at capture time.
	Exists bool
}

// Capture takes snapshots of the given files relative to root.
// Non-existent files are captured with Exists=false and empty Hash.
// Returns an error only for I/O errors on existing files.
func Capture(root string, files []string) ([]FileSnapshot, error) {
	snapshots := make([]FileSnapshot, len(files))

	for i, file := range files {
		path := filepath.Join(root, file)
		snap := FileSnapshot{Path: file}

		hash, err := hashFile(path)
		if err != nil {
			if os.IsNotExist(err) {
				snap.Exists = false
				snap.Hash = ""
			} else {
				return nil, err
			}
		} else {
			snap.Exists = true
			snap.Hash = hash
		}

		snapshots[i] = snap
	}

	return snapshots, nil
}

// Changed compares before and after snapshots and returns true if any file changed.
// Files are considered changed if:
// - A file was created (didn't exist before, exists now)
// - A file was deleted (existed before, doesn't exist now)
// - A file's content changed (different hash)
func Changed(before, after []FileSnapshot) bool {
	if len(before) != len(after) {
		return true
	}

	beforeMap := make(map[string]FileSnapshot, len(before))
	for _, s := range before {
		beforeMap[s.Path] = s
	}

	for _, a := range after {
		b, ok := beforeMap[a.Path]
		if !ok {
			return true // New file in after
		}
		if a.Exists != b.Exists {
			return true // File created or deleted
		}
		if a.Hash != b.Hash {
			return true // Content changed
		}
	}

	return false
}

// hashFile computes the SHA256 hash of a file.
func hashFile(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer func() { _ = f.Close() }()

	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}

	return hex.EncodeToString(h.Sum(nil)), nil
}
