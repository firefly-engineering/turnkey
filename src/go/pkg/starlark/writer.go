package starlark

import (
	"sort"
	"strconv"
	"strings"
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
			write:    func(b *strings.Builder) { writeTarget(b, target) },
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

	case ExprValue:
		// Use original text for complex expressions
		b.WriteString(v.originalText)
	}
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
