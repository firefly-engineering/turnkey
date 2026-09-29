// Package materialize keeps a deps cell's directory in line with its cell
// index (ADR 0004).
//
// A deps cell (.turnkey/<cell>) is a real directory. It holds a store link
// per package, _store/<store basename>, pointing at the package's store
// path, and an alias package per package path (vendor/anyhow@1.0.100) and
// per unversioned name (vendor/anyhow): a real rules.star whose alias()
// targets forward to a store link's package. A store link is named after
// its target, so it is only ever created or deleted, never retargeted:
// everything that changes on a dependency bump is a real file, which
// buck2's file watcher sees.
//
// The shell builds the index with Nix; the materializer never runs Nix to
// build anything and never reads Starlark: target names come from the index.
package materialize

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"syscall"
)

const (
	storeDir   = "_store"
	markerName = ".deps-file-sha256"
	buildFile  = "rules.star"
	gcrootsDir = "gcroots"
)

// Index is a cell index, as nix/lib/deps-cell's mkCellIndex writes it
type Index struct {
	Cell           string             `json:"cell"`
	DepsFileSHA256 string             `json:"deps_file_sha256"`
	Buckconfig     string             `json:"buckconfig"`
	Packages       map[string]Package `json:"packages"`
	// Each alias package's path, and the package it forwards to
	Aliases map[string]string `json:"aliases"`
}

// Package is one package of the cell: its store path, and its targets
type Package struct {
	Store   string   `json:"store"`
	Targets []string `json:"targets"`
}

// ReadIndex reads a cell index
func ReadIndex(path string) (*Index, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var index Index
	if err := json.Unmarshal(data, &index); err != nil {
		return nil, fmt.Errorf("parsing cell index %s: %w", path, err)
	}
	if index.Cell == "" {
		return nil, fmt.Errorf("cell index %s names no cell", path)
	}
	return &index, nil
}

// Options says which cell to materialize, from which index
type Options struct {
	// Root is the project root; the cell is .turnkey/<index's cell> under it
	Root string
	// IndexPath is the cell index's store path
	IndexPath string
	// AddRoot registers link as a GC root for storePath (the materializer
	// never runs Nix itself; tk passes one that does). Nil skips rooting.
	AddRoot func(link, storePath string) error
}

// Result counts what a materialization changed. A no-op changes nothing.
type Result struct {
	SwitchedOver     bool
	StoreLinksAdded  int
	StoreLinksGone   int
	PackagesWritten  int
	PackagesGone     int
	BuckconfigWrites int
	Rooted           bool
}

// Changed reports whether the materialization changed anything
func (r Result) Changed() bool {
	return r.SwitchedOver || r.StoreLinksAdded+r.StoreLinksGone+r.PackagesWritten+r.PackagesGone+r.BuckconfigWrites > 0 || r.Rooted
}

// CellDir is where a cell lives in a project
func CellDir(root, cell string) string {
	return filepath.Join(root, ".turnkey", cell)
}

