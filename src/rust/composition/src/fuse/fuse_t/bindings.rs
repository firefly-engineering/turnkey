//! Hand-written FFI bindings to libfuse3 (macOS userspace libfuse3 ABI)
//!
//! Minimal bindings covering the high-level FUSE API:
//! - `fuse_operations` struct (callback table)
//! - Session lifecycle: `fuse_new_30`, `fuse_mount`, `fuse_loop`, `fuse_unmount`, `fuse_destroy`
//! - Context: `fuse_get_context` for accessing private_data in callbacks
//!
//! On macOS these resolve via the standard libfuse3.4.dylib install path
//! (/usr/local/lib/libfuse3.4.dylib). macFUSE 5.x and FUSE-T both expose the same
//! libfuse3.16 ABI, so the same bindings work against either backend; the runtime
//! choice is determined by which library is installed at /usr/local/lib at link time.
//! On Linux the link resolves against the upstream libfuse3.

#![allow(non_camel_case_types, dead_code)]

use std::ffi::c_void;
use std::os::raw::{c_char, c_int};

// Opaque types
pub enum fuse {}
pub enum fuse_pollhandle {}
pub enum fuse_bufvec {}
pub enum fuse_conn_info {}

/// FUSE configuration, passed to the `init` callback. Layout matches
/// libfuse 3.18's `struct fuse_config` from fuse.h.
///
/// Fields between `auto_cache` and `no_rofd_flush` were added in 3.17/3.18
/// and were missing from our prior 3.16-style bindings — `no_rofd_flush`
/// in particular sat at the wrong offset, so any future read/write would
/// have hit `ac_attr_timeout_set` instead. The `reserved` tail is part of
/// the public ABI and must be preserved at the right size.
#[repr(C)]
pub struct fuse_config {
    pub set_gid: c_int,
    pub gid: libc::c_uint,
    pub set_uid: c_int,
    pub uid: libc::c_uint,
    pub set_mode: c_int,
    pub umask: libc::c_uint,
    pub entry_timeout: f64,
    pub negative_timeout: f64,
    pub attr_timeout: f64,
    pub intr: c_int,
    pub intr_signal: c_int,
    pub remember: c_int,
    pub hard_remove: c_int,
    pub use_ino: c_int,
    pub readdir_ino: c_int,
    pub direct_io: c_int,
    pub kernel_cache: c_int,
    pub auto_cache: c_int,
    pub ac_attr_timeout_set: c_int,
    pub ac_attr_timeout: f64,
    pub nullpath_ok: c_int,
    pub show_help: c_int,
    pub modules: *mut c_char,
    pub debug: c_int,
    pub fmask: libc::c_uint,
    pub dmask: libc::c_uint,
    pub no_rofd_flush: c_int,
    pub parallel_direct_writes: c_int,
    pub flags: libc::c_uint,
    pub reserved: [u64; 48],
}
pub enum fuse_loop_config {}

/// libfuse version struct, passed to fuse_new_31.
///
/// On Apple, the fourth word is a bitfield in macFUSE's
/// `<fuse_common.h>` (around line 1075): bit 0 is
/// `darwin_extensions_enabled` and bits 1..31 are `padding`. Setting bit 0
/// is what tells macFUSE that our `fuse_operations` callbacks use the
/// Darwin-flavored signatures (e.g. `getattr` takes `*fuse_darwin_attr`,
/// not POSIX `*struct stat`). Without it, macFUSE picks the vanilla
/// dispatch path (`fuse_lib_getattr`, not `fuse_lib_getattr$DARWIN`),
/// allocates a `struct stat` for the call, and our 192-byte
/// fuse_darwin_attr write overruns the 144-byte `stat` buffer — corrupts
/// stack memory, only uid/gid happen to land in the right slots.
///
/// Use [`Self::darwin_extensions()`] to construct the version with the
/// bit set; do NOT just initialise `padding: 0` on macOS.
#[repr(C)]
pub struct libfuse_version {
    pub major: u32,
    pub minor: u32,
    pub hotfix: u32,
    /// On Apple this carries the `darwin_extensions_enabled` bit at bit 0.
    /// On Linux it's straight reserved padding.
    pub padding: u32,
}

