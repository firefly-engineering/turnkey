package goparse

import (
	"fmt"
	"go/build/constraint"
	"go/parser"
	"go/scanner"
	"go/token"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

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
		// The path is a Go string literal, in double quotes or backquotes
		path, err := strconv.Unquote(imp.Path.Value)
		if err != nil {
			return nil, fmt.Errorf("%s: import %s: %v", fset.Position(imp.Path.Pos()), imp.Path.Value, err)
		}
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

	embeds, err := extractEmbeds(fset, path)
	if err != nil {
		return nil, err
	}
	gf.EmbedDirs = embeds

	return gf, nil
}

// extractEmbeds returns the patterns of every //go:embed directive in a
// file. A directive can follow the imports anywhere in the file, past
// where the ImportsOnly parse stops, so the file is tokenized with
// go/scanner: that finds the line comments that are directives, and not
// a "//go:embed" inside a string, a block comment or another comment.
func extractEmbeds(fset *token.FileSet, path string) ([]string, error) {
	src, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var s scanner.Scanner
	// Errors in the rest of the file don't matter here; the parse above
	// has already accepted the package clause and imports in ParseFile
	s.Init(fset.AddFile(path, -1, len(src)), src, nil, scanner.ScanComments)
	var embeds []string
	for {
		pos, tok, lit := s.Scan()
		if tok == token.EOF {
			return embeds, nil
		}
		if tok != token.COMMENT {
			continue
		}
		args, ok := strings.CutPrefix(lit, "//go:embed")
		if !ok || (args != "" && args[0] != ' ' && args[0] != '\t') {
			continue
		}
		patterns, err := parseEmbedPatterns(args)
		if err != nil {
			return nil, fmt.Errorf("%s: invalid //go:embed: %v", fset.Position(pos), err)
		}
		embeds = append(embeds, patterns...)
	}
}

// parseEmbedPatterns splits a //go:embed directive's arguments as the go
// command does: separated by spaces, each either bare or a Go string
// literal in double quotes or backquotes.
func parseEmbedPatterns(args string) ([]string, error) {
	var patterns []string
	for {
		args = strings.TrimLeft(args, " \t")
		if args == "" {
			return patterns, nil
		}
		var pattern string
		switch args[0] {
		case '"', '`':
			quoted, err := strconv.QuotedPrefix(args)
			if err != nil {
				return nil, fmt.Errorf("unterminated or malformed string %s", args)
			}
			pattern, _ = strconv.Unquote(quoted)
			args = args[len(quoted):]
			if args != "" && args[0] != ' ' && args[0] != '\t' {
				return nil, fmt.Errorf("missing space after %s", quoted)
			}
		default:
			end := strings.IndexAny(args, " \t")
			if end < 0 {
				end = len(args)
			}
			pattern, args = args[:end], args[end:]
		}
		patterns = append(patterns, pattern)
	}
}
