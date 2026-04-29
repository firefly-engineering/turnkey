//! FUSE-T operation callbacks
//!
//! Each callback retrieves FsCore from fuse_get_context()->private_data,
//! converts C paths to Rust, delegates to FsCore, and converts results back.

#![cfg(target_os = "macos")]
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_void, CStr, CString};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::raw::{c_char, c_int};
use std::os::unix::fs::MetadataExt;
use std::ptr;
use std::time::Instant;

use super::bindings;
use super::metrics;
use crate::fuse::fs_core::{FsCore, ResolvedPath, VirtualFile};

/// Global pointer to the FsCore instance.
///
/// FUSE-T's `fuse_get_context()->private_data` does not reliably point to
/// our user_data, so we use a global instead. This is safe because only one
/// FUSE session runs per process.
static CORE_PTR: std::sync::atomic::AtomicPtr<FsCore> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Set the global FsCore pointer. Must be called before fuse_loop.
pub(crate) fn set_core(core: *const FsCore) {
    CORE_PTR.store(core as *mut FsCore, std::sync::atomic::Ordering::Release);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Retrieve `&FsCore` from the global pointer.
///
/// # Safety
/// Must only be called after `set_core` has been called with a valid pointer
/// that remains valid for the duration of the FUSE session.
unsafe fn get_core<'a>() -> &'a FsCore {
    &*CORE_PTR.load(std::sync::atomic::Ordering::Acquire)
}

/// Convert a C path pointer to a Rust `&str`.
///
/// # Safety
/// `path` must be a valid null-terminated C string.
unsafe fn path_str<'a>(path: *const c_char) -> Result<&'a str, c_int> {
    CStr::from_ptr(path).to_str().map_err(|_| -libc::EINVAL)
}

/// Get current time as seconds since epoch.
fn now_secs() -> libc::time_t {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0)
}

/// Build a `fuse_darwin_timespec` from a Unix epoch second value.
fn ts(secs: libc::time_t) -> bindings::fuse_darwin_timespec {
    bindings::fuse_darwin_timespec {
        tv_sec: secs as i64,
        tv_nsec: 0,
    }
}

/// Fill a `fuse_darwin_attr` buffer with directory attributes.
unsafe fn fill_dir_stat(stbuf: *mut bindings::fuse_darwin_attr, ino: u64, uid: u32, gid: u32) {
    let now = now_secs();
    (*stbuf).ino = ino;
    (*stbuf).mode = libc::S_IFDIR | 0o755;
    (*stbuf).nlink = 2;
    (*stbuf).size = 4096;
    (*stbuf).uid = uid;
    (*stbuf).gid = gid;
    (*stbuf).blksize = 4096;
    (*stbuf).blocks = 8;
    (*stbuf).atimespec = ts(now);
    (*stbuf).mtimespec = ts(now);
    (*stbuf).ctimespec = ts(now);
}

/// Fill a `fuse_darwin_attr` buffer with virtual-file attributes.
unsafe fn fill_virtual_file_stat(
    stbuf: *mut bindings::fuse_darwin_attr,
    ino: u64,
    size: u64,
    uid: u32,
    gid: u32,
) {
    let now = now_secs();
    (*stbuf).ino = ino;
    (*stbuf).mode = libc::S_IFREG | 0o444;
    (*stbuf).nlink = 1;
    (*stbuf).size = size as libc::off_t;
    (*stbuf).uid = uid;
    (*stbuf).gid = gid;
    (*stbuf).blksize = 4096;
    (*stbuf).atimespec = ts(now);
    (*stbuf).mtimespec = ts(now);
    (*stbuf).ctimespec = ts(now);
}

