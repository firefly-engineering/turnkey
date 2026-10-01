//! rules-syncer: the deps of a project's `rules.star` files kept in step with
//! their sources
//!
//! [`sync::Syncer`] walks a project's `rules.star` files (or those under
//! directories git reports changes in), and for each target of a language
//! it knows, resolves what the target needs through the language's
//! [`mapper`] plug-in, in every build configuration of the project, and
//! writes it back with rules-star: a plain list, or `[...] + select({...})`
//! for deps that differ between configurations. [`report`] is what it did,
//! which tk prints.
//!
//! Ported from Go's mapper, rulessync and rulesreport (#215); tk calls it
//! directly (#216).

pub mod mapper;
pub mod report;
pub mod sync;
