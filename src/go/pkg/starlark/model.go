// Package starlark provides an object model for rules.star files with
// modification tracking and span-preserving serialization.
package starlark

import (
	"go.starlark.net/syntax"
)

// File represents a parsed rules.star file with modification tracking.
type File struct {
	// Path is the file path.
	Path string

	// Source is the original source code.
	Source []byte

	// AST is the parsed Starlark AST.
	AST *syntax.File

	// Loads are the load statements in the file.
	Loads []*Load

	// Targets are the build targets (function calls like go_library, rust_binary, etc.)
	Targets []*Target

	// HeaderComments are comments before the first statement.
	HeaderComments []string

	// modified tracks whether the file has been modified.
	modified bool
}

// Load represents a load statement.
type Load struct {
	// Module is the module being loaded (e.g., "@prelude//:rules.bzl").
	Module string

	// Symbols are the symbols being imported.
	Symbols []LoadSymbol

	// Stmt is the underlying AST node.
	Stmt *syntax.LoadStmt

	// span is the original byte range in the source.
	span Span

	// modified tracks whether this load has been modified.
	modified bool
}

// LoadSymbol represents a single symbol imported in a load statement.
type LoadSymbol struct {
	// Name is the local name (left side of alias, or the name itself).
	Name string

	// Original is the name in the loaded module (right side of alias, or same as Name).
	Original string
}

// Target represents a build target (a function call like go_library, rust_binary, etc.)
type Target struct {
	// Rule is the rule type (e.g., "go_library", "rust_binary").
	Rule string

	// Name is the target name (from the "name" attribute).
	Name string

	// Attributes are the target's attributes.
	Attributes map[string]*Attribute

	// AttributeOrder preserves the original order of attributes.
	AttributeOrder []string

	// Expr is the underlying CallExpr AST node.
	Expr *syntax.CallExpr

	// span is the original byte range in the source.
	span Span

	// modified tracks whether this target has been modified.
	modified bool

	// modifiedAttrs tracks which attributes have been modified.
	modifiedAttrs map[string]bool
}

// Attribute represents a target attribute (e.g., name = "foo", deps = [...]).
type Attribute struct {
	// Name is the attribute name.
	Name string

	// Value is the attribute value.
	Value AttributeValue

	// Expr is the underlying BinaryExpr AST node (name = value).
	Expr *syntax.BinaryExpr

	// span is the original byte range in the source.
	span Span

	// modified tracks whether this attribute has been modified.
	modified bool
}

// AttributeValue represents the value of an attribute.
// It can be a string, list, dict, bool, int, or identifier.
type AttributeValue interface {
	// Type returns the type of the value.
	Type() AttributeType

	// String returns a string representation for debugging.
	String() string
}

// AttributeType identifies the type of an attribute value.
type AttributeType int

const (
	TypeString AttributeType = iota
	TypeStringList
	TypeBool
	TypeInt
	TypeIdent
	TypeDict
	TypeExpr // Catch-all for complex expressions
)

// StringValue is a string attribute value.
type StringValue struct {
	Value string
}

func (v StringValue) Type() AttributeType { return TypeString }
func (v StringValue) String() string      { return v.Value }

// StringListValue is a list of strings attribute value.
type StringListValue struct {
	Values []string
}

func (v StringListValue) Type() AttributeType { return TypeStringList }
func (v StringListValue) String() string      { return "[...]" }

// DepsValue is a deps list with support for preserved sections.
// It tracks auto-managed deps (between turnkey:auto-start/end markers)
// and preserved deps (between turnkey:preserve-start/end markers).
type DepsValue struct {
	// AutoDeps are dependencies managed by turnkey (can be regenerated).
	AutoDeps []string

	// PreservedDeps are dependencies that should not be modified.
	PreservedDeps []string

	// HasMarkers indicates whether the original had turnkey markers.
	// If false, the entire deps list is treated as auto-managed.
	HasMarkers bool

	// RawDeps is used when there are no markers - the entire list.
	RawDeps []string
}

