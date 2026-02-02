//! FUSE-based composition backend for Linux and macOS
//!
//! This module provides a FUSE filesystem backend that presents a unified view
//! of the repository with dependency cells mounted at fixed paths.
//!
//! # Platform Support
//!
//! - **Linux**: Uses native FUSE via `/dev/fuse` and `fuser` crate
//! - **macOS**: Uses FUSE-T (NFS-based, no kernel extension required)
//!
//! # FUSE-T on macOS
//!
//! FUSE-T is the recommended FUSE implementation for macOS as it doesn't require
//! a kernel extension (important for Apple Silicon and macOS security). Install with:
//!
//! ```bash
//! brew install macos-fuse-t/homebrew-cask/fuse-t
//! ```
//!
//! # Architecture
//!
//! The FUSE backend runs as a background thread that handles filesystem
//! operations. The main components are:
//!
//! - `FuseBackend`: The public interface implementing `CompositionBackend`
//! - `CompositionFs`: The FUSE filesystem implementation
//! - `platform`: Platform detection and FUSE availability checking
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
pub mod platform;

pub use backend::FuseBackend;
pub use edit_overlay::{EditOverlay, EditedFileInfo};
pub use patch_generator::{PatchGenerator, PatchInfo};
pub use platform::{check_fuse_availability, FuseAvailability, Platform};
