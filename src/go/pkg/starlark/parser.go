package starlark

import (
	"fmt"
	"os"
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

// parseDepsValue parses a deps attribute with marker support.
// It looks for turnkey:auto-start/end and turnkey:preserve-start/end markers.
func parseDepsValue(list *syntax.ListExpr, source []byte) AttributeValue {
	// Get the original text for this list
	text := extractText(list, source)

	// Check for markers
	hasAutoStart := strings.Contains(text, "# turnkey:auto-start")
	hasAutoEnd := strings.Contains(text, "# turnkey:auto-end")
	hasPreserveStart := strings.Contains(text, "# turnkey:preserve-start")
	hasPreserveEnd := strings.Contains(text, "# turnkey:preserve-end")

	// If no markers, parse as regular string list
	if !hasAutoStart && !hasAutoEnd && !hasPreserveStart && !hasPreserveEnd {
		return parseListValue(list, source)
	}

	// Parse with markers
	depsValue := DepsValue{
		HasMarkers: true,
	}

	// Parse the text line by line to extract deps in each section
	lines := strings.Split(text, "\n")
	inAutoSection := false
	inPreserveSection := false

	for _, line := range lines {
		trimmed := strings.TrimSpace(line)

		// Check for markers
		if strings.Contains(trimmed, "# turnkey:auto-start") {
			inAutoSection = true
			continue
		}
		if strings.Contains(trimmed, "# turnkey:auto-end") {
			inAutoSection = false
			continue
		}
		if strings.Contains(trimmed, "# turnkey:preserve-start") {
			inPreserveSection = true
			continue
		}
		if strings.Contains(trimmed, "# turnkey:preserve-end") {
			inPreserveSection = false
			continue
		}

		// Skip empty lines, brackets, and pure comments
		if trimmed == "" || trimmed == "[" || trimmed == "]" || trimmed == "]," {
			continue
		}
		if strings.HasPrefix(trimmed, "#") {
			continue
		}

		// Extract the dep string
		dep := extractDepFromLine(trimmed)
		if dep == "" {
			continue
		}

		if inAutoSection {
			depsValue.AutoDeps = append(depsValue.AutoDeps, dep)
		} else if inPreserveSection {
			depsValue.PreservedDeps = append(depsValue.PreservedDeps, dep)
		}
		// Deps outside markers are ignored (they'll be in one section or another after sync)
	}

	return depsValue
}

// extractDepFromLine extracts a dependency string from a line like `"//foo:bar",`
func extractDepFromLine(line string) string {
	// Remove trailing comma
	line = strings.TrimSuffix(line, ",")
	line = strings.TrimSpace(line)

	// Remove inline comments
	if idx := strings.Index(line, "#"); idx != -1 {
		line = strings.TrimSpace(line[:idx])
	}

	// Unquote the string
	if strings.HasPrefix(line, `"`) && strings.HasSuffix(line, `"`) {
		s, err := strconv.Unquote(line)
		if err == nil {
			return s
		}
	}

	return ""
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
