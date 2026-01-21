package rules

import (
	"fmt"
	"os"
	"strings"

	"go.starlark.net/syntax"
)

// Parser parses and generates rules.star files using proper Starlark AST.
type Parser struct {
	// Indent is the indentation string to use (default: 4 spaces).
	Indent string
}

// NewParser creates a new Parser with default settings.
func NewParser() *Parser {
	return &Parser{
		Indent: "    ",
	}
}

// ParseFile reads and parses a rules.star file.
func (p *Parser) ParseFile(path string) (*RulesFile, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read %s: %w", path, err)
	}

	return p.Parse(path, string(content))
}

// Parse parses rules.star content using Starlark AST.
func (p *Parser) Parse(path, content string) (*RulesFile, error) {
	// Parse the Starlark file
	f, err := syntax.Parse(path, content, 0)
	if err != nil {
		return nil, fmt.Errorf("failed to parse Starlark: %w", err)
	}

	rf := &RulesFile{
		Path:       path,
		RawContent: content,
	}

	// Extract hash from comments if present
	for _, comment := range f.Stmts {
		if c, ok := comment.(*syntax.CommentBlock); ok {
			for _, line := range c.Lines {
				if strings.Contains(line.Text, "Auto-managed by turnkey") {
					if idx := strings.Index(line.Text, "Hash:"); idx >= 0 {
						rf.Hash = strings.TrimSpace(line.Text[idx+5:])
					}
				}
			}
		}
	}

	// Process statements
	for _, stmt := range f.Stmts {
		switch s := stmt.(type) {
		case *syntax.LoadStmt:
			// Reconstruct load statement as string
			rf.Loads = append(rf.Loads, p.formatLoadStmt(s))

		case *syntax.ExprStmt:
			// Check if it's a function call (target definition)
			if call, ok := s.X.(*syntax.CallExpr); ok {
				target := p.parseTargetCall(call, content)
				if target != nil {
					rf.Targets = append(rf.Targets, target)
				}
			}
		}
	}

	return rf, nil
}

// formatLoadStmt formats a load statement back to string.
func (p *Parser) formatLoadStmt(load *syntax.LoadStmt) string {
	var parts []string
	for i, from := range load.From {
		to := load.To[i]
		if from.Name == to.Name {
			parts = append(parts, fmt.Sprintf("%q", from.Name))
		} else {
			parts = append(parts, fmt.Sprintf("%s = %q", to.Name, from.Name))
		}
	}
	return fmt.Sprintf("load(%q, %s)", load.Module.Value, strings.Join(parts, ", "))
}

// parseTargetCall parses a target function call (e.g., go_binary(...)).
func (p *Parser) parseTargetCall(call *syntax.CallExpr, content string) *Target {
	// Get the rule name (function being called)
	var ruleName string
	switch fn := call.Fn.(type) {
	case *syntax.Ident:
		ruleName = fn.Name
	case *syntax.DotExpr:
		// Handle cases like module.rule_name
		if ident, ok := fn.Name.(*syntax.Ident); ok {
			ruleName = ident.Name
		}
	default:
		return nil
	}

	target := &Target{
		Rule:      ruleName,
		StartLine: int(call.Lparen.Line),
		EndLine:   int(call.Rparen.Line),
	}

	// Parse keyword arguments
	for _, arg := range call.Args {
		if binOp, ok := arg.(*syntax.BinaryExpr); ok && binOp.Op == syntax.EQ {
			if ident, ok := binOp.X.(*syntax.Ident); ok {
				p.parseTargetArg(target, ident.Name, binOp.Y, content)
			}
		}
	}

	return target
}

// parseTargetArg parses a single target argument.
func (p *Parser) parseTargetArg(target *Target, name string, value syntax.Expr, content string) {
	switch name {
	case "name":
		if lit, ok := value.(*syntax.Literal); ok && lit.Token == syntax.STRING {
			target.Name = lit.Value.(string)
		}

	case "srcs":
		target.Srcs = p.parseStringListOrGlob(value)

	case "deps":
		target.Deps, target.AutoDeps, target.PreservedDeps = p.parseDepsWithMarkers(value, content)
	}
}

// parseStringListOrGlob parses a list of strings or a glob() call.
func (p *Parser) parseStringListOrGlob(expr syntax.Expr) []string {
	switch e := expr.(type) {
	case *syntax.ListExpr:
		var result []string
		for _, elem := range e.List {
			if lit, ok := elem.(*syntax.Literal); ok && lit.Token == syntax.STRING {
				result = append(result, lit.Value.(string))
			}
		}
		return result

	case *syntax.CallExpr:
		// Handle glob(["*.go"])
		if ident, ok := e.Fn.(*syntax.Ident); ok && ident.Name == "glob" {
			if len(e.Args) > 0 {
				if list, ok := e.Args[0].(*syntax.ListExpr); ok {
					var patterns []string
					for _, elem := range list.List {
						if lit, ok := elem.(*syntax.Literal); ok && lit.Token == syntax.STRING {
							patterns = append(patterns, lit.Value.(string))
						}
					}
					return patterns
				}
			}
		}
	}
	return nil
}

