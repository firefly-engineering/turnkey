//! project-sync: a turnkey project's `.turnkey/sync.toml`, and the deps
//! sync it drives
//!
//! A turnkey shell writes `.turnkey/sync.toml` at the top of a project. It
//! says which deps files are generated from which sources, how tools are
//! wrapped, and what rules sync knows of the languages and platforms. This
//! crate is what `tk` and `tw` share of it:
//!
//! - [`config`]: the file itself, and finding the project root;
//! - [`staleness`]: whether a target is older than its sources;
//! - [`syncer`]: regenerating stale targets with their rules' generators;
//! - [`launch`]: starting a child process the way Go's `os/exec` does, so
//!   the processes these tools start see the same argv, cwd and env as
//!   under the Go versions they port.
//!
//! Paths are joined, cleaned and globbed with `gostd::filepath`, as the Go
//! versions do with `path/filepath`.
//!
//! It ports the Go packages `syncconfig`, `staleness` and `syncer`.

pub mod config;
pub mod launch;
pub mod staleness;
pub mod syncer;