/// Fill a `fuse_darwin_attr` buffer with symlink attributes.
unsafe fn fill_symlink_stat(
    stbuf: *mut bindings::fuse_darwin_attr,
    ino: u64,
    target_len: usize,
    uid: u32,
    gid: u32,
) {
    let now = now_secs();
    (*stbuf).ino = ino;
    (*stbuf).mode = libc::S_IFLNK | 0o777;
    (*stbuf).nlink = 1;
    (*stbuf).size = target_len as libc::off_t;
    (*stbuf).uid = uid;
    (*stbuf).gid = gid;
    (*stbuf).atimespec = ts(now);
    (*stbuf).mtimespec = ts(now);
    (*stbuf).ctimespec = ts(now);
}

/// Fill a `fuse_darwin_attr` buffer from `fs::Metadata`. Note macFUSE's
/// attr struct has no `dev` field (only `rdev`); we drop the device id.
unsafe fn fill_stat_from_metadata(stbuf: *mut bindings::fuse_darwin_attr, meta: &fs::Metadata) {
    (*stbuf).ino = meta.ino();
    (*stbuf).mode = meta.mode() as libc::mode_t;
    (*stbuf).nlink = meta.nlink() as libc::nlink_t;
    (*stbuf).uid = meta.uid();
    (*stbuf).gid = meta.gid();
    (*stbuf).rdev = meta.rdev() as libc::dev_t;
    (*stbuf).size = meta.size() as libc::off_t;
    (*stbuf).blksize = meta.blksize() as libc::blksize_t;
    (*stbuf).blocks = meta.blocks() as libc::blkcnt_t;
    (*stbuf).atimespec = ts(meta.atime() as libc::time_t);
    (*stbuf).mtimespec = ts(meta.mtime() as libc::time_t);
    (*stbuf).ctimespec = ts(meta.ctime() as libc::time_t);
}


// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

/// Scope guard that records timing on drop.
struct TimedOp {
    start: Instant,
    op: fn(Instant),
}

impl Drop for TimedOp {
    fn drop(&mut self) {
        (self.op)(self.start);
    }
}

macro_rules! timed {
    ($op:ident) => {
        let _timer = TimedOp { start: Instant::now(), op: metrics::$op };
    };
}

