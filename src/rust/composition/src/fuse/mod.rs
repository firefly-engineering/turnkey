//! FUSE-based composition backend for Linux and macOS
//!
//! This module provides a FUSE filesystem backend that presents a unified view
//! of the repository with dependency cells mounted at fixed paths.
//!
//! # Platform Support
//!
//! - **Linux**: native FUSE via `/dev/fuse` and the `fuser` crate.
//! - **macOS**: macFUSE 5.2+ (FSKit on macOS 26+, kext on older releases) via
//!   direct FFI against `/usr/local/lib/libfuse3.4.dylib`. FUSE-T also exposes
//!   the same libfuse3 ABI at the same path, so the FFI-level code accepts
//!   either backend; macFUSE is the project default and what the Nix package
//!   targets. The legacy `fuse_t` module name reflects history, not policy.
//!
//! # macFUSE on macOS
//!
//! Install with:
//!
//! ```bash
//! brew install --cask macfuse
//! ```
//!
//! After install, the FSKit file-system extension must be registered (run the
//! macFUSE app once) and enabled in System Settings > General > Login Items &
//! Extensions > File System Extensions. [`detect_macfuse_backend`] surfaces
//! the activation state at runtime; the FUSE backend pre-checks before
//! `fuse_mount` to avoid hangs on a GUI approval dialog.
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

mod edit_overlay;
pub mod fs_core;
mod patch_generator;
pub mod platform;

// Linux: fuser-based backend (uses fuser crate)
#[cfg(target_os = "linux")]
mod filesystem;
#[cfg(target_os = "linux")]
mod backend;
#[cfg(target_os = "linux")]
pub use backend::FuseBackend;

// macOS: direct libfuse3 FFI backend (macFUSE primary, FUSE-T compatible).
// The module retains its historical `fuse_t` name; the code itself targets the
// upstream libfuse3 ABI and works against either implementation.
#[cfg(target_os = "macos")]
pub mod fuse_t;
#[cfg(target_os = "macos")]
pub use fuse_t::backend::FuseTBackend as FuseBackend;
pub use edit_overlay::{EditOverlay, EditedFileInfo};
pub use patch_generator::{PatchGenerator, PatchInfo};
pub use platform::{check_fuse_availability, FuseAvailability, Platform};
#[cfg(target_os = "macos")]
pub use platform::{detect_macfuse_backend, MacFuseBackend};
