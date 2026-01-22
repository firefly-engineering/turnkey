package goparse

import "go/build/constraint"

// Platform represents a target OS/arch combination
type Platform struct {
	OS   string // e.g., "linux", "darwin", "windows"
	Arch string // e.g., "amd64", "arm64"
}

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

// GoPackage represents aggregated info for a Go package directory
type GoPackage struct {
	Dir        string                // directory path
	ImportPath string                // full import path
	Name       string                // package name
	GoFiles    map[Platform][]string // platform -> source files
	Imports    map[Platform][]string // platform -> import paths
	EmbedDirs  []string              // embed directories (common)
	HasCgo     bool                  // any file has cgo
}

// Common platforms for cross-platform analysis
var DefaultPlatforms = []Platform{
	{OS: "linux", Arch: "amd64"},
	{OS: "linux", Arch: "arm64"},
	{OS: "darwin", Arch: "amd64"},
	{OS: "darwin", Arch: "arm64"},
}
