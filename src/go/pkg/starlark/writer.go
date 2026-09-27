package starlark

import (
	"sort"
	"strconv"
	"strings"

	"go.starlark.net/syntax"
)

// Write serializes the file back to rules.star format.
// It preserves unchanged parts from the original source.
func (f *File) Write() []byte {
	if !f.IsModified() {
		// No modifications, return original source
		return f.Source
	}

	// Build output by copying unchanged spans and regenerating modified ones
	var builder strings.Builder

	// Track position in original source
	pos := 0

	// Sort all spans (loads and targets) by position
	type spanItem struct {
		span     Span
		modified bool
		write    func(*strings.Builder)
	}
	var items []spanItem

	for _, load := range f.Loads {
		items = append(items, spanItem{
			span:     load.span,
			modified: load.modified,
			write:    func(b *strings.Builder) { writeLoad(b, load) },
		})
	}

	for _, target := range f.Targets {
		items = append(items, spanItem{
			span:     target.span,
			modified: target.modified,
			write: func(b *strings.Builder) {
				// Rewrite only the attributes that changed, if possible
				if target.span.End > 0 {
					if text, ok := spliceTarget(f.Source, target); ok {
						b.WriteString(text)
						return
					}
				}
				writeTarget(b, target)
			},
		})
	}

	// Sort by start position
	sort.Slice(items, func(i, j int) bool {
		return items[i].span.Start < items[j].span.Start
	})

	// Build output
	for _, item := range items {
		// Handle new items (no span)
		if item.span.Start == 0 && item.span.End == 0 && item.modified {
			item.write(&builder)
			builder.WriteByte('\n')
			continue
		}

		// Copy everything before this item
		if item.span.Start > pos {
			builder.Write(f.Source[pos:item.span.Start])
		}

		if item.modified {
			// Regenerate this item
			item.write(&builder)
		} else {
			// Copy original
			builder.Write(f.Source[item.span.Start:item.span.End])
		}

		pos = item.span.End
	}

	// Copy remaining content
	if pos < len(f.Source) {
		builder.Write(f.Source[pos:])
	}

	return []byte(builder.String())
}

// WriteFormatted writes the file with consistent formatting.
// This regenerates all content, not just modified parts.
func (f *File) WriteFormatted() []byte {
	var builder strings.Builder

	// Write loads
	for _, load := range f.Loads {
		writeLoad(&builder, load)
		builder.WriteByte('\n')
	}

	if len(f.Loads) > 0 && len(f.Targets) > 0 {
		builder.WriteByte('\n')
	}

	// Write targets
	for i, target := range f.Targets {
		if i > 0 {
			builder.WriteByte('\n')
		}
		writeTarget(&builder, target)
		builder.WriteByte('\n')
	}

	return []byte(builder.String())
}

// writeLoad writes a load statement.
func writeLoad(b *strings.Builder, load *Load) {
	b.WriteString("load(")
	b.WriteString(strconv.Quote(load.Module))

	for _, sym := range load.Symbols {
		b.WriteString(", ")
		if sym.Name == sym.Original {
			b.WriteString(strconv.Quote(sym.Original))
		} else {
			b.WriteString(sym.Name)
			b.WriteString(" = ")
			b.WriteString(strconv.Quote(sym.Original))
		}
	}

	b.WriteString(")")
}

// writeTarget writes a target (function call).
func writeTarget(b *strings.Builder, target *Target) {
	b.WriteString(target.Rule)
	b.WriteString("(\n")

	// Determine attribute order
	order := target.AttributeOrder
	if len(order) == 0 {
		// Use sorted keys as fallback
		for name := range target.Attributes {
			order = append(order, name)
		}
		sort.Strings(order)
	}

	for _, name := range order {
		attr, exists := target.Attributes[name]
		if !exists {
			continue
		}

		b.WriteString("    ")
		b.WriteString(name)
		b.WriteString(" = ")
		writeValue(b, attr.Value, "    ")
		b.WriteString(",\n")
	}

	b.WriteString(")")
}

// writeValue writes an attribute value.
func writeValue(b *strings.Builder, value AttributeValue, indent string) {
	switch v := value.(type) {
	case StringValue:
		b.WriteString(strconv.Quote(v.Value))

	case StringListValue:
		writeStringList(b, v.Values, indent)

	case DepsValue:
		writeDepsValue(b, v, indent)

	case BoolValue:
		if v.Value {
			b.WriteString("True")
		} else {
			b.WriteString("False")
		}

	case IntValue:
		b.WriteString(strconv.FormatInt(v.Value, 10))

	case IdentValue:
		b.WriteString(v.Name)

	case SelectValue:
		writeSelect(b, v, indent)

	case ExprValue:
		// Use original text for complex expressions
		b.WriteString(v.originalText)
	}
}