impl libfuse_version {
    /// Build a version struct that opts into macFUSE's Darwin-flavored
    /// `fuse_operations` signatures. On Linux this is identical to
    /// initialising `padding: 0` — Linux libfuse ignores the bit.
    pub const fn darwin_extensions(major: u32, minor: u32, hotfix: u32) -> Self {
        Self {
            major,
            minor,
            hotfix,
            // bit 0 = darwin_extensions_enabled; bits 1..31 = padding.
            padding: if cfg!(target_os = "macos") { 1 } else { 0 },
        }
    }
}

/// Two-word time spec used inside `fuse_darwin_attr`. Matches Darwin's
/// `struct timespec { __darwin_time_t tv_sec; long tv_nsec; }` where both
/// fields are 64-bit on 64-bit platforms.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct fuse_darwin_timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// macFUSE's stat replacement passed to the Darwin-flavored `getattr`,
/// `setattr`, and `readdir` filler callbacks. Defined in
/// `/usr/local/include/fuse3/fuse_common.h`. Layout differs from POSIX
/// `struct stat` — most notably `ino` is at offset 0 (not 8), there is no
/// `dev` field, and times are full `timespec` rather than `time_t`.
///
/// Total size: 192 bytes (verified by layout test).
#[repr(C)]
pub struct fuse_darwin_attr {
    pub ino: u64,
    pub mode: libc::mode_t,
    pub nlink: libc::nlink_t,
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
    pub rdev: libc::dev_t,
    // Rust's #[repr(C)] inserts 4 bytes of trailing padding here so that
    // atimespec (alignment 8) lands at offset 24 — matching C.
    pub atimespec: fuse_darwin_timespec,
    pub mtimespec: fuse_darwin_timespec,
    pub ctimespec: fuse_darwin_timespec,
    /// Birth time (file creation).
    pub btimespec: fuse_darwin_timespec,
    /// Last backup time.
    pub bkuptimespec: fuse_darwin_timespec,
    pub size: libc::off_t,
    pub blocks: libc::blkcnt_t,
    pub blksize: libc::blksize_t,
    pub flags: libc::c_uint,
    pub reserved: [u64; 8],
}

impl fuse_darwin_attr {
    /// Zero-initialize a fuse_darwin_attr (all fields 0 / default).
    pub fn zeroed() -> Self {
        // Safety: POD struct, all-zeros is a valid representation.
        unsafe { std::mem::zeroed() }
    }
}

