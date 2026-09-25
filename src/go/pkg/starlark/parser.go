package starlark

import (
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"

	"go.starlark.net/syntax"
)

// ParseFile parses a rules.star file and returns the object model.
func ParseFile(path string) (*File, error) {
	source, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("reading file: %w", err)
	}

	return Parse(path, source)
}

// Parse parses rules.star source code and returns the object model.
func Parse(path string, source []byte) (*File, error) {
	ast, err := syntax.Parse(path, source, syntax.RetainComments)
	if err != nil {
		return nil, fmt.Errorf("parsing starlark: %w", err)
	}

	f := &File{
		Path:   path,
		Source: source,
		AST:    ast,
	}

	for _, stmt := range ast.Stmts {
		switch s := stmt.(type) {
		case *syntax.LoadStmt:
			load, err := parseLoad(s, source)
			if err != nil {
				return nil, fmt.Errorf("parsing load: %w", err)
			}
			f.Loads = append(f.Loads, load)

		case *syntax.ExprStmt:
			if call, ok := s.X.(*syntax.CallExpr); ok {
				target, err := parseTarget(call, source)
				if err != nil {
					// Not a valid target, skip
					continue
				}
				f.Targets = append(f.Targets, target)
			}
		}
	}

	return f, nil
}

// parseLoad parses a LoadStmt into a Load.
func parseLoad(stmt *syntax.LoadStmt, source []byte) (*Load, error) {
	load := &Load{
		Module: stmt.ModuleName(),
		Stmt:   stmt,
		span:   spanFromNode(stmt, source),
	}

	for i, to := range stmt.To {
		load.Symbols = append(load.Symbols, LoadSymbol{
			Name:     to.Name,
			Original: stmt.From[i].Name,
		})
	}

	return load, nil
}

// parseTarget parses a CallExpr into a Target.
func parseTarget(call *syntax.CallExpr, source []byte) (*Target, error) {
	// Get the function name
	ident, ok := call.Fn.(*syntax.Ident)
	if !ok {
		return nil, fmt.Errorf("call target is not an identifier")
	}

	target := &Target{
		Rule:          ident.Name,
		Expr:          call,
		Attributes:    make(map[string]*Attribute),
		modifiedAttrs: make(map[string]bool),
		span:          spanFromNode(call, source),
	}

	// Parse arguments as attributes
	for _, arg := range call.Args {
		binop, ok := arg.(*syntax.BinaryExpr)
		if !ok || binop.Op != syntax.EQ {
			// Not a named argument, skip
			continue
		}

		nameIdent, ok := binop.X.(*syntax.Ident)
		if !ok {
			continue
		}

		attr, err := parseAttribute(nameIdent.Name, binop, source)
		if err != nil {
			continue
		}

		target.Attributes[attr.Name] = attr
		target.AttributeOrder = append(target.AttributeOrder, attr.Name)

		// Extract target name
		if attr.Name == "name" {
			if str, ok := attr.Value.(StringValue); ok {
				target.Name = str.Value
			}
		}
	}

	if target.Name == "" {
		return nil, fmt.Errorf("target has no name attribute")
	}

	return target, nil
}

// parseAttribute parses a BinaryExpr (name = value) into an Attribute.
func parseAttribute(name string, expr *syntax.BinaryExpr, source []byte) (*Attribute, error) {
	attr := &Attribute{
		Name: name,
		Expr: expr,
		span: spanFromNode(expr, source),
	}

	// Special handling for deps attribute to support markers
	if name == "deps" {
		if list, ok := expr.Y.(*syntax.ListExpr); ok {
			attr.Value = parseDepsValue(list, source)
			return attr, nil
		}
	}

	attr.Value = parseValue(expr.Y, source)

	return attr, nil
}

// parseValue parses an expression into an AttributeValue.
func parseValue(expr syntax.Expr, source []byte) AttributeValue {
	switch e := expr.(type) {
	case *syntax.Literal:
		switch e.Token {
		case syntax.STRING:
			// Remove quotes from string literal
			s, _ := strconv.Unquote(e.Raw)
			return StringValue{Value: s}
		case syntax.INT:
			val, _ := strconv.ParseInt(e.Raw, 0, 64)
			return IntValue{Value: val}
		}

	case *syntax.Ident:
		switch e.Name {
		case "True":
			return BoolValue{Value: true}
		case "False":
			return BoolValue{Value: false}
		default:
			return IdentValue{Name: e.Name}
		}

	case *syntax.ListExpr:
		return parseListValue(e, source)

	case *syntax.CallExpr:
		// Could be glob(["*.go"]) or similar
		if ident, ok := e.Fn.(*syntax.Ident); ok && ident.Name == "glob" {
			// Preserve as ExprValue
			return ExprValue{
				Expr:         e,
				originalText: extractText(e, source),
			}
		}
	}

	// Fall back to ExprValue for complex expressions
	return ExprValue{
		Expr:         expr,
		originalText: extractText(expr, source),
	}
}

