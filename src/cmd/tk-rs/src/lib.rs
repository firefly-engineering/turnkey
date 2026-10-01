//! tk, turnkey's buck2 wrapper, in Rust: the parts ported so far
//!
//! tk runs sync (deps and rules.star) before the buck2 commands that read
//! the build graph, and passes everything else to buck2, rewriting its
//! command line on the way (buck2-args). These modules are its own
//! subcommands and flags; its main, sync and rules sync come with
//! project-sync and the rules-sync library.
//!
//! The Rust port of src/cmd/tk (#216).

pub mod completion;
pub mod compose;
pub mod flags;
pub mod help;
pub mod materialize;
