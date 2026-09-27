package goparse

import (
	"bufio"
	"go/build/constraint"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

var embedRegexp = regexp.MustCompile(`//go:embed\s+(.*)`)

// ParseFile parses a single Go file and extracts metadata.
// Uses go/parser with ImportsOnly mode for efficiency.
// Also parses comments for //go:build and //go:embed directives.
func ParseFile(path string) (*GoFile, error) {
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, nil, parser.ImportsOnly|parser.ParseComments)
	if err != nil {
		return nil, err
	}

	gf := &GoFile{
		Path:    path,
		Package: f.Name.Name,
		IsTest:  strings.HasSuffix(filepath.Base(path), "_test.go"),
	}

	for _, imp := range f.Imports {
		path := strings.Trim(imp.Path.Value, `"`)
		gf.Imports = append(gf.Imports, path)
		if path == "C" {
			gf.HasCgo = true
		}
	}

	// Extract build constraints, which must appear before the package
	// clause: //go:build, or in files that predate it, the // +build
	// lines, which all must hold
	var plusBuild constraint.Expr
	for _, cg := range f.Comments {
		if cg.Pos() >= f.Package {
			continue
		}
		for _, c := range cg.List {
			expr, err := constraint.Parse(c.Text)
			if err != nil {
				continue
			}
			switch {
			case constraint.IsGoBuild(c.Text):
				gf.Constraint = expr
			case constraint.IsPlusBuild(c.Text):
				if plusBuild == nil {
					plusBuild = expr
				} else {
					plusBuild = &constraint.AndExpr{X: plusBuild, Y: expr}
				}
			}
		}
	}
	if gf.Constraint == nil {
		gf.Constraint = plusBuild
	}

	// Extract //go:embed directives
	// These can be anywhere but usually near variables.
	// Since we used ImportsOnly, we might not get all comments if they are not at the top?
	// Actually parser.ImportsOnly stops after imports, but ParseComments might still include them.
	// If ImportsOnly stops too early, we might need to scan the file manually for embeds.
	// Let's scan the file for embeds to be safe, or check if f.Comments has them.

	// According to Go specs, go:embed can be anywhere.
	// Let's use a scanner to find all //go:embed in the file.
	embeds, err := extractEmbeds(path)
	if err == nil {
		gf.EmbedDirs = embeds
	}

	return gf, nil
}

func extractEmbeds(path string) ([]string, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var embeds []string
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := scanner.Text()
		matches := embedRegexp.FindStringSubmatch(line)
		if len(matches) > 1 {
			args := strings.Fields(matches[1])
			embeds = append(embeds, args...)
		}
	}
	return embeds, scanner.Err()
}