// Materialize brings the cell's directory in line with the index, under a
// lock. It is idempotent: re-running after a crash repairs a partial
// materialization.
func Materialize(opts Options) (Result, error) {
	var result Result
	index, err := ReadIndex(opts.IndexPath)
	if err != nil {
		return result, err
	}
	turnkey := filepath.Join(opts.Root, ".turnkey")
	if err := os.MkdirAll(turnkey, 0o755); err != nil {
		return result, err
	}
	unlock, err := lock(filepath.Join(turnkey, index.Cell+".lock"))
	if err != nil {
		return result, err
	}
	defer unlock()

	cell := CellDir(opts.Root, index.Cell)

	// Switching over: the cell was a symlink to one store path
	if info, err := os.Lstat(cell); err == nil && info.Mode()&fs.ModeSymlink != 0 {
		if err := os.Remove(cell); err != nil {
			return result, fmt.Errorf("replacing the %s cell symlink: %w", index.Cell, err)
		}
		result.SwitchedOver = true
	}
	for _, dir := range []string{cell, filepath.Join(cell, storeDir), filepath.Join(cell, "vendor")} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return result, err
		}
	}

	written, err := writeIfChanged(filepath.Join(cell, ".buckconfig"), []byte(index.Buckconfig))
	if err != nil {
		return result, err
	}
	if written {
		result.BuckconfigWrites++
	}

	// 1. Store links: added, never retargeted
	links := map[string]string{}
	for _, pkg := range index.Packages {
		links[filepath.Base(pkg.Store)] = pkg.Store
	}
	for name, store := range links {
		added, err := ensureStoreLink(filepath.Join(cell, storeDir, name), store)
		if err != nil {
			return result, err
		}
		if added {
			result.StoreLinksAdded++
		}
	}

	// 2. Alias packages: rewritten atomically when they change
	packages := map[string][]byte{}
	for path, pkg := range index.Packages {
		packages[path] = aliasFile(pkg)
	}
	for path, target := range index.Aliases {
		pkg, ok := index.Packages[target]
		if !ok {
			return result, fmt.Errorf("cell index: alias %s forwards to %s, which it doesn't list", path, target)
		}
		packages[path] = aliasFile(pkg)
	}
	for path, content := range packages {
		dir := filepath.Join(cell, path)
		if err := replaceSymlinkWithDir(dir); err != nil {
			return result, err
		}
		written, err := writeIfChanged(filepath.Join(dir, buildFile), content)
		if err != nil {
			return result, err
		}
		if written {
			result.PackagesWritten++
		}
	}

	// 3. What the index no longer names
	gone, err := removeStaleStoreLinks(filepath.Join(cell, storeDir), links)
	if err != nil {
		return result, err
	}
	result.StoreLinksGone = gone
	gone, err = removeStalePackages(cell, packages)
	if err != nil {
		return result, err
	}
	result.PackagesGone = gone

	// 4. The GC root, for the current index
	if opts.AddRoot != nil {
		link := filepath.Join(turnkey, gcrootsDir, index.Cell)
		if current, err := os.Readlink(link); err != nil || current != opts.IndexPath {
			if err := os.MkdirAll(filepath.Dir(link), 0o755); err != nil {
				return result, err
			}
			if err := opts.AddRoot(link, opts.IndexPath); err != nil {
				return result, fmt.Errorf("rooting the %s cell index: %w", index.Cell, err)
			}
			result.Rooted = true
		}
	}

	// 5. The deps file's hash, which tk compares with the file on disk
	if _, err := writeIfChanged(filepath.Join(cell, markerName), []byte(index.DepsFileSHA256+"\n")); err != nil {
		return result, err
	}
	return result, nil
}

// Stale reports whether a materialized cell was built from another version
// of depsFile than the one on disk. A cell that isn't materialized (no
// marker) is not stale: there is nothing to compare.
func Stale(root, cell, depsFile string) (bool, error) {
	marker, err := os.ReadFile(filepath.Join(CellDir(root, cell), markerName))
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	content, err := os.ReadFile(filepath.Join(root, depsFile))
	if errors.Is(err, fs.ErrNotExist) {
		return true, nil
	}
	if err != nil {
		return false, err
	}
	sum := sha256.Sum256(content)
	return strings.TrimSpace(string(marker)) != hex.EncodeToString(sum[:]), nil
}

// aliasFile is an alias package's rules.star: each target forwards to the
// same-named target of the package's store link
func aliasFile(pkg Package) []byte {
	var b bytes.Buffer
	b.WriteString("# Generated by tk materialize from the cell index. Do not edit.\n")
	targets := slices.Clone(pkg.Targets)
	slices.Sort(targets)
	for _, t := range targets {
		fmt.Fprintf(&b, "alias(name = %q, actual = \"//%s/%s:%s\", visibility = [\"PUBLIC\"])\n",
			t, storeDir, filepath.Base(pkg.Store), t)
	}
	return b.Bytes()
}