/// FUSE file info, passed to most callbacks. Layout follows libfuse 3.18's
/// `struct fuse_file_info` from fuse_common.h.
///
/// The bitfields in C (`writepage`, `direct_io`, `keep_cache`, `flush`,
/// `nonseekable`, `flock_release`, `cache_readdir`, `noflush`,
/// `parallel_direct_writes`, plus 23 bits of padding) are packed into a
/// single `u32` here, accessed via the bit-helper methods below. The two
/// `padding2`/`padding3` words are part of the public ABI and must be
/// preserved; in 3.16 they didn't exist, so a 3.16-compiled struct's `fh`
/// landed at offset 8 — this layout puts it at offset 16 (the 3.18 ABI),
/// which is what macFUSE 5.2 (libfuse 3.18.2) expects.
#[repr(C)]
pub struct fuse_file_info {
    pub flags: i32,
    pub bitfields: u32,
    pub padding2: u32,
    pub padding3: u32,
    pub fh: u64,
    pub lock_owner: u64,
    pub poll_events: u32,
    pub backing_id: i32,
    pub compat_flags: u64,
    pub reserved: [u64; 2],
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
///
/// On macOS this struct includes four Apple-only fields that aren't part
/// of upstream libfuse: `setattr` after `getattr`, plus `chflags` /
/// `setvolname` / `monitor` between `lseek` and `statx`. macFUSE's headers
/// add these via `#ifdef __APPLE__`, so any binary linking against
/// macFUSE's libfuse3 must match this layout — otherwise every callback
/// past `getattr` is at the wrong offset and libfuse mis-dispatches (e.g.
/// `OPEN` ends up calling our `read`). The fields stay null because we
/// don't implement them. The `fuse_t` module is already macOS-only at the
/// parent (`#[cfg(target_os = "macos")] pub mod fuse_t;`), so no per-field
/// gating is needed here.
#[repr(C)]
pub struct fuse_operations {
    /// On macFUSE the second argument is `*mut fuse_darwin_attr`, NOT POSIX
    /// `*mut libc::stat` — the layouts differ (notably `ino` is at offset 0
    /// here, vs `st_ino` at 8 in libc::stat). Filling a stat into this slot
    /// produces nonsensical attrs to libfuse and fails LOOKUP/GETATTR.
    pub getattr: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            stbuf: *mut fuse_darwin_attr,
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
    /// Apple-only: set file attributes. Not implemented; kept for ABI parity.
    pub setattr: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            attr: *mut fuse_darwin_attr,
            to_set: c_int,
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
    /// Apple-only: set BSD file flags. Not implemented; null for ABI parity.
    pub chflags: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            fi: *mut fuse_file_info,
            flags: libc::c_uint,
        ) -> c_int,
    >,
    /// Apple-only: rename the mounted volume. Not implemented; null for ABI parity.
    pub setvolname: Option<unsafe extern "C" fn(name: *const c_char) -> c_int>,
    /// Apple-only: notify of file watcher count changes (FUSE_MONITOR_BEGIN/END).
    /// Not implemented; null for ABI parity.
    pub monitor: Option<unsafe extern "C" fn(path: *const c_char, op: u32)>,
    pub statx: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            flags: c_int,
            mask: c_int,
            stxbuf: *mut c_void, // struct statx*
            fi: *mut fuse_file_info,
        ) -> c_int,
    >,
}

impl fuse_operations {
    /// Create a zeroed fuse_operations struct (all callbacks null)
    pub fn zeroed() -> Self {
        // Safety: fuse_operations is a POD struct of function pointers; all-zeros is valid (all None)
        unsafe { std::mem::zeroed() }
    }
}

// Link against libfuse3 (macFUSE on macOS, libfuse3 on Linux).
#[link(name = "fuse3")]
unsafe extern "C" {
    /// Create a new FUSE filesystem instance.
    ///
    /// `_fuse_new_31` (underscore prefix) is the real entry point that
    /// honours the `version` arg — including the macFUSE-specific
    /// `darwin_extensions_enabled` bit. The unprefixed `fuse_new_31`
    /// symbol exported by libfuse is a legacy ABI compat shim with
    /// signature `(args, op, op_size, user_data)` that internally
    /// constructs `version = { 0 }` and discards anything we passed.
    /// Linking the wrong symbol means the version struct never reaches
    /// libfuse, the Darwin code path is never selected, and our
    /// `fuse_darwin_attr` writes overrun the smaller `struct stat`
    /// buffer — only uid/gid happened to land at right offsets.
    #[link_name = "_fuse_new_31"]
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

    /// Run the multi-threaded FUSE event loop (blocks until unmounted).
    /// clone_fd: 0 = share fd, 1 = clone fd per thread
    #[link_name = "fuse_loop_mt_31"]
    pub fn fuse_loop_mt(f: *mut fuse, clone_fd: c_int) -> c_int;

    /// Create a FUSE loop configuration.
    pub fn fuse_loop_cfg_create() -> *mut fuse_loop_config;

    /// Destroy a FUSE loop configuration.
    pub fn fuse_loop_cfg_destroy(config: *mut fuse_loop_config);

    /// Set the maximum number of threads.
    pub fn fuse_loop_cfg_set_max_threads(config: *mut fuse_loop_config, max: libc::c_uint);

    /// Set the number of idle threads to keep.
    pub fn fuse_loop_cfg_set_idle_threads(config: *mut fuse_loop_config, idle: libc::c_uint);

    /// Unmount the FUSE filesystem.
    pub fn fuse_unmount(f: *mut fuse);

    /// Destroy the FUSE filesystem and free resources.
    pub fn fuse_destroy(f: *mut fuse);

    /// Get the current FUSE context (uid, gid, pid, private_data).
    pub fn fuse_get_context() -> *mut fuse_context;
}