unsafe extern "C" fn fuse_getattr(
    path: *const c_char,
    stbuf: *mut bindings::fuse_darwin_attr,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(getattr);
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Zero the stat buffer
    ptr::write_bytes(stbuf, 0, 1);

    let resolved = core.resolve_path(path);
    match resolved {
        ResolvedPath::Root => {
            fill_dir_stat(stbuf, 1, core.uid, core.gid);
            0
        }
        ResolvedPath::Source => {
            fill_dir_stat(stbuf, 2, core.uid, core.gid);
            0
        }
        ResolvedPath::CellPrefix => {
            fill_dir_stat(stbuf, 3, core.uid, core.gid);
            0
        }
        ResolvedPath::VirtualFile { file } => {
            let ino = match file {
                VirtualFile::BuckConfig => 4,
                VirtualFile::BuckRoot => 5,
                VirtualFile::Envrc => 6,
                VirtualFile::RootCellBuckConfig => 7,
            };
            let content = core.get_virtual_file_content(file);
            fill_virtual_file_stat(stbuf, ino, content.len() as u64, core.uid, core.gid);
            0
        }
        ResolvedPath::Cell { real_path, .. } => {
            // Expose cells as symlinks to their real Nix store paths.
            // This allows the kernel to follow the symlink and read cell
            // contents directly from the store, bypassing FUSE entirely.
            let target = real_path.to_string_lossy();
            fill_symlink_stat(stbuf, 100, target.len(), core.uid, core.gid);
            0
        }
        ResolvedPath::OutputMount { real_path, symlink: true }
        | ResolvedPath::OutputChild { real_path, symlink: true } => {
            // Symlink output mounts (e.g., bin/) bypass FUSE
            let target = real_path.to_string_lossy();
            fill_symlink_stat(stbuf, 101, target.len(), core.uid, core.gid);
            0
        }
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::CellChild { real_path, .. }
        | ResolvedPath::OutputMount { real_path, .. }
        | ResolvedPath::OutputChild { real_path, .. } => {
            match fs::symlink_metadata(&real_path) {
                Ok(meta) => {
                    fill_stat_from_metadata(stbuf, &meta);
                    0
                }
                Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        }
        ResolvedPath::NotFound => -libc::ENOENT,
    }
}

unsafe extern "C" fn fuse_readdir(
    path: *const c_char,
    buf: *mut c_void,
    filler: bindings::fuse_fill_dir_t,
    _offset: libc::off_t,
    _fi: *mut bindings::fuse_file_info,
    _flags: bindings::fuse_readdir_flags,
) -> c_int {
    timed!(readdir);
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // macFUSE's FSKit dispatcher can invoke readdir with a null filler
    // (the libfuse signature is `Option<fn>`). Bail out cleanly rather than
    // panicking on unwrap.
    let Some(filler_fn) = filler else {
        return 0;
    };
    let fill = |name: &str| {
        if let Ok(cname) = CString::new(name) {
            filler_fn(buf, cname.as_ptr(), ptr::null(), 0, 0);
        }
    };

    fill(".");
    fill("..");

    match core.resolve_path(path) {
        ResolvedPath::Root => {
            fill(&core.config.source_dir_name);
            fill(&core.config.cell_prefix);
            fill(".buckconfig");
            fill(".buckroot");
            fill(".envrc");
            for om in &core.config.output_mounts {
                fill(&om.mount_as);
            }
            0
        }
        ResolvedPath::Source => {
            // Virtual .buckconfig for the root cell (sets buildfile name)
            fill(".buckconfig");
            match fs::read_dir(&core.repo_root) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            // Skip the real .buckconfig (virtual one takes precedence)
                            if name == ".buckconfig" {
                                continue;
                            }
                            if !core.is_excluded(name) {
                                fill(name);
                            }
                        }
                    }
                    0
                }
                Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        }
        ResolvedPath::CellPrefix => {
            let cell_paths = core.cell_paths.read().unwrap();
            for name in cell_paths.keys() {
                fill(name);
            }
            0
        }
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::Cell { real_path, .. }
        | ResolvedPath::CellChild { real_path, .. }
        | ResolvedPath::OutputMount { real_path, .. }
        | ResolvedPath::OutputChild { real_path, .. } => {
            match fs::read_dir(&real_path) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            fill(name);
                        }
                    }
                    0
                }
                Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        }
        ResolvedPath::VirtualFile { .. } => -libc::ENOTDIR,
        ResolvedPath::NotFound => -libc::ENOENT,
    }
}

unsafe extern "C" fn fuse_open(
    _path: *const c_char,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(open);
    0
}

unsafe extern "C" fn fuse_opendir(
    _path: *const c_char,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(opendir);
    0
}

unsafe extern "C" fn fuse_releasedir(
    _path: *const c_char,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(releasedir);
    0
}

