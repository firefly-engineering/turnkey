//! FUSE-T operation callbacks
//!
//! Each callback retrieves FsCore from fuse_get_context()->private_data,
//! converts C paths to Rust, delegates to FsCore, and converts results back.
//!
//! Implemented in turnkey-eii.3.
