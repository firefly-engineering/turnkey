//! FUSE-T operation callbacks
//!
//! Each callback retrieves FsCore from fuse_get_context()->private_data,
//! converts C paths to Rust, delegates to FsCore, and converts results back.

#![cfg(target_os = "macos")]

use std::ffi::{c_void, CStr, CString};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::raw::{c_char, c_int};
use std::os::unix::fs::MetadataExt;
use std::ptr;

use super::bindings;
use crate::fuse::fs_core::{FsCore, ResolvedPath, VirtualFile};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Retrieve `&FsCore` from the FUSE context's `private_data`.
///
/// # Safety
/// Must only be called from within a FUSE callback where the context has been
/// set up with a valid `FsCore` pointer as `private_data`.
unsafe fn get_core<'a>() -> &'a FsCore {
    let ctx = bindings::fuse_get_context();
    &*((*ctx).private_data as *const FsCore)
}

/// Convert a C path pointer to a Rust `&str`.
///
/// # Safety
/// `path` must be a valid null-terminated C string.
unsafe fn path_str<'a>(path: *const c_char) -> Result<&'a str, c_int> {
    CStr::from_ptr(path).to_str().map_err(|_| -libc::EINVAL)
}

/// Fill a `libc::stat` buffer with directory attributes.
unsafe fn fill_dir_stat(stbuf: *mut libc::stat, uid: u32, gid: u32) {
    (*stbuf).st_mode = libc::S_IFDIR | 0o755;
    (*stbuf).st_nlink = 2;
    (*stbuf).st_uid = uid;
    (*stbuf).st_gid = gid;
}

/// Fill a `libc::stat` buffer with virtual-file attributes.
unsafe fn fill_virtual_file_stat(stbuf: *mut libc::stat, size: u64, uid: u32, gid: u32) {
    (*stbuf).st_mode = libc::S_IFREG | 0o444;
    (*stbuf).st_nlink = 1;
    (*stbuf).st_size = size as libc::off_t;
    (*stbuf).st_uid = uid;
    (*stbuf).st_gid = gid;
}

/// Fill a `libc::stat` buffer from `fs::Metadata`.
unsafe fn fill_stat_from_metadata(stbuf: *mut libc::stat, meta: &fs::Metadata) {
    (*stbuf).st_dev = meta.dev() as libc::dev_t;
    (*stbuf).st_ino = meta.ino() as libc::ino_t;
    (*stbuf).st_mode = meta.mode() as libc::mode_t;
    (*stbuf).st_nlink = meta.nlink() as libc::nlink_t;
    (*stbuf).st_uid = meta.uid();
    (*stbuf).st_gid = meta.gid();
    (*stbuf).st_rdev = meta.rdev() as libc::dev_t;
    (*stbuf).st_size = meta.size() as libc::off_t;
    (*stbuf).st_blksize = meta.blksize() as i32;
    (*stbuf).st_blocks = meta.blocks() as libc::blkcnt_t;
    (*stbuf).st_atime = meta.atime() as libc::time_t;
    (*stbuf).st_mtime = meta.mtime() as libc::time_t;
    (*stbuf).st_ctime = meta.ctime() as libc::time_t;
}