// parseDepsWithMarkers parses deps list and extracts auto/preserved sections.
func (p *Parser) parseDepsWithMarkers(expr syntax.Expr, content string) (all, auto, preserved []string) {
	list, ok := expr.(*syntax.ListExpr)
	if !ok {
		return nil, nil, nil
	}

	// Get the source range for the deps list to extract comments
	startLine := int(list.Lbrack.Line)
	endLine := int(list.Rbrack.Line)

	// Extract the deps section from content to find markers
	lines := strings.Split(content, "\n")
	inAutoSection := false
	inPreserveSection := false

	// Track which deps are in which section based on line positions
	depLineMap := make(map[int]string) // line -> dep value
	for _, elem := range list.List {
		if lit, ok := elem.(*syntax.Literal); ok && lit.Token == syntax.STRING {
			dep := lit.Value.(string)
			all = append(all, dep)
			depLineMap[int(lit.TokenPos.Line)] = dep
		}
	}

	// Scan lines for markers and categorize deps
	for lineNum := startLine; lineNum <= endLine && lineNum <= len(lines); lineNum++ {
		line := lines[lineNum-1] // lines are 1-indexed

		if strings.Contains(line, MarkerAutoStart) {
			inAutoSection = true
			continue
		}
		if strings.Contains(line, MarkerAutoEnd) {
			inAutoSection = false
			continue
		}
		if strings.Contains(line, MarkerPreserveStart) {
			inPreserveSection = true
			continue
		}
		if strings.Contains(line, MarkerPreserveEnd) {
			inPreserveSection = false
			continue
		}

		// Check if this line has a dep
		if dep, ok := depLineMap[lineNum]; ok {
			if inPreserveSection {
				preserved = append(preserved, dep)
			} else {
				// Default to auto (including when in auto section or no markers)
				auto = append(auto, dep)
			}
		}
	}

	// If no markers found, all deps are auto-managed
	if len(auto) == 0 && len(preserved) == 0 {
		auto = all
	}

	return all, auto, preserved
}

// GenerateTarget generates the rules.star content for a target with updated deps.
func (p *Parser) GenerateTarget(target *Target, newAutoDeps []string) string {
	indent := p.Indent

	var lines []string
	lines = append(lines, fmt.Sprintf("%s(", target.Rule))
	lines = append(lines, fmt.Sprintf("%sname = \"%s\",", indent, target.Name))

	// Add srcs if present
	if len(target.Srcs) > 0 {
		if len(target.Srcs) == 1 && !strings.Contains(target.Srcs[0], "*") {
			lines = append(lines, fmt.Sprintf("%ssrcs = [\"%s\"],", indent, target.Srcs[0]))
		} else if len(target.Srcs) == 1 && strings.Contains(target.Srcs[0], "*") {
			// It's a glob pattern
			lines = append(lines, fmt.Sprintf("%ssrcs = glob([\"%s\"]),", indent, target.Srcs[0]))
		} else {
			lines = append(lines, fmt.Sprintf("%ssrcs = [", indent))
			for _, src := range target.Srcs {
				lines = append(lines, fmt.Sprintf("%s%s\"%s\",", indent, indent, src))
			}
			lines = append(lines, fmt.Sprintf("%s],", indent))
		}
	}

	// Add deps with markers
	allDeps := append(newAutoDeps, target.PreservedDeps...)
	if len(allDeps) > 0 {
		lines = append(lines, fmt.Sprintf("%sdeps = [", indent))
		lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerAutoStart))
		for _, dep := range newAutoDeps {
			lines = append(lines, fmt.Sprintf("%s%s\"%s\",", indent, indent, dep))
		}
		lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerAutoEnd))

		if len(target.PreservedDeps) > 0 {
			lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerPreserveStart))
			for _, dep := range target.PreservedDeps {
				lines = append(lines, fmt.Sprintf("%s%s\"%s\",", indent, indent, dep))
			}
			lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerPreserveEnd))
		}
		lines = append(lines, fmt.Sprintf("%s],", indent))
	}

	lines = append(lines, fmt.Sprintf("%svisibility = [\"PUBLIC\"],", indent))
	lines = append(lines, ")")

	return strings.Join(lines, "\n")
}

// GenerateHeader generates the file header with hash comment.
func (p *Parser) GenerateHeader(hash string) string {
	return fmt.Sprintf("# Auto-managed by turnkey. Hash: %s\n# Manual sections marked with turnkey:preserve-start/end are not modified.\n", hash)
}

// FindTarget finds a target by name in a rules file.
func (rf *RulesFile) FindTarget(name string) *Target {
	for _, t := range rf.Targets {
		if t.Name == name {
			return t
		}
	}
	return nil
}

// HasMarkers returns true if any target has turnkey markers.
func (rf *RulesFile) HasMarkers() bool {
	return strings.Contains(rf.RawContent, MarkerAutoStart) ||
		strings.Contains(rf.RawContent, MarkerPreserveStart)
}
