// Package goparse provides tools for parsing Go source files directly to extract
// package metadata, imports, and build constraints without relying on 'go list'.
// The mapper (rules sync) reads a tree's build constraint tags and the Go build
// of a configuration with it; buckgen uses its Rust port, src/rust/goparse.
package goparse