/// Call the filler function with a directory entry name.
///
/// Returns 0 on success, or -EIO if the CString conversion fails.
unsafe fn call_filler(
    buf: *mut c_void,
    filler: bindings::fuse_fill_dir_t,
    name: &str,
) -> c_int {
    let Ok(cname) = CString::new(name) else {
        return -libc::EIO;
    };
    filler.unwrap()(
        buf,
        cname.as_ptr(),
        ptr::null(),
        0,
        bindings::fuse_fill_dir_flags::FUSE_FILL_DIR_PLUS, // value doesn't matter with null stat + offset 0
    );
    0
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

unsafe extern "C" fn fuse_getattr(
    path: *const c_char,
    stbuf: *mut libc::stat,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Zero the stat buffer
    ptr::write_bytes(stbuf, 0, 1);

    match core.resolve_path(path) {
        ResolvedPath::Root | ResolvedPath::Source | ResolvedPath::CellPrefix => {
            fill_dir_stat(stbuf, core.uid, core.gid);
            0
        }
        ResolvedPath::VirtualFile { file } => {
            let content = core.get_virtual_file_content(file);
            fill_virtual_file_stat(stbuf, content.len() as u64, core.uid, core.gid);
            0
        }
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::Cell { real_path, .. }
        | ResolvedPath::CellChild { real_path, .. } => {
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
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Always add . and ..
    call_filler(buf, filler, ".");
    call_filler(buf, filler, "..");

    match core.resolve_path(path) {
        ResolvedPath::Root => {
            call_filler(buf, filler, &core.config.source_dir_name);
            call_filler(buf, filler, &core.config.cell_prefix);
            0
        }
        ResolvedPath::Source => {
            call_filler(buf, filler, ".buckconfig");
            call_filler(buf, filler, ".buckroot");
            match fs::read_dir(&core.repo_root) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            call_filler(buf, filler, name);
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
                call_filler(buf, filler, name);
            }
            0
        }
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::Cell { real_path, .. }
        | ResolvedPath::CellChild { real_path, .. } => {
            match fs::read_dir(&real_path) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            call_filler(buf, filler, name);
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
    0
}

unsafe extern "C" fn fuse_opendir(
    _path: *const c_char,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    0
}

unsafe extern "C" fn fuse_releasedir(
    _path: *const c_char,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
    0
}

unsafe extern "C" fn fuse_read(
    path: *const c_char,
    buf: *mut c_char,
    size: libc::size_t,
    offset: libc::off_t,
    _fi: *mut bindings::fuse_file_info,
) -> c_int {
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
        ResolvedPath::SourceChild { real_path } | ResolvedPath::CellChild { real_path, .. } => {
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
        | ResolvedPath::Cell { .. } => -libc::EISDIR,
        ResolvedPath::NotFound => -libc::ENOENT,
    }
}

unsafe extern "C" fn fuse_readlink(
    path: *const c_char,
    buf: *mut c_char,
    size: libc::size_t,
) -> c_int {
    let core = get_core();
    let path = match path_str(path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let real_path = match core.resolve_path(path) {
        ResolvedPath::SourceChild { real_path }
        | ResolvedPath::Cell { real_path, .. }
        | ResolvedPath::CellChild { real_path, .. } => real_path,
        ResolvedPath::NotFound => return -libc::ENOENT,
        _ => return -libc::EINVAL,
    };

    match fs::read_link(&real_path) {
        Ok(target) => {
            let target_bytes = target.as_os_str().as_encoded_bytes();
            let to_copy = target_bytes.len().min(size - 1);
            ptr::copy_nonoverlapping(target_bytes.as_ptr(), buf as *mut u8, to_copy);
            // Null-terminate
            *buf.add(to_copy) = 0;
            0
        }
        Err(e) => -(e.raw_os_error().unwrap_or(libc::EIO)),
    }
}

unsafe extern "C" fn fuse_statfs(
    _path: *const c_char,
    stbuf: *mut libc::statvfs,
) -> c_int {
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
    _cfg: *mut bindings::fuse_config,
) -> *mut c_void {
    // Return private_data as-is so it stays available in subsequent callbacks
    let ctx = bindings::fuse_get_context();
    (*ctx).private_data
}

unsafe extern "C" fn fuse_destroy(_private_data: *mut c_void) {
    // no-op: FsCore lifetime is managed by the caller
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a `fuse_operations` struct populated with the implemented callbacks.
pub fn build_operations() -> bindings::fuse_operations {
    let mut ops = bindings::fuse_operations::zeroed();
    ops.getattr = Some(fuse_getattr);
    ops.readdir = Some(fuse_readdir);
    ops.open = Some(fuse_open);
    ops.opendir = Some(fuse_opendir);
    ops.releasedir = Some(fuse_releasedir);
    ops.read = Some(fuse_read);
    ops.readlink = Some(fuse_readlink);
    ops.statfs = Some(fuse_statfs);
    ops.init = Some(fuse_init);
    ops.destroy = Some(fuse_destroy);
    ops
}