// parseListValue parses a ListExpr into an AttributeValue.
func parseListValue(list *syntax.ListExpr, source []byte) AttributeValue {
	// Try to parse as string list
	var strings []string
	allStrings := true

	for _, elem := range list.List {
		if lit, ok := elem.(*syntax.Literal); ok && lit.Token == syntax.STRING {
			s, _ := strconv.Unquote(lit.Raw)
			strings = append(strings, s)
		} else {
			allStrings = false
			break
		}
	}

	if allStrings {
		return StringListValue{Values: strings}
	}

	// Not a pure string list, preserve as ExprValue
	return ExprValue{
		Expr:         list,
		originalText: extractText(list, source),
	}
}

// parseDepsValue parses a deps attribute with marker support: the labels
// between turnkey:auto-start and turnkey:auto-end are the auto-managed
// deps; every other label (between turnkey:preserve-start and
// turnkey:preserve-end, or outside any markers) is preserved, since a person
// wrote it. The markers are read from the syntax tree's comments, so any
// layout works, including several labels on one line.
func parseDepsValue(list *syntax.ListExpr, source []byte) AttributeValue {
	markers := listMarkers(list)
	if len(markers) == 0 {
		return parseListValue(list, source)
	}

	depsValue := DepsValue{HasMarkers: true}
	inAutoSection := false
	next := 0
	for _, elem := range list.List {
		start, _ := elem.Span()
		for ; next < len(markers) && precedes(markers[next].position, start); next++ {
			inAutoSection = markers[next].opensAuto
		}
		lit, ok := elem.(*syntax.Literal)
		if !ok || lit.Token != syntax.STRING {
			continue
		}
		dep := lit.Value.(string)
		if inAutoSection {
			depsValue.AutoDeps = append(depsValue.AutoDeps, dep)
		} else {
			depsValue.PreservedDeps = append(depsValue.PreservedDeps, dep)
		}
	}
	return depsValue
}

// marker is a turnkey section comment inside a deps list.
type marker struct {
	position syntax.Position
	// opensAuto: turnkey:auto-start; every other marker ends the auto section
	opensAuto bool
}

// listMarkers returns the turnkey markers among the comments inside list, in
// source order, wherever the parser attached them.
func listMarkers(list *syntax.ListExpr) []marker {
	var markers []marker
	syntax.Walk(list, func(n syntax.Node) bool {
		// Walk calls f with nil once it has visited a node's children
		if n == nil {
			return false
		}
		comments := n.Comments()
		if comments == nil {
			return true
		}
		for _, group := range [][]syntax.Comment{comments.Before, comments.Suffix, comments.After} {
			for _, c := range group {
				switch strings.TrimSpace(strings.TrimPrefix(c.Text, "#")) {
				case "turnkey:auto-start":
					markers = append(markers, marker{c.Start, true})
				case "turnkey:auto-end", "turnkey:preserve-start", "turnkey:preserve-end":
					markers = append(markers, marker{c.Start, false})
				}
			}
		}
		return true
	})
	sort.Slice(markers, func(i, j int) bool { return precedes(markers[i].position, markers[j].position) })
	return markers
}

// precedes reports whether position a comes before position b.
func precedes(a, b syntax.Position) bool {
	return a.Line < b.Line || (a.Line == b.Line && a.Col < b.Col)
}

// spanFromNode creates a Span from a syntax.Node.
func spanFromNode(node syntax.Node, source []byte) Span {
	start, end := node.Span()
	return Span{
		Start: positionToOffset(start, source),
		End:   positionToOffset(end, source),
	}
}

// positionToOffset converts a syntax.Position to a byte offset.
func positionToOffset(pos syntax.Position, source []byte) int {
	// Position is 1-indexed line and column
	line := int(pos.Line)
	col := int(pos.Col)

	offset := 0
	currentLine := 1

	for i, b := range source {
		if currentLine == line {
			// Found the line, add column offset
			return offset + col - 1
		}
		if b == '\n' {
			currentLine++
		}
		offset = i + 1
	}

	return len(source)
}

// extractText extracts the original source text for a node.
func extractText(node syntax.Node, source []byte) string {
	span := spanFromNode(node, source)
	if span.Start >= 0 && span.End <= len(source) && span.Start < span.End {
		return string(source[span.Start:span.End])
	}
	return ""
}
