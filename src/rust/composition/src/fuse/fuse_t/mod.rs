//! macOS FUSE-T backend via direct libfuse3 FFI
//!
//! Bypasses the `fuser` crate and calls FUSE-T's libfuse3 C API directly,
//! using its session loop (`fuse_loop`) which correctly handles FUSE-T's
//! NFS-based socket protocol.

pub mod bindings;
pub mod backend;
pub mod metrics;
pub mod operations;