func (v DepsValue) Type() AttributeType { return TypeStringList }
func (v DepsValue) String() string      { return "[...]" }

// AllDeps returns all deps (auto + preserved) in order.
func (v DepsValue) AllDeps() []string {
	if !v.HasMarkers {
		return v.RawDeps
	}
	result := make([]string, 0, len(v.AutoDeps)+len(v.PreservedDeps))
	result = append(result, v.AutoDeps...)
	result = append(result, v.PreservedDeps...)
	return result
}

// BoolValue is a boolean attribute value.
type BoolValue struct {
	Value bool
}

func (v BoolValue) Type() AttributeType { return TypeBool }
func (v BoolValue) String() string {
	if v.Value {
		return "True"
	}
	return "False"
}

// IntValue is an integer attribute value.
type IntValue struct {
	Value int64
}

func (v IntValue) Type() AttributeType { return TypeInt }
func (v IntValue) String() string      { return "" }

// IdentValue is an identifier attribute value.
type IdentValue struct {
	Name string
}

func (v IdentValue) Type() AttributeType { return TypeIdent }
func (v IdentValue) String() string      { return v.Name }

// ExprValue is a catch-all for complex expressions we don't need to understand.
type ExprValue struct {
	Expr syntax.Expr
	// originalText is the original source text for this expression.
	originalText string
}

func (v ExprValue) Type() AttributeType { return TypeExpr }
func (v ExprValue) String() string      { return v.originalText }

// Span represents a byte range in the source file.
type Span struct {
	Start int // Start byte offset (inclusive)
	End   int // End byte offset (exclusive)
}

// IsModified returns true if the file has been modified.
func (f *File) IsModified() bool {
	if f.modified {
		return true
	}
	for _, t := range f.Targets {
		if t.modified {
			return true
		}
	}
	return false
}

// GetTarget returns the target with the given name, or nil if not found.
func (f *File) GetTarget(name string) *Target {
	for _, t := range f.Targets {
		if t.Name == name {
			return t
		}
	}
	return nil
}

// IsModified returns true if the target has been modified.
func (t *Target) IsModified() bool {
	return t.modified
}

// GetAttribute returns the attribute with the given name, or nil if not found.
func (t *Target) GetAttribute(name string) *Attribute {
	return t.Attributes[name]
}

// GetDeps returns the deps attribute as a list of strings, or nil if not present.
// For DepsValue with markers, it returns all deps (auto + preserved).
func (t *Target) GetDeps() []string {
	attr := t.GetAttribute("deps")
	if attr == nil {
		return nil
	}
	if list, ok := attr.Value.(StringListValue); ok {
		return list.Values
	}
	if deps, ok := attr.Value.(DepsValue); ok {
		return deps.AllDeps()
	}
	return nil
}

// GetAutoDeps returns only the auto-managed deps (for DepsValue with markers).
// For regular StringListValue, returns all deps.
func (t *Target) GetAutoDeps() []string {
	attr := t.GetAttribute("deps")
	if attr == nil {
		return nil
	}
	if deps, ok := attr.Value.(DepsValue); ok {
		if deps.HasMarkers {
			return deps.AutoDeps
		}
		return deps.RawDeps
	}
	if list, ok := attr.Value.(StringListValue); ok {
		return list.Values
	}
	return nil
}

// GetPreservedDeps returns only the preserved deps (for DepsValue with markers).
func (t *Target) GetPreservedDeps() []string {
	attr := t.GetAttribute("deps")
	if attr == nil {
		return nil
	}
	if deps, ok := attr.Value.(DepsValue); ok {
		return deps.PreservedDeps
	}
	return nil
}

// GetStringAttr returns a string attribute value, or empty string if not present.
func (t *Target) GetStringAttr(name string) string {
	attr := t.GetAttribute(name)
	if attr == nil {
		return ""
	}
	if str, ok := attr.Value.(StringValue); ok {
		return str.Value
	}
	return ""
}
