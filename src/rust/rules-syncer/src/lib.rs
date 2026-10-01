//! rules-syncer: the deps of a project's `rules.star` files kept in step with
//! their sources
//!
//! [`sync::Syncer`] walks a project's `rules.star` files (or those under
//! directories git reports changes in), and for each target of a language
//! it knows, resolves what the target needs through the language's
//! [`mapper`] plug-in, in every build configuration of the project, and
//! writes it back with rules-star: a plain list, or `[...] + select({...})`
//! for deps that differ between configurations. [`report`] is what it did,
//! as the rules-sync binary prints it for tk.
//!
//! Ported from Go's mapper, rulessync and rulesreport (#215); tk (Go)
//! still reads the report with src/go/pkg/rulesreport.

pub mod mapper;
pub mod report;
pub mod sync;
