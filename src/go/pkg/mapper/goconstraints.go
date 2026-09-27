package mapper

import (
	"bufio"
	"go/build/constraint"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// constraintTags returns the tags the build constraints (//go:build and
// // +build lines) of the Go files under pkgDir name, sorted: those that
// can change which files a build of the package includes. go list's
// ./... covers the same files: testdata, vendor and hidden or _-prefixed
// directories are skipped.
func constraintTags(pkgDir string) ([]string, error) {
	seen := make(map[string]bool)
	err := filepath.WalkDir(pkgDir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		name := d.Name()
		if d.IsDir() {
			if path != pkgDir && (name == "testdata" || name == "vendor" || strings.HasPrefix(name, ".") || strings.HasPrefix(name, "_")) {
				return filepath.SkipDir
			}
			return nil
		}
		if !strings.HasSuffix(name, ".go") {
			return nil
		}
		return fileConstraintTags(path, seen)
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

// fileConstraintTags adds the tags of a Go file's build constraints, which
// come before its package clause, to seen.
func fileConstraintTags(path string, seen map[string]bool) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if strings.HasPrefix(line, "package ") {
			break
		}
		if !constraint.IsGoBuild(line) && !constraint.IsPlusBuild(line) {
			continue
		}
		expr, err := constraint.Parse(line)
		if err != nil {
			continue
		}
		collectTags(expr, seen)
	}
	return scanner.Err()
}

func collectTags(expr constraint.Expr, seen map[string]bool) {
	switch e := expr.(type) {
	case *constraint.TagExpr:
		seen[e.Tag] = true
	case *constraint.NotExpr:
		collectTags(e.X, seen)
	case *constraint.AndExpr:
		collectTags(e.X, seen)
		collectTags(e.Y, seen)
	case *constraint.OrExpr:
		collectTags(e.X, seen)
		collectTags(e.Y, seen)
	}
}
