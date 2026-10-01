//! deps-extract's extraction protocol, as a library
//!
//! deps-extract prints a JSON [`extraction::Result`]: the imports it found
//! in each package of a directory. Rules sync (src/rust/rules-syncer) runs
//! it and reads the result back with the same types. The binary compiles
//! the module itself.

pub mod extraction;
