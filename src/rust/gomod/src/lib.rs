//! gomod: go.mod and go.work files, module paths and versions, as
//! `golang.org/x/mod` reads them
//!
//! A port of the parts of `golang.org/x/mod` turnkey's Go tools use, for
//! their Rust ports (#207). No Rust crate reads these files faithfully
//! (quoted module paths, `// indirect;` comments, empty blocks, go.work),
//! so this is a hand-written lexer and parser for the documented grammar,
//! not a regex.
//!
//! - [`syntax`]: the lexer and parser, shared by go.mod and go.work;
//! - [`modfile`]: the directives of a go.mod ([`parse_mod`]) and a
//!   go.work ([`parse_work`]);
//! - [`module`]: module path and version checks, and the module proxy
//!   protocol's case-escaping;
//! - [`semver`]: Go's semantic versions.

pub mod modfile;
pub mod module;
pub mod semver;
pub mod syntax;

pub use modfile::{
    ModFile, Replace, Require, Version, WorkFile, is_directory_path, parse_mod, parse_work,
};
pub use syntax::ErrorList;
