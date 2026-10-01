//! gostd: ports of the Go standard library behaviours turnkey's generated
//! files depend on
//!
//! turnkey's tools were written in Go, and the files they generate (deps
//! files, `rules.star`) carry the marks of Go's standard library: strings
//! quoted by `strconv.Quote`, paths cleaned by `path.Clean`. A Rust port
//! has to produce the same bytes, so it uses these ports rather than the
//! nearest Rust equivalent, which differs at the edges (`char::escape_debug`
//! is not `strconv.Quote`, `Path::join` replaces on an absolute path where
//! `filepath.Join` concatenates).
//!
//! - [`unicode`]: `unicode.IsPrint`, `unicode.IsSpace`, `unicode.IsLetter`,
//!   `unicode.IsDigit`
//! - [`strconv`]: `strconv.Quote`, `strconv.Unquote` and
//!   `strconv.QuotedPrefix`
//! - [`strings`]: `strings.ToLower` and `unicode.ToLower`
//! - [`path`]: `path.Clean`, `path.Join`, `path.Dir`, `path.Base` and
//!   `filepath.Rel`,
//!   for slash-separated paths (turnkey runs on Linux and macOS only)
//! - [`constraint`]: `go/build/constraint`, the `//go:build` and
//!   `// +build` lines of Go files

pub mod constraint;
pub mod path;
pub mod strconv;
pub mod strings;
pub mod unicode;
