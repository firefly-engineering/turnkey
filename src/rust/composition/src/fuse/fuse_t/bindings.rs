//! Hand-written FFI bindings to libfuse3 (FUSE-T on macOS)
//!
//! Minimal bindings covering the high-level FUSE API:
//! - `fuse_operations` struct (callback table)
//! - Session lifecycle: `fuse_new_30`, `fuse_mount`, `fuse_loop`, `fuse_unmount`, `fuse_destroy`
//! - Context: `fuse_get_context` for accessing private_data in callbacks
//!
//! These bind directly to FUSE-T's libfuse3.dylib, which handles the NFS translation
//! internally via its own session loop.

#![allow(non_camel_case_types, dead_code)]

use std::ffi::c_void;
use std::os::raw::{c_char, c_int};

// Opaque types
pub enum fuse {}
pub enum fuse_pollhandle {}
pub enum fuse_bufvec {}
pub enum fuse_conn_info {}
pub enum fuse_config {}

/// libfuse version struct, passed to fuse_new_30
#[repr(C)]
pub struct libfuse_version {
    pub major: u32,
    pub minor: u32,
    pub hotfix: u32,
    pub padding: u32,
}

/// FUSE file info, passed to most callbacks
#[repr(C)]
pub struct fuse_file_info {
    pub flags: i32,
    // Bitfields in C — represented as a u32 with manual bit access
    pub bitfields: u32,
    pub fh: u64,
    pub lock_owner: u64,
    pub poll_events: u32,
    _padding: u32,
}

/// FUSE context, returned by fuse_get_context()
#[repr(C)]
pub struct fuse_context {
    pub fuse: *mut fuse,
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
    pub pid: libc::pid_t,
    pub private_data: *mut c_void,
    pub umask: libc::mode_t,
}

/// FUSE args for passing mount options
#[repr(C)]
pub struct fuse_args {
    pub argc: c_int,
    pub argv: *mut *mut c_char,
    pub allocated: c_int,
}

/// Readdir flags (passed as c_int)
pub type fuse_readdir_flags = c_int;

/// Fill dir flags (passed as c_int, not a Rust enum, to avoid transmute panics)
pub type fuse_fill_dir_flags = c_int;
pub const FUSE_FILL_DIR_PLUS: fuse_fill_dir_flags = 2;

/// fuse_fill_dir_t callback type for readdir
pub type fuse_fill_dir_t = Option<
    unsafe extern "C" fn(
        buf: *mut c_void,
        name: *const c_char,
        stbuf: *const libc::stat,
        off: libc::off_t,
        flags: fuse_fill_dir_flags,
    ) -> c_int,
>;

