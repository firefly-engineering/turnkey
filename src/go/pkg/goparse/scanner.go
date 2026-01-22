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

// ScanPackage scans a directory and aggregates files into a GoPackage.
// Evaluates constraints for each platform in DefaultPlatforms if platforms is nil.
func ScanPackage(dir, importPath string, platforms []Platform) (*GoPackage, error) {
	if platforms == nil {
		platforms = DefaultPlatforms
	}

	files, err := ScanDir(dir)
	if err != nil {
		return nil, err
	}

	if len(files) == 0 {
		return nil, nil // No Go files found
	}

	pkg := &GoPackage{
		Dir:        dir,
		ImportPath: importPath,
		Name:       files[0].Package, // Assume all files in dir have same package name
		GoFiles:    make(map[Platform][]string),
		Imports:    make(map[Platform][]string),
	}

	embedSet := make(map[string]bool)

	for _, p := range platforms {
		importSet := make(map[string]bool)
		var platformFiles []string

		for _, f := range files {
			// Skip test files - they shouldn't contribute to library deps
			if f.IsTest {
				continue
			}
			if MatchesPlatform(f, p) {
				platformFiles = append(platformFiles, filepath.Base(f.Path))
				for _, imp := range f.Imports {
					importSet[imp] = true
				}
				for _, embed := range f.EmbedDirs {
					embedSet[embed] = true
				}
				if f.HasCgo {
					pkg.HasCgo = true
				}
			}
		}

		if len(platformFiles) > 0 {
			sort.Strings(platformFiles)
			pkg.GoFiles[p] = platformFiles

			var imports []string
			for imp := range importSet {
				imports = append(imports, imp)
			}
			sort.Strings(imports)
			pkg.Imports[p] = imports
		}
	}

	for embed := range embedSet {
		pkg.EmbedDirs = append(pkg.EmbedDirs, embed)
	}
	sort.Strings(pkg.EmbedDirs)

	return pkg, nil
}