// Render returns an attribute value as sync writes it, on one line where
// it fits. Two values that render the same are the same.
func Render(value AttributeValue) string {
	return renderValue(value, "")
}

// RenderIndented returns an attribute value as written on a line indented
// by indent, e.g. an attribute of a rule call ("    ").
func RenderIndented(value AttributeValue, indent string) string {
	return renderValue(value, indent)
}

// renderValue returns an attribute value as written on a line indented by
// indent.
func renderValue(value AttributeValue, indent string) string {
	var b strings.Builder
	writeValue(&b, value, indent)
	return b.String()
}

// writeSelect writes [<common>] + select({...}), or select({...}) alone.
func writeSelect(b *strings.Builder, v SelectValue, indent string) {
	if v.Common != nil {
		writeValue(b, v.Common, indent)
		b.WriteString(" + ")
	}
	b.WriteString("select({\n")
	for _, branch := range v.Branches {
		b.WriteString(indent)
		b.WriteString("    ")
		b.WriteString(strconv.Quote(branch.Key))
		b.WriteString(": ")
		writeValue(b, branch.Value, indent+"    ")
		b.WriteString(",\n")
	}
	b.WriteString(indent)
	b.WriteString("})")
}

// edit replaces the source bytes [start, end) with text.
type edit struct {
	start, end int
	text       string
}

// spliceTarget returns a modified target's source with only what changed
// rewritten: each modified attribute's value (only the changed parts of a
// select() whose keys are unchanged), and new attributes inserted before
// the closing parenthesis. It reports false if that isn't possible (an
// attribute was removed, or the call's layout leaves no clean place for a
// new one); the whole target is then regenerated.
func spliceTarget(src []byte, t *Target) (string, bool) {
	for name := range t.modifiedAttrs {
		if _, ok := t.Attributes[name]; !ok {
			return "", false
		}
	}

	var edits []edit
	for _, name := range t.AttributeOrder {
		if !t.modifiedAttrs[name] {
			continue
		}
		attr := t.Attributes[name]
		if attr.Expr == nil {
			e, ok := insertAttribute(src, t, attr)
			if !ok {
				return "", false
			}
			edits = append(edits, e)
			continue
		}
		edits = append(edits, valueEdits(src, attr)...)
	}
	sort.SliceStable(edits, func(i, j int) bool { return edits[i].start < edits[j].start })

	var b strings.Builder
	pos := t.span.Start
	for _, e := range edits {
		b.Write(src[pos:e.start])
		b.WriteString(e.text)
		pos = e.end
	}
	b.Write(src[pos:t.span.End])
	return b.String(), true
}

// valueEdits returns the edits that rewrite an existing attribute's value.
func valueEdits(src []byte, attr *Attribute) []edit {
	span := spanFromNode(attr.Expr.Y, src)
	if sel, ok := attr.Value.(SelectValue); ok {
		if orig, ok := attr.original.(SelectValue); ok {
			if edits, ok := selectEdits(src, attr.Expr.Y, orig, sel); ok {
				return edits
			}
		}
	}
	return []edit{{span.Start, span.End, renderValue(attr.Value, lineIndent(src, span.Start))}}
}

// selectEdits returns the edits that turn the select() expr, parsed as
// orig, into sel: one per changed part. It reports false if the two have
// different shapes (a common part added or removed, different keys), which
// needs the whole value rewritten.
func selectEdits(src []byte, expr syntax.Expr, orig, sel SelectValue) ([]edit, bool) {
	if (orig.Common == nil) != (sel.Common == nil) || len(orig.Branches) != len(sel.Branches) {
		return nil, false
	}
	for i := range orig.Branches {
		if orig.Branches[i].Key != sel.Branches[i].Key {
			return nil, false
		}
	}

	var edits []edit
	replace := func(node syntax.Node, before, after AttributeValue) {
		if renderValue(before, "") == renderValue(after, "") {
			return
		}
		span := spanFromNode(node, src)
		edits = append(edits, edit{span.Start, span.End, renderValue(after, lineIndent(src, span.Start))})
	}

	call := expr
	if bin, ok := expr.(*syntax.BinaryExpr); ok {
		replace(bin.X, orig.Common, sel.Common)
		call = bin.Y
	}
	dict := call.(*syntax.CallExpr).Args[0].(*syntax.DictExpr)
	for i, e := range dict.List {
		replace(e.(*syntax.DictEntry).Value, orig.Branches[i].Value, sel.Branches[i].Value)
	}
	return edits, true
}

