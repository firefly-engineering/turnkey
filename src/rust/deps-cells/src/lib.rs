//! deps-cells: the deps cells under `.turnkey/`
//!
//! A deps cell (`.turnkey/<cell>`, ADR 0004) is materialized by tk from the
//! cell index the shell builds with Nix ([`materialize()`], [`Index`]), and
//! tk warns when the cell lags its deps file ([`stale`]). A cell that is
//! still a symlink into the Nix store changes target when Nix rebuilds it,
//! which buck2's daemon doesn't notice ([`freshness`]). And `tk compose`
//! edits a cell's files into patches ([`edits`]).
//!
//! Nothing here runs a process or reads the environment: tk passes in what
//! roots a store path and what kills buck2's daemon.
//!
//! Ported from Go's materialize and cellfresh packages and the file
//! handling of the Go tk's compose (#216), with the same results.

pub mod edits;
pub mod freshness;
mod fsutil;
mod index;
mod materialize;

pub use fsutil::write_file;
pub use index::{Index, Package, ReadError, ResolveError, Resolved, current_index, read_index};
pub use materialize::{AddRoot, Options, Outcome, materialize, stale};

use std::fmt;
use std::path::{Path, PathBuf};

/// Where a materialized cell's store links are
const STORE_DIR: &str = "_store";
/// The file holding the hash of the deps file a cell was built from
const MARKER_NAME: &str = ".deps-file-sha256";
/// A package's build file
const BUILD_FILE: &str = "rules.star";
/// Where, under `.turnkey/`, the cells' GC roots are
const GCROOTS_DIR: &str = "gcroots";

/// Where a cell lives in a project
pub fn cell_dir(root: &Path, cell: &str) -> PathBuf {
    root.join(".turnkey").join(cell)
}

/// What went wrong, said as the Go version said it
#[derive(Debug)]
pub struct Error(String);

impl Error {
    fn new(message: String) -> Self {
        Error(message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

impl From<ReadError> for Error {
    fn from(e: ReadError) -> Self {
        Error(e.to_string())
    }
}
