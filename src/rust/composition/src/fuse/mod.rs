//! FUSE-based composition backend for Linux
//!
//! This module provides a FUSE filesystem backend that presents a unified view
//! of the repository with dependency cells mounted at fixed paths.
//!
//! # Architecture
//!
//! The FUSE backend runs as a background thread that handles filesystem
//! operations. The main components are:
//!
//! - `FuseBackend`: The public interface implementing `CompositionBackend`
//! - `CompositionFs`: The FUSE filesystem implementation
//!
//! # Mount Point Structure
//!
//! ```text
//! /firefly/turnkey/           # Mount point (configurable)
//! ├── src/                    # Pass-through to repository src/
//! └── external/               # Dependency cells
//!     ├── godeps/             # -> Nix store path
//!     ├── rustdeps/           # -> Nix store path
//!     └── ...
//! ```

mod backend;
mod edit_overlay;
mod filesystem;
mod patch_generator;

pub use backend::FuseBackend;
pub use edit_overlay::{EditOverlay, EditedFileInfo};
pub use patch_generator::{PatchGenerator, PatchInfo};
