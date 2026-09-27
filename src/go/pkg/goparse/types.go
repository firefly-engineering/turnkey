package goparse

import (
	"go/build/constraint"
	"sort"
)

// GoFile represents parsed info from a single .go file
type GoFile struct {
	Path       string          // file path
	Package    string          // package name
	Imports    []string        // import paths
	EmbedDirs  []string        // from //go:embed directives
	Constraint constraint.Expr // parsed build constraint (nil if none)
	HasCgo     bool            // true if imports "C"
	IsTest     bool            // true if *_test.go
}

// GoPackage represents the Go files of a package directory
type GoPackage struct {
	Dir        string    // directory path
	ImportPath string    // full import path
	Name       string    // package name
	Files      []*GoFile // its non-test Go files, whatever their constraints
	EmbedDirs  []string  // embed directories (common)
	HasCgo     bool      // any file has cgo
}

// Imports returns the imports of the package's files that are part of a
// build in ctx, sorted and without duplicates. It reports false if no file
// is: the package isn't built there.
func (p *GoPackage) Imports(ctx BuildContext) ([]string, bool) {
	seen := make(map[string]bool)
	built := false
	for _, f := range p.Files {
		if !ctx.Matches(f) {
			continue
		}
		built = true
		for _, imp := range f.Imports {
			seen[imp] = true
		}
	}
	imports := make([]string, 0, len(seen))
	for imp := range seen {
		imports = append(imports, imp)
	}
	sort.Strings(imports)
	return imports, built
}