// insertAttribute returns the edit that adds a new attribute on its own
// line before the call's closing parenthesis, indented like the last
// argument. It reports false unless the parenthesis is on its own line and
// the last argument ends with a comma.
func insertAttribute(src []byte, t *Target, attr *Attribute) (edit, bool) {
	if len(t.Expr.Args) == 0 {
		return edit{}, false
	}
	rparen := positionToOffset(t.Expr.Rparen, src)
	lineStart := rparen
	for lineStart > 0 && src[lineStart-1] != '\n' {
		lineStart--
	}
	if strings.TrimSpace(string(src[lineStart:rparen])) != "" {
		return edit{}, false
	}
	last := spanFromNode(t.Expr.Args[len(t.Expr.Args)-1], src)
	if last.End > lineStart || !strings.HasPrefix(strings.TrimSpace(string(src[last.End:lineStart])), ",") {
		return edit{}, false
	}
	indent := lineIndent(src, last.Start)
	text := indent + attr.Name + " = " + renderValue(attr.Value, indent) + ",\n"
	return edit{lineStart, lineStart, text}, true
}

// lineIndent returns the leading whitespace of the line holding offset.
func lineIndent(src []byte, offset int) string {
	start := offset
	for start > 0 && src[start-1] != '\n' {
		start--
	}
	end := start
	for end < len(src) && (src[end] == ' ' || src[end] == '\t') {
		end++
	}
	return string(src[start:end])
}

// writeDepsValue writes a deps list with markers.
func writeDepsValue(b *strings.Builder, deps DepsValue, indent string) {
	if !deps.HasMarkers {
		// No markers, write as simple list
		writeStringList(b, deps.RawDeps, indent)
		return
	}

	// Write with markers
	allEmpty := len(deps.AutoDeps) == 0 && len(deps.PreservedDeps) == 0
	if allEmpty {
		b.WriteString("[]")
		return
	}

	b.WriteString("[\n")

	// Write auto-managed deps
	if len(deps.AutoDeps) > 0 || len(deps.PreservedDeps) > 0 {
		b.WriteString(indent)
		b.WriteString("    # turnkey:auto-start\n")
		for _, dep := range deps.AutoDeps {
			b.WriteString(indent)
			b.WriteString("    ")
			b.WriteString(strconv.Quote(dep))
			b.WriteString(",\n")
		}
		b.WriteString(indent)
		b.WriteString("    # turnkey:auto-end\n")
	}

	// Write preserved deps
	if len(deps.PreservedDeps) > 0 {
		b.WriteString(indent)
		b.WriteString("    # turnkey:preserve-start\n")
		for _, dep := range deps.PreservedDeps {
			b.WriteString(indent)
			b.WriteString("    ")
			b.WriteString(strconv.Quote(dep))
			b.WriteString(",\n")
		}
		b.WriteString(indent)
		b.WriteString("    # turnkey:preserve-end\n")
	}

	b.WriteString(indent)
	b.WriteString("]")
}

// writeStringList writes a list of strings.
func writeStringList(b *strings.Builder, values []string, indent string) {
	if len(values) == 0 {
		b.WriteString("[]")
		return
	}

	if len(values) == 1 {
		b.WriteString("[")
		b.WriteString(strconv.Quote(values[0]))
		b.WriteString("]")
		return
	}

	b.WriteString("[\n")
	for _, v := range values {
		b.WriteString(indent)
		b.WriteString("    ")
		b.WriteString(strconv.Quote(v))
		b.WriteString(",\n")
	}
	b.WriteString(indent)
	b.WriteString("]")
}

// FormatDeps formats a deps list with proper indentation.
// This is useful for deps that have markers like # turnkey:auto-start.
func FormatDepsWithMarkers(deps []string, preservedDeps []string, indent string) string {
	var b strings.Builder

	b.WriteString("[\n")

	// Auto-managed deps
	if len(deps) > 0 {
		b.WriteString(indent)
		b.WriteString("    # turnkey:auto-start\n")
		for _, dep := range deps {
			b.WriteString(indent)
			b.WriteString("    ")
			b.WriteString(strconv.Quote(dep))
			b.WriteString(",\n")
		}
		b.WriteString(indent)
		b.WriteString("    # turnkey:auto-end\n")
	}

	// Preserved deps
	if len(preservedDeps) > 0 {
		b.WriteString(indent)
		b.WriteString("    # turnkey:preserve-start\n")
		for _, dep := range preservedDeps {
			b.WriteString(indent)
			b.WriteString("    ")
			b.WriteString(strconv.Quote(dep))
			b.WriteString(",\n")
		}
		b.WriteString(indent)
		b.WriteString("    # turnkey:preserve-end\n")
	}

	b.WriteString(indent)
	b.WriteString("]")

	return b.String()
}