/// High-level FUSE operations callback table.
///
/// Field order MUST match the C struct in fuse3/fuse.h exactly.
/// Unused fields are set to None (null function pointer).
#[repr(C)]
pub struct fuse_operations {
    pub getattr: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            stbuf: *mut libc::stat,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub readlink: Option<
        unsafe extern "C" fn(path: *const c_char, buf: *mut c_char, size: libc::size_t) -> c_int,
    >,
    pub mknod: Option<
        unsafe extern "C" fn(path: *const c_char, mode: libc::mode_t, dev: libc::dev_t) -> c_int,
    >,
    pub mkdir: Option<unsafe extern "C" fn(path: *const c_char, mode: libc::mode_t) -> c_int>,
    pub unlink: Option<unsafe extern "C" fn(path: *const c_char) -> c_int>,
    pub rmdir: Option<unsafe extern "C" fn(path: *const c_char) -> c_int>,
    pub symlink: Option<unsafe extern "C" fn(from: *const c_char, to: *const c_char) -> c_int>,
    pub rename: Option<
        unsafe extern "C" fn(
            from: *const c_char,
            to: *const c_char,
            flags: libc::c_uint,
        ) -> c_int,
    >,
    pub link: Option<unsafe extern "C" fn(from: *const c_char, to: *const c_char) -> c_int>,
    pub chmod: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            mode: libc::mode_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub chown: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            uid: libc::uid_t,
            gid: libc::gid_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub truncate: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            size: libc::off_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub open:
        Option<unsafe extern "C" fn(path: *const c_char, fi: *mut fuse_file_info) -> c_int>,
    pub read: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            buf: *mut c_char,
            size: libc::size_t,
            offset: libc::off_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub write: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            buf: *const c_char,
            size: libc::size_t,
            offset: libc::off_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub statfs:
        Option<unsafe extern "C" fn(path: *const c_char, stbuf: *mut libc::statvfs) -> c_int>,
    pub flush:
        Option<unsafe extern "C" fn(path: *const c_char, fi: *mut fuse_file_info) -> c_int>,
    pub release:
        Option<unsafe extern "C" fn(path: *const c_char, fi: *mut fuse_file_info) -> c_int>,
    pub fsync: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            datasync: c_int,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub setxattr: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            name: *const c_char,
            value: *const c_char,
            size: libc::size_t,
            flags: c_int,
        ) -> c_int,
    >,
    pub getxattr: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            name: *const c_char,
            value: *mut c_char,
            size: libc::size_t,
        ) -> c_int,
    >,
    pub listxattr: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            list: *mut c_char,
            size: libc::size_t,
        ) -> c_int,
    >,
    pub removexattr:
        Option<unsafe extern "C" fn(path: *const c_char, name: *const c_char) -> c_int>,
    pub opendir:
        Option<unsafe extern "C" fn(path: *const c_char, fi: *mut fuse_file_info) -> c_int>,
    pub readdir: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            buf: *mut c_void,
            filler: fuse_fill_dir_t,
            offset: libc::off_t,
            fi: *mut fuse_file_info,
            flags: fuse_readdir_flags,
        ) -> c_int,
    >,
    pub releasedir:
        Option<unsafe extern "C" fn(path: *const c_char, fi: *mut fuse_file_info) -> c_int>,
    pub fsyncdir: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            datasync: c_int,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub init: Option<
        unsafe extern "C" fn(conn: *mut fuse_conn_info, cfg: *mut fuse_config) -> *mut c_void,
    >,
    pub destroy: Option<unsafe extern "C" fn(private_data: *mut c_void)>,
    pub access: Option<unsafe extern "C" fn(path: *const c_char, mask: c_int) -> c_int>,
    pub create: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            mode: libc::mode_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub lock: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            fi: *mut fuse_file_info,
            cmd: c_int,
            lock: *mut libc::flock,
        ) -> c_int,
    >,
    pub utimens: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            tv: *const libc::timespec,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub bmap: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            blocksize: libc::size_t,
            idx: *mut u64,
        ) -> c_int,
    >,
    pub ioctl: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            cmd: libc::c_uint,
            arg: *mut c_void,
            fi: *mut fuse_file_info,
            flags: libc::c_uint,
            data: *mut c_void,
        ) -> c_int,
    >,
    pub poll: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            fi: *mut fuse_file_info,
            ph: *mut fuse_pollhandle,
            reventsp: *mut libc::c_uint,
        ) -> c_int,
    >,
    pub write_buf: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            buf: *mut fuse_bufvec,
            off: libc::off_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub read_buf: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            bufp: *mut *mut fuse_bufvec,
            size: libc::size_t,
            off: libc::off_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub flock: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            fi: *mut fuse_file_info,
            op: c_int,
        ) -> c_int,
    >,
    pub fallocate: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            mode: c_int,
            offset: libc::off_t,
            length: libc::off_t,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    pub copy_file_range: Option<
        unsafe extern "C" fn(
            path_in: *const c_char,
            fi_in: *mut fuse_file_info,
            offset_in: libc::off_t,
            path_out: *const c_char,
            fi_out: *mut fuse_file_info,
            offset_out: libc::off_t,
            size: libc::size_t,
            flags: c_int,
        ) -> libc::ssize_t,
    >,
    pub lseek: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            off: libc::off_t,
            whence: c_int,
            fi: *mut fuse_file_info,
        ) -> libc::off_t,
    >,
}

impl fuse_operations {
    /// Create a zeroed fuse_operations struct (all callbacks null)
    pub fn zeroed() -> Self {
        // Safety: fuse_operations is a POD struct of function pointers; all-zeros is valid (all None)
        unsafe { std::mem::zeroed() }
    }
}

// Link against FUSE-T's libfuse3
#[link(name = "fuse3")]
unsafe extern "C" {
    /// Create a new FUSE filesystem instance.
    /// We call the versioned symbol directly since fuse_new() is an inline wrapper.
    #[link_name = "fuse_new_30"]
    pub fn fuse_new(
        args: *mut fuse_args,
        op: *const fuse_operations,
        op_size: libc::size_t,
        version: *mut libfuse_version,
        user_data: *mut c_void,
    ) -> *mut fuse;

    /// Mount the FUSE filesystem at the given mountpoint.
    pub fn fuse_mount(f: *mut fuse, mountpoint: *const c_char) -> c_int;

    /// Run the FUSE event loop (blocks until unmounted).
    pub fn fuse_loop(f: *mut fuse) -> c_int;

    /// Unmount the FUSE filesystem.
    pub fn fuse_unmount(f: *mut fuse);

    /// Destroy the FUSE filesystem and free resources.
    pub fn fuse_destroy(f: *mut fuse);

    /// Get the current FUSE context (uid, gid, pid, private_data).
    pub fn fuse_get_context() -> *mut fuse_context;
}

/// FUSE major version (3)
pub const FUSE_MAJOR_VERSION: u32 = 3;
/// FUSE minor version
pub const FUSE_MINOR_VERSION: u32 = 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuse_operations_zeroed() {
        let ops = fuse_operations::zeroed();
        // All callbacks should be None
        assert!(ops.getattr.is_none());
        assert!(ops.readdir.is_none());
        assert!(ops.init.is_none());
    }
}