/// FUSE major version (3)
pub const FUSE_MAJOR_VERSION: u32 = 3;
/// FUSE minor version (matches macFUSE 5.2.0's libfuse 3.18.2; FUSE-T's
/// libfuse3 also implements the 3.18 ABI).
pub const FUSE_MINOR_VERSION: u32 = 18;

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

    #[test]
    fn test_fuse_operations_size() {
        // macFUSE-on-Apple fuse_operations is 47 function pointers = 376 bytes:
        // upstream's 43 fields plus four Apple-only entries (setattr, chflags,
        // setvolname, monitor). Vanilla upstream is 344 bytes; matching that
        // here mis-dispatches every callback past getattr — see the struct
        // doc-comment for details.
        let rust_size = std::mem::size_of::<fuse_operations>();
        assert_eq!(rust_size, 376, "fuse_operations size mismatch: Rust={rust_size} expected=376");
    }

    #[test]
    fn test_fuse_operations_field_offsets() {
        let base = std::ptr::null::<fuse_operations>();
        unsafe {
            // macFUSE-on-Apple offsets. setattr inserted at slot 1 shifts every
            // subsequent field by 8 bytes vs. upstream; chflags/setvolname/monitor
            // before statx push statx from 336 to 360.
            assert_eq!(std::ptr::addr_of!((*base).getattr) as usize, 0, "getattr");
            assert_eq!(std::ptr::addr_of!((*base).setattr) as usize, 8, "setattr (Apple)");
            assert_eq!(std::ptr::addr_of!((*base).readlink) as usize, 16, "readlink");
            assert_eq!(std::ptr::addr_of!((*base).open) as usize, 104, "open");
            assert_eq!(std::ptr::addr_of!((*base).read) as usize, 112, "read");
            assert_eq!(std::ptr::addr_of!((*base).statfs) as usize, 128, "statfs");
            assert_eq!(std::ptr::addr_of!((*base).opendir) as usize, 192, "opendir");
            assert_eq!(std::ptr::addr_of!((*base).readdir) as usize, 200, "readdir");
            assert_eq!(std::ptr::addr_of!((*base).releasedir) as usize, 208, "releasedir");
            assert_eq!(std::ptr::addr_of!((*base).init) as usize, 224, "init");
            assert_eq!(std::ptr::addr_of!((*base).destroy) as usize, 232, "destroy");
            assert_eq!(std::ptr::addr_of!((*base).chflags) as usize, 344, "chflags (Apple)");
            assert_eq!(std::ptr::addr_of!((*base).setvolname) as usize, 352, "setvolname (Apple)");
            assert_eq!(std::ptr::addr_of!((*base).monitor) as usize, 360, "monitor (Apple)");
            assert_eq!(std::ptr::addr_of!((*base).statx) as usize, 368, "statx");
        }
    }

    #[test]
    fn test_fuse_file_info_layout() {
        // libfuse 3.18 layout: flags(4) bitfields(4) padding2(4) padding3(4)
        // fh(8) lock_owner(8) poll_events(4) backing_id(4) compat_flags(8)
        // reserved[2](16) = 64 bytes total.
        assert_eq!(std::mem::size_of::<fuse_file_info>(), 64);
        let base = std::ptr::null::<fuse_file_info>();
        unsafe {
            assert_eq!(std::ptr::addr_of!((*base).flags) as usize, 0);
            assert_eq!(std::ptr::addr_of!((*base).bitfields) as usize, 4);
            assert_eq!(std::ptr::addr_of!((*base).fh) as usize, 16, "fh must be at 16, not 8");
            assert_eq!(std::ptr::addr_of!((*base).lock_owner) as usize, 24);
            assert_eq!(std::ptr::addr_of!((*base).poll_events) as usize, 32);
            assert_eq!(std::ptr::addr_of!((*base).backing_id) as usize, 36);
            assert_eq!(std::ptr::addr_of!((*base).compat_flags) as usize, 40);
        }
    }

    #[test]
    fn test_fuse_darwin_attr_layout() {
        // Layout per /usr/local/include/fuse3/fuse_common.h.
        // Total size: 192 bytes. Critical: ino@0 (vs stat's st_ino@8) and
        // no st_dev — the divergence from libc::stat is what made our
        // pre-port LOOKUP responses unintelligible to macFUSE.
        assert_eq!(std::mem::size_of::<fuse_darwin_attr>(), 192);
        let base = std::ptr::null::<fuse_darwin_attr>();
        unsafe {
            assert_eq!(std::ptr::addr_of!((*base).ino) as usize, 0);
            assert_eq!(std::ptr::addr_of!((*base).mode) as usize, 8);
            assert_eq!(std::ptr::addr_of!((*base).nlink) as usize, 10);
            assert_eq!(std::ptr::addr_of!((*base).uid) as usize, 12);
            assert_eq!(std::ptr::addr_of!((*base).gid) as usize, 16);
            assert_eq!(std::ptr::addr_of!((*base).rdev) as usize, 20);
            // 4 bytes of padding here to align timespec
            assert_eq!(std::ptr::addr_of!((*base).atimespec) as usize, 24);
            assert_eq!(std::ptr::addr_of!((*base).mtimespec) as usize, 40);
            assert_eq!(std::ptr::addr_of!((*base).ctimespec) as usize, 56);
            assert_eq!(std::ptr::addr_of!((*base).btimespec) as usize, 72);
            assert_eq!(std::ptr::addr_of!((*base).bkuptimespec) as usize, 88);
            assert_eq!(std::ptr::addr_of!((*base).size) as usize, 104);
            assert_eq!(std::ptr::addr_of!((*base).blocks) as usize, 112);
            assert_eq!(std::ptr::addr_of!((*base).blksize) as usize, 120);
            assert_eq!(std::ptr::addr_of!((*base).flags) as usize, 124);
            assert_eq!(std::ptr::addr_of!((*base).reserved) as usize, 128);
        }
    }

    #[test]
    fn test_fuse_config_critical_offsets() {
        // libfuse 3.18 fuse_config: key fields we read/write in fuse_init
        // must land at the right C offsets. These were correct in 3.16 too,
        // but verify we didn't perturb them while extending the tail.
        let base = std::ptr::null::<fuse_config>();
        unsafe {
            assert_eq!(std::ptr::addr_of!((*base).entry_timeout) as usize, 24);
            assert_eq!(std::ptr::addr_of!((*base).negative_timeout) as usize, 32);
            assert_eq!(std::ptr::addr_of!((*base).attr_timeout) as usize, 40);
            assert_eq!(std::ptr::addr_of!((*base).kernel_cache) as usize, 76);
            assert_eq!(std::ptr::addr_of!((*base).auto_cache) as usize, 80);
            // Fields below were absent / wrong in our 3.16 layout.
            assert_eq!(std::ptr::addr_of!((*base).ac_attr_timeout_set) as usize, 84);
            assert_eq!(std::ptr::addr_of!((*base).ac_attr_timeout) as usize, 88);
            assert_eq!(std::ptr::addr_of!((*base).no_rofd_flush) as usize, 124);
        }
    }
}