unsafe extern "C" fn fuse_read(
    path: *const c_char,
    buf: *mut c_char,
    size: libc::size_t,
    offset: libc::off_t,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(read);
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match core.resolve_path(path) {
        ResolvedPath::VirtualFile { file } => {
            let content = core.get_virtual_file_content(file);
            let bytes = content.as_bytes();
            let off = offset as usize;
            if off >= bytes.len() {
                return 0;
            }
            let available = &bytes[off..];
            let to_copy = available.len().min(size);
            ptr::copy_nonoverlapping(available.as_ptr(), buf as *mut u8, to_copy);
            to_copy as c_int
        }
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::CellChild { real_path, .. }
        | ResolvedPath::OutputChild { real_path, .. } => {
            let mut file = match std::fs::File::open(&real_path) {
                Ok(f) => f,
                Err(e) => return -(e.raw_os_error().unwrap_or(libc::EIO)),
            };
            if let Err(e) = file.seek(SeekFrom::Start(offset as u64)) {
                return -(e.raw_os_error().unwrap_or(libc::EIO));
            }
            let slice = std::slice::from_raw_parts_mut(buf as *mut u8, size);
            match file.read(slice) {
                Ok(n) => n as c_int,
                Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        }
        ResolvedPath::Root
        | ResolvedPath::Source
        | ResolvedPath::CellPrefix
        | ResolvedPath::Cell { .. }
        | ResolvedPath::OutputMount { .. } => -libc::EISDIR,
        ResolvedPath::NotFound => -libc::ENOENT,
    }
}

/// Copy a symlink target into libfuse's caller buffer, NUL-terminating.
/// Returns the FUSE callback's int result (0 on success).
///
/// Saturates to the buffer's last byte so we never overrun. `size == 0`
/// is treated as a no-op success (libfuse should never pass it, but a
/// underflow on `size - 1` would otherwise trip the kernel stack canary
/// inside libfuse's PATH_MAX-sized linkname buffer).
unsafe fn copy_link_target(buf: *mut c_char, size: libc::size_t, target: &[u8]) -> c_int {
    if size == 0 {
        return 0;
    }
    let to_copy = target.len().min(size - 1);
    ptr::copy_nonoverlapping(target.as_ptr(), buf as *mut u8, to_copy);
    *buf.add(to_copy) = 0;
    0
}

unsafe extern "C" fn fuse_readlink(
    path: *const c_char,
    buf: *mut c_char,
    size: libc::size_t,
) -> c_int {
    timed!(readlink);
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match core.resolve_path(path) {
        ResolvedPath::Cell { real_path, .. } => {
            copy_link_target(buf, size, real_path.as_os_str().as_encoded_bytes())
        }
        ResolvedPath::OutputMount { real_path, symlink: true }
        | ResolvedPath::OutputChild { real_path, symlink: true } => {
            copy_link_target(buf, size, real_path.as_os_str().as_encoded_bytes())
        }
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::CellChild { real_path, .. }
        | ResolvedPath::OutputChild { real_path, .. } => {
            match fs::read_link(&real_path) {
                Ok(target) => copy_link_target(buf, size, target.as_os_str().as_encoded_bytes()),
                Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        }
        ResolvedPath::NotFound => -libc::ENOENT,
        _ => -libc::EINVAL,
    }
}

unsafe extern "C" fn fuse_statfs(
    _path: *const c_char,
    stbuf: *mut libc::statvfs,
) -> c_int {
    timed!(statfs);
    ptr::write_bytes(stbuf, 0, 1);
    (*stbuf).f_bsize = 512;
    (*stbuf).f_frsize = 512;
    (*stbuf).f_blocks = 0;
    (*stbuf).f_bfree = 0;
    (*stbuf).f_bavail = 0;
    (*stbuf).f_files = 0;
    (*stbuf).f_ffree = 0;
    (*stbuf).f_favail = 0;
    (*stbuf).f_namemax = 255;
    0
}

unsafe extern "C" fn fuse_init(
    _conn: *mut bindings::fuse_conn_info,
    cfg: *mut bindings::fuse_config,
) -> *mut c_void {
    // Set kernel cache timeouts to reduce NFS round-trips.
    // Source files and virtual configs don't change during a build,
    // so aggressive caching is safe.
    if !cfg.is_null() {
        (*cfg).entry_timeout = 300.0;    // cache name lookups for 5 min
        (*cfg).attr_timeout = 300.0;     // cache file attributes for 5 min
        // negative_timeout MUST be 0 on macFUSE FSKit. With a non-zero
        // value, libfuse converts ENOENT lookups into "negative entry"
        // replies with ino=0 (a Linux kernel convention for caching
        // negative dentries — see fuse_lib_lookup$DARWIN in macFUSE's
        // fuse.c around line 3255). FSKit doesn't honor that convention:
        // it treats ino=0 as a real node and follows up with a GETATTR
        // for nodeid=0, which makes libfuse's get_node() abort with
        // "fuse internal error: node 0 not found".
        (*cfg).negative_timeout = 0.0;
        (*cfg).kernel_cache = 1;         // allow kernel to cache file contents
        (*cfg).auto_cache = 1;           // invalidate cache when mtime changes
    }

    let ctx = bindings::fuse_get_context();
    (*ctx).private_data
}

unsafe extern "C" fn fuse_destroy(_private_data: *mut c_void) {
    metrics::report();
}

/// Get the real path for a writable resolved path, or return EROFS
fn writable_real_path(resolved: &ResolvedPath) -> Result<&std::path::Path, c_int> {
    match resolved {
        ResolvedPath::OutputMount { real_path, symlink: false }
        | ResolvedPath::OutputChild { real_path, symlink: false } => Ok(real_path),
        _ => Err(-libc::EROFS),
    }
}

unsafe extern "C" fn fuse_mkdir(
    path: *const c_char,
    _mode: libc::mode_t,
) -> c_int {
    timed!(mkdir);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };
    match std::fs::create_dir(real) {
        Ok(()) => 0,
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_unlink(path: *const c_char) -> c_int {
    timed!(unlink);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };
    match std::fs::remove_file(real) {
        Ok(()) => 0,
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_rmdir(path: *const c_char) -> c_int {
    timed!(rmdir);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };
    match std::fs::remove_dir(real) {
        Ok(()) => 0,
        Err(e) if e.raw_os_error() == Some(libc::ENOTEMPTY) => {
            // FUSE-T NFS caching can leave stale entries (e.g., .DS_Store,
            // ._ AppleDouble files). Clean them up and retry.
            if let Ok(entries) = std::fs::read_dir(real) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        std::fs::remove_dir_all(&p).ok();
                    } else {
                        std::fs::remove_file(&p).ok();
                    }
                }
            }
            match std::fs::remove_dir(real) {
                Ok(()) => 0,
                Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
            }
        }
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_create(
    path: *const c_char,
    mode: libc::mode_t,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(create);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };
    match std::fs::File::create(real) {
        Ok(_) => {
            // Set the requested permissions
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(mode as u32);
                std::fs::set_permissions(real, perms).ok();
            }
            0
        }
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_write(
    path: *const c_char,
    buf: *const c_char,
    size: libc::size_t,
    offset: libc::off_t,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(write);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };

    use std::io::Write;
    let mut file = match std::fs::OpenOptions::new().write(true).open(real) {
        Ok(f) => f,
        Err(e) => return -(e.raw_os_error().unwrap_or(libc::EIO)),
    };
    if let Err(e) = file.seek(SeekFrom::Start(offset as u64)) {
        return -(e.raw_os_error().unwrap_or(libc::EIO));
    }
    let data = std::slice::from_raw_parts(buf as *const u8, size);
    match file.write(data) {
        Ok(n) => n as c_int,
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_truncate(
    path: *const c_char,
    size: libc::off_t,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(truncate);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };
    match std::fs::File::options().write(true).open(real) {
        Ok(f) => match f.set_len(size as u64) {
            Ok(()) => 0,
            Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
        },
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_chmod(
    path: *const c_char,
    mode: libc::mode_t,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    timed!(chmod);
    let core = get_core();
    let path = match path_str(path) { Ok(p) => p, Err(e) => return e };
    let resolved = core.resolve_path(path);
    let real = match writable_real_path(&resolved) { Ok(p) => p, Err(e) => return e };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode as u32);
        match std::fs::set_permissions(real, perms) {
            Ok(()) => 0,
            Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
        }
    }
    #[cfg(not(unix))]
    { 0 }
}

unsafe extern "C" fn fuse_rename(
    from: *const c_char,
    to: *const c_char,
    _flags: libc::c_uint,
) -> c_int {
    timed!(rename);
    let core = get_core();
    let from_path = match path_str(from) { Ok(p) => p, Err(e) => return e };
    let to_path = match path_str(to) { Ok(p) => p, Err(e) => return e };
    let from_resolved = core.resolve_path(from_path);
    let to_resolved = core.resolve_path(to_path);
    let from_real = match writable_real_path(&from_resolved) { Ok(p) => p, Err(e) => return e };
    let to_real = match writable_real_path(&to_resolved) { Ok(p) => p, Err(e) => return e };
    match std::fs::rename(from_real, to_real) {
        Ok(()) => 0,
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_symlink(
    from: *const c_char,
    to: *const c_char,
) -> c_int {
    timed!(symlink);
    let core = get_core();
    let to_path = match path_str(to) { Ok(p) => p, Err(e) => return e };
    let to_resolved = core.resolve_path(to_path);
    let to_real = match writable_real_path(&to_resolved) { Ok(p) => p, Err(e) => return e };
    let from_str = match path_str(from) { Ok(p) => p, Err(e) => return e };
    match std::os::unix::fs::symlink(from_str, to_real) {
        Ok(()) => 0,
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

unsafe extern "C" fn fuse_access(
    _path: *const c_char,
    _mask: c_int,
) -> c_int {
    timed!(access);
    0 // Allow all access
}

/// Permissive setattr: lets clients (e.g. Buck2) issue chmod/utimens on
/// writable output paths without us forcing libfuse to break the request
/// into individual handlers.
///
/// macFUSE's `fuse_lib_setattr$DARWIN` (fuse.c:3501) tries `op.setattr`
/// first, falls back to per-attr handlers (chmod / chown / truncate /
/// utimens / chflags) on -ENOSYS. We only implement chmod, so a setattr
/// that bundles e.g. mode+utimens crashes the chain at utimens with
/// ENOSYS — which then surfaces to userspace and fails things like
/// Buck2's CACHEDIR.TAG creation. Returning 0 here ("setattr handled it")
/// causes libfuse to skip the per-attr fan-out; libfuse re-reads attrs
/// via getattr afterwards.
unsafe extern "C" fn fuse_setattr(
    _path: *const c_char,
    _attr: *mut bindings::fuse_darwin_attr,
    _to_set: c_int,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    0
}

/// Build a `fuse_operations` struct populated with the implemented callbacks.
///
/// `register_readdir = false` leaves the readdir slot null, forcing libfuse
/// (and its FSKit shim) to enumerate directories via LOOKUP-per-entry plus
/// our `getattr`. This is the workaround for the macFUSE-FSKit `readdir`
/// crash tracked in turnkey-4vl.6: under FSKit the filler argument arrives
/// as an opaque token rather than a function pointer, so calling it
/// segfaults regardless of signature. Cache TTLs in `fuse_init`
/// (entry_timeout=300, attr_timeout=300) keep the LOOKUP cost amortized.
pub fn build_operations(register_readdir: bool) -> bindings::fuse_operations {
    let mut ops = bindings::fuse_operations::zeroed();
    ops.getattr = Some(fuse_getattr);
    ops.setattr = Some(fuse_setattr);
    if register_readdir {
        ops.readdir = Some(fuse_readdir);
    }
    ops.open = Some(fuse_open);
    ops.opendir = Some(fuse_opendir);
    ops.releasedir = Some(fuse_releasedir);
    ops.read = Some(fuse_read);
    ops.readlink = Some(fuse_readlink);
    ops.statfs = Some(fuse_statfs);
    ops.access = Some(fuse_access);
    ops.init = Some(fuse_init);
    ops.destroy = Some(fuse_destroy);
    // Write operations (only output mounts are writable, others return EROFS)
    ops.mkdir = Some(fuse_mkdir);
    ops.unlink = Some(fuse_unlink);
    ops.rmdir = Some(fuse_rmdir);
    ops.create = Some(fuse_create);
    ops.write = Some(fuse_write);
    ops.truncate = Some(fuse_truncate);
    ops.chmod = Some(fuse_chmod);
    ops.rename = Some(fuse_rename);
    ops.symlink = Some(fuse_symlink);
    ops
}