// ensureStoreLink creates the store link if it is missing. One that exists
// must already point at its name: nothing may retarget a store link.
func ensureStoreLink(link, store string) (bool, error) {
	info, err := os.Lstat(link)
	if errors.Is(err, fs.ErrNotExist) {
		return true, os.Symlink(store, link)
	}
	if err != nil {
		return false, err
	}
	if info.Mode()&fs.ModeSymlink == 0 {
		return false, fmt.Errorf("store link %s is not a symlink: remove it and re-run", link)
	}
	target, err := os.Readlink(link)
	if err != nil {
		return false, err
	}
	if target != store {
		return false, fmt.Errorf("store link %s points at %s, not %s: store links are never retargeted, remove it and re-run", link, target, store)
	}
	return false, nil
}

// replaceSymlinkWithDir makes dir a real directory: the old cell layout had
// symlinks where packages now are
func replaceSymlinkWithDir(dir string) error {
	if info, err := os.Lstat(dir); err == nil && info.Mode()&fs.ModeSymlink != 0 {
		if err := os.Remove(dir); err != nil {
			return err
		}
	}
	return os.MkdirAll(dir, 0o755)
}

// removeStaleStoreLinks removes the store links the index no longer names
func removeStaleStoreLinks(dir string, wanted map[string]string) (int, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return 0, err
	}
	gone := 0
	for _, e := range entries {
		if _, ok := wanted[e.Name()]; ok {
			continue
		}
		if err := os.RemoveAll(filepath.Join(dir, e.Name())); err != nil {
			return gone, err
		}
		gone++
	}
	return gone, nil
}

// removeStalePackages removes, under vendor/, everything but the wanted
// packages' rules.star files, and the directories left empty
func removeStalePackages(cell string, wanted map[string][]byte) (int, error) {
	keep := map[string]bool{}
	for path := range wanted {
		keep[filepath.Join(cell, path, buildFile)] = true
	}
	vendor := filepath.Join(cell, "vendor")
	gone := 0
	var dirs []string
	err := filepath.WalkDir(vendor, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			dirs = append(dirs, path)
			return nil
		}
		if keep[path] {
			return nil
		}
		if d.Name() == buildFile {
			gone++
		}
		return os.Remove(path)
	})
	if err != nil {
		return gone, err
	}
	// Deepest first, so a parent is empty once its children are gone
	slices.SortFunc(dirs, func(a, b string) int { return len(b) - len(a) })
	for _, dir := range dirs {
		if dir == vendor {
			continue
		}
		entries, err := os.ReadDir(dir)
		if err != nil {
			return gone, err
		}
		if len(entries) == 0 {
			if err := os.Remove(dir); err != nil {
				return gone, err
			}
		}
	}
	return gone, nil
}

// writeIfChanged writes content atomically (temporary file, then rename),
// and only when the file doesn't already hold it: an unchanged file keeps
// its mtime, so buck2 sees nothing
func writeIfChanged(path string, content []byte) (bool, error) {
	if current, err := os.ReadFile(path); err == nil && bytes.Equal(current, content) {
		return false, nil
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".tmp-"+filepath.Base(path)+"-*")
	if err != nil {
		return false, err
	}
	if _, err := tmp.Write(content); err != nil {
		_ = tmp.Close()
		_ = os.Remove(tmp.Name())
		return false, err
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmp.Name())
		return false, err
	}
	if err := os.Chmod(tmp.Name(), 0o644); err != nil {
		_ = os.Remove(tmp.Name())
		return false, err
	}
	return true, os.Rename(tmp.Name(), path)
}

// lock takes an exclusive lock on path, waiting for another materialization
// of the same cell to finish
func lock(path string) (func(), error) {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o644)
	if err != nil {
		return nil, err
	}
	if err := syscall.Flock(int(f.Fd()), syscall.LOCK_EX); err != nil {
		_ = f.Close()
		return nil, fmt.Errorf("locking %s: %w", path, err)
	}
	return func() {
		_ = syscall.Flock(int(f.Fd()), syscall.LOCK_UN)
		_ = f.Close()
	}, nil
}
