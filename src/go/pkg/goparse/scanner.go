package goparse

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// ScanDir scans a directory and returns all Go files (excluding testdata/).
func ScanDir(dir string) ([]*GoFile, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}

	var goFiles []*GoFile
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".go") {
			continue
		}
		// Skip files in testdata (though ReadDir is not recursive, we might be IN testdata)
		if strings.Contains(dir, "/testdata/") || strings.HasSuffix(dir, "/testdata") {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		gf, err := ParseFile(path)
		if err != nil {
			// Skip files that can't be parsed
			continue
		}
		goFiles = append(goFiles, gf)
	}

	return goFiles, nil
}

// ScanPackage scans a directory and aggregates its non-test Go files into
// a GoPackage, or returns nil if it has none. Which files a build includes
// is decided per build (GoPackage.Imports).
func ScanPackage(dir, importPath string) (*GoPackage, error) {
	files, err := ScanDir(dir)
	if err != nil {
		return nil, err
	}

	pkg := &GoPackage{Dir: dir, ImportPath: importPath}
	embedSet := make(map[string]bool)
	for _, f := range files {
		// Test files don't contribute to library deps
		if f.IsTest {
			continue
		}
		if pkg.Name == "" {
			pkg.Name = f.Package
		}
		pkg.Files = append(pkg.Files, f)
		for _, embed := range f.EmbedDirs {
			embedSet[embed] = true
		}
		if f.HasCgo {
			pkg.HasCgo = true
		}
	}
	if len(pkg.Files) == 0 {
		return nil, nil // No Go files found
	}

	for embed := range embedSet {
		pkg.EmbedDirs = append(pkg.EmbedDirs, embed)
	}
	sort.Strings(pkg.EmbedDirs)

	return pkg, nil
}

// TreeConstraintTags returns the tags the build constraints of the Go
// files under dir name (GoFile.ConstraintTags), sorted: those that can
// change which files a build of a package there includes. It covers the
// files go list's ./... does, test files included: testdata, vendor and
// hidden or _-prefixed directories are skipped. A file that doesn't parse
// names none.
func TreeConstraintTags(dir string) ([]string, error) {
	seen := make(map[string]bool)
	err := filepath.WalkDir(dir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		name := d.Name()
		if d.IsDir() {
			if path != dir && (name == "testdata" || name == "vendor" || strings.HasPrefix(name, ".") || strings.HasPrefix(name, "_")) {
				return filepath.SkipDir
			}
			return nil
		}
		if !strings.HasSuffix(name, ".go") {
			return nil
		}
		f, err := ParseFile(path)
		if err != nil {
			return nil
		}
		for _, tag := range f.ConstraintTags() {
			seen[tag] = true
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	tags := make([]string, 0, len(seen))
	for tag := range seen {
		tags = append(tags, tag)
	}
	sort.Strings(tags)
	return tags, nil
}
