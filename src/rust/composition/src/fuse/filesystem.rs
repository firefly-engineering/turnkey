//! FUSE filesystem implementation (fuser adapter)
//!
//! This module is a thin adapter that implements the `fuser::Filesystem` trait
//! by delegating all logic to the platform-agnostic `FsCore` in `fs_core.rs`.
//!
//! The only `fuser`-specific code lives here: type conversions between
//! `FsCore` types (`FsAttr`, `FsFileType`, `u64` inodes) and `fuser` types
//! (`FileAttr`, `FileType`, `INodeNo`), plus the `Filesystem` trait methods
//! that call into `self.core`.
//!
//! # Edit Layer (Copy-on-Write)
//!
//! When editing is enabled (`config.enable_editing`), writes to editable cells
//! are captured in an overlay directory (`.turnkey/edits/`). This allows editing
//! external dependencies without modifying the read-only Nix store.
//!
//! # Consistency During Updates
//!
//! When dependency cells are being rebuilt, the filesystem handles reads based
//! on the configured `ConsistencyMode` — see `fs_core.rs` for policy details.

use fuser::{
    BsdFileFlags, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation,
    INodeNo, LockOwner, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyOpen,
    ReplyWrite, Request, TimeOrNow, WriteFlags,
};
use log::debug;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::fs_core::{
    FsAttr, FsCore, FsFileType, InodePath, VirtualFile, BUCKCONFIG_INO, BUCKROOT_INO,
    CELL_PREFIX_INO, ROOT_INO, SOURCE_INO,
};
use crate::performance::CacheConfig;
use crate::policy::{BoxedPolicy, OperationType};
use crate::state::ConsistencyStateMachine;
use crate::CompositionConfig;

// ---------------------------------------------------------------------------
// Type conversions: FsCore <-> fuser
// ---------------------------------------------------------------------------

/// Convert a platform-neutral `FsFileType` to a `fuser::FileType`.
#[inline]
fn to_fuser_file_type(ft: FsFileType) -> FileType {
    match ft {
        FsFileType::Directory => FileType::Directory,
        FsFileType::RegularFile => FileType::RegularFile,
        FsFileType::Symlink => FileType::Symlink,
    }
}

/// Convert a platform-neutral `FsAttr` to a `fuser::FileAttr`.
#[inline]
fn to_fuser_attr(a: &FsAttr) -> FileAttr {
    FileAttr {
        ino: INodeNo(a.ino),
        size: a.size,
        blocks: a.blocks,
        atime: a.atime,
        mtime: a.mtime,
        ctime: a.ctime,
        crtime: UNIX_EPOCH,
        kind: to_fuser_file_type(a.kind),
        perm: a.perm,
        nlink: a.nlink,
        uid: a.uid,
        gid: a.gid,
        rdev: a.rdev,
        blksize: a.blksize,
        flags: 0,
    }
}

/// Convert a `std::fs::FileType` to a `fuser::FileType` (convenience shortcut).
#[inline]
fn std_to_fuser_file_type(ft: std::fs::FileType) -> FileType {
    to_fuser_file_type(FsCore::to_fs_file_type(ft))
}

/// Convert an `i32` errno from `FsCore` to a `fuser::Errno`.
#[inline]
fn to_fuser_errno(errno: i32) -> Errno {
    Errno::from_i32(errno)
}

// ---------------------------------------------------------------------------
// CompositionFs — thin wrapper around FsCore
// ---------------------------------------------------------------------------

/// The FUSE filesystem for composition views.
///
/// All state and logic lives in `FsCore`; this struct only adds the
/// `fuser::Filesystem` trait implementation.
pub struct CompositionFs {
    core: FsCore,
}

impl CompositionFs {
    /// Create a new composition filesystem
    pub fn new(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
    ) -> Self {
        Self {
            core: FsCore::new(config, repo_root, state_machine),
        }
    }

    /// Create a new composition filesystem with a custom policy
    pub fn with_policy(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
        policy: BoxedPolicy,
    ) -> Self {
        Self {
            core: FsCore::with_policy(config, repo_root, state_machine, policy),
        }
    }

    /// Create a new composition filesystem with custom policy and cache configuration
    pub fn with_options(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
        policy: BoxedPolicy,
        cache_config: CacheConfig,
    ) -> Self {
        Self {
            core: FsCore::with_options(config, repo_root, state_machine, policy, cache_config),
        }
    }

    /// Get the layout name
    #[allow(dead_code)]
    pub fn layout_name(&self) -> &'static str {
        self.core.layout_name()
    }

    /// Apply pending cell updates from the state machine
    pub fn apply_pending_updates(&self) -> Option<usize> {
        self.core.apply_pending_updates()
    }

    /// Check if there are pending updates that need to be applied
    pub fn has_pending_updates(&self) -> bool {
        self.core.has_pending_updates()
    }
}

// ---------------------------------------------------------------------------
// fuser::Filesystem implementation — delegates to self.core
// ---------------------------------------------------------------------------

impl Filesystem for CompositionFs {
    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: fuser::ReplyStatfs) {
        reply.statfs(0, 0, 0, 0, 0, 512, 255, 0);
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("lookup(parent={:?}, name={:?})", parent, name_str);

        let parent_raw = parent.0;

        match self.core.get_inode_path(parent_raw) {
            Some(InodePath::Root) => {
                if name_str == self.core.config.source_dir_name {
                    let attr = to_fuser_attr(&self.core.virtual_dir_attr(SOURCE_INO));
                    reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                } else if name_str == self.core.config.cell_prefix {
                    let attr = to_fuser_attr(&self.core.virtual_dir_attr(CELL_PREFIX_INO));
                    reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                } else {
                    reply.error(Errno::ENOENT);
                }
            }
            Some(InodePath::CellPrefix) => {
                if let Some(ino) = self.core.find_cell_inode(&name_str) {
                    if let Err(errno) =
                        self.core.check_cell_policy(&name_str, OperationType::Lookup)
                    {
                        reply.error(to_fuser_errno(errno));
                        return;
                    }
                    if let Some(path) = self.core.resolve_real_path(ino) {
                        if let Ok(meta) = fs::metadata(&path) {
                            let attr = to_fuser_attr(&self.core.metadata_to_attr(ino, &meta));
                            reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                            return;
                        }
                    }
                }
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Virtual { .. }) => {
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Source) => {
                if name_str == ".buckconfig" {
                    let content =
                        self.core.get_virtual_file_content(VirtualFile::BuckConfig);
                    let attr = to_fuser_attr(
                        &self.core.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64),
                    );
                    reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                    return;
                }
                if name_str == ".buckroot" {
                    let content =
                        self.core.get_virtual_file_content(VirtualFile::BuckRoot);
                    let attr = to_fuser_attr(
                        &self.core.virtual_file_attr(BUCKROOT_INO, content.len() as u64),
                    );
                    reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                    return;
                }
                if let Some(parent_path) = self.core.resolve_real_path(parent_raw) {
                    let child_path = parent_path.join(name);
                    if let Ok(meta) = fs::symlink_metadata(&child_path) {
                        let ino = self.core.get_or_alloc_inode(&child_path);
                        let attr = to_fuser_attr(&self.core.metadata_to_attr(ino, &meta));
                        reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                if let Err(errno) =
                    self.core.check_inode_policy(parent_raw, OperationType::Lookup)
                {
                    reply.error(to_fuser_errno(errno));
                    return;
                }
                if let Some(parent_path) = self.core.resolve_real_path(parent_raw) {
                    let child_path = parent_path.join(name);
                    if let Ok(meta) = fs::symlink_metadata(&child_path) {
                        let ino = self.core.get_or_alloc_inode(&child_path);
                        let attr = to_fuser_attr(&self.core.metadata_to_attr(ino, &meta));
                        reply.entry(&self.core.attr_ttl(), &attr, Generation(0));
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        debug!("getattr(ino={:?})", ino);
        let ino_raw = ino.0;

        match self.core.get_inode_path(ino_raw) {
            Some(InodePath::Root) | Some(InodePath::CellPrefix) => {
                let attr = to_fuser_attr(&self.core.virtual_dir_attr(ino_raw));
                reply.attr(&self.core.attr_ttl(), &attr);
            }
            Some(InodePath::Virtual { file }) => {
                let content = self.core.get_virtual_file_content(file);
                let attr = to_fuser_attr(
                    &self.core.virtual_file_attr(ino_raw, content.len() as u64),
                );
                reply.attr(&self.core.attr_ttl(), &attr);
            }
            Some(InodePath::Source) => {
                if let Some(path) = self.core.resolve_real_path(ino_raw) {
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        let attr = to_fuser_attr(&self.core.metadata_to_attr(ino_raw, &meta));
                        reply.attr(&self.core.attr_ttl(), &attr);
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                if let Err(errno) =
                    self.core.check_inode_policy(ino_raw, OperationType::Getattr)
                {
                    reply.error(to_fuser_errno(errno));
                    return;
                }
                if let Some(path) = self.core.resolve_real_path(ino_raw) {
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        let attr = to_fuser_attr(&self.core.metadata_to_attr(ino_raw, &meta));
                        reply.attr(&self.core.attr_ttl(), &attr);
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        debug!("open(ino={:?})", ino);
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        debug!("opendir(ino={:?})", ino);
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let ino_raw = ino.0;
        debug!("read(ino={:?}, offset={}, size={})", ino, offset, size);

        // Check for virtual files first
        if let Some(InodePath::Virtual { file }) = self.core.get_inode_path(ino_raw) {
            let content = self.core.get_virtual_file_content(file);
            let bytes = content.as_bytes();
            let start = offset as usize;
            if start >= bytes.len() {
                reply.data(&[]);
            } else {
                let end = std::cmp::min(start + size as usize, bytes.len());
                reply.data(&bytes[start..end]);
            }
            return;
        }

        // Check policy for cell paths before reading
        if let Err(errno) = self.core.check_inode_policy(ino_raw, OperationType::Read) {
            reply.error(to_fuser_errno(errno));
            return;
        }

        // Get the real path
        if let Some(path) = self.core.resolve_real_path(ino_raw) {
            let read_path = self.core.get_edit_overlay_path(&path).unwrap_or(path);

            match File::open(&read_path) {
                Ok(mut file) => {
                    use std::io::Seek;
                    if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                        let mut buf = vec![0u8; size as usize];
                        match file.read(&mut buf) {
                            Ok(n) => {
                                reply.data(&buf[..n]);
                                return;
                            }
                            Err(_) => {}
                        }
                    }
                }
                Err(_) => {}
            }
        }
        reply.error(Errno::ENOENT);
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let ino_raw = ino.0;
        debug!(
            "write(ino={:?}, offset={}, size={})",
            ino,
            offset,
            data.len()
        );

        if let Err(errno) = self.core.check_inode_policy(ino_raw, OperationType::Write) {
            reply.error(to_fuser_errno(errno));
            return;
        }

        let (cell_name, relative, original_path) = match self.core.check_edit_allowed(ino_raw) {
            Ok(info) => info,
            Err(errno) => {
                debug!("write denied: errno={}", errno);
                reply.error(to_fuser_errno(errno));
                return;
            }
        };

        let overlay = match &self.core.edit_overlay {
            Some(o) => o,
            None => {
                reply.error(Errno::EROFS);
                return;
            }
        };

        match overlay.write(&cell_name, &relative, &original_path, offset as i64, data) {
            Ok(written) => {
                debug!(
                    "Wrote {} bytes to {}/{} via overlay",
                    written,
                    cell_name,
                    relative.display()
                );
                reply.written(written);
            }
            Err(e) => {
                log::warn!(
                    "Write failed for {}/{}: {}",
                    cell_name,
                    relative.display(),
                    e
                );
                reply.error(Errno::EIO);
            }
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let ino_raw = ino.0;
        debug!("setattr(ino={:?}, size={:?})", ino, size);

        if let Some(new_size) = size {
            let (cell_name, relative, original_path) = match self.core.check_edit_allowed(ino_raw) {
                Ok(info) => info,
                Err(errno) => {
                    reply.error(to_fuser_errno(errno));
                    return;
                }
            };

            let overlay = match &self.core.edit_overlay {
                Some(o) => o,
                None => {
                    reply.error(Errno::EROFS);
                    return;
                }
            };

            if let Err(e) = overlay.truncate(&cell_name, &relative, &original_path, new_size) {
                log::warn!(
                    "Truncate failed for {}/{}: {}",
                    cell_name,
                    relative.display(),
                    e
                );
                reply.error(Errno::EIO);
                return;
            }
        }

        match self.core.get_inode_path(ino_raw) {
            Some(InodePath::Virtual { file }) => {
                let content = self.core.get_virtual_file_content(file);
                let attr = to_fuser_attr(
                    &self.core.virtual_file_attr(ino_raw, content.len() as u64),
                );
                reply.attr(&self.core.attr_ttl(), &attr);
            }
            Some(_) => {
                if let Some(path) = self.core.resolve_real_path(ino_raw) {
                    let attr_path = self.core.get_edit_overlay_path(&path).unwrap_or(path);
                    if let Ok(meta) = fs::symlink_metadata(&attr_path) {
                        let attr =
                            to_fuser_attr(&self.core.metadata_to_attr(ino_raw, &meta));
                        reply.attr(&self.core.attr_ttl(), &attr);
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let ino_raw = ino.0;
        debug!("readdir(ino={:?}, offset={})", ino, offset);

        match self.core.get_inode_path(ino_raw) {
            Some(InodePath::Root) => {
                let source_name = self.core.config.source_dir_name.clone();
                let cell_prefix = self.core.config.cell_prefix.clone();

                let entries: Vec<(INodeNo, FileType, String)> = vec![
                    (INodeNo(ROOT_INO), FileType::Directory, ".".into()),
                    (INodeNo(ROOT_INO), FileType::Directory, "..".into()),
                    (INodeNo(SOURCE_INO), FileType::Directory, source_name),
                    (INodeNo(CELL_PREFIX_INO), FileType::Directory, cell_prefix),
                ];

                for (i, (inode, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                    if reply.add(*inode, (i + 1) as u64, *kind, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Some(InodePath::CellPrefix) => {
                let mut entries: Vec<(INodeNo, FileType, String)> = vec![
                    (INodeNo(CELL_PREFIX_INO), FileType::Directory, ".".into()),
                    (INodeNo(ROOT_INO), FileType::Directory, "..".into()),
                ];

                for cell in &self.core.config.cells {
                    if let Some(cell_ino) = self.core.find_cell_inode(&cell.name) {
                        entries.push((INodeNo(cell_ino), FileType::Directory, cell.name.clone()));
                    }
                }

                for (i, (inode, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                    if reply.add(*inode, (i + 1) as u64, *kind, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Some(InodePath::Source) => {
                if let Some(path) = self.core.resolve_real_path(ino_raw) {
                    if let Ok(read_dir) = fs::read_dir(&path) {
                        let mut entries: Vec<(INodeNo, FileType, String)> = vec![
                            (ino, FileType::Directory, ".".into()),
                            (INodeNo(ROOT_INO), FileType::Directory, "..".into()),
                        ];

                        let mut has_buckconfig = false;
                        let mut has_buckroot = false;

                        for entry in read_dir.flatten() {
                            let child_path = entry.path();
                            if let Some(name) = entry.file_name().to_str() {
                                if name == ".buckconfig" {
                                    has_buckconfig = true;
                                    entries.push((
                                        INodeNo(BUCKCONFIG_INO),
                                        FileType::RegularFile,
                                        ".buckconfig".into(),
                                    ));
                                    continue;
                                }
                                if name == ".buckroot" {
                                    has_buckroot = true;
                                    entries.push((
                                        INodeNo(BUCKROOT_INO),
                                        FileType::RegularFile,
                                        ".buckroot".into(),
                                    ));
                                    continue;
                                }

                                let child_ino =
                                    INodeNo(self.core.get_or_alloc_inode(&child_path));
                                let kind = entry
                                    .file_type()
                                    .map(std_to_fuser_file_type)
                                    .unwrap_or(FileType::RegularFile);
                                entries.push((child_ino, kind, name.to_string()));
                            }
                        }

                        if !has_buckconfig {
                            entries.push((
                                INodeNo(BUCKCONFIG_INO),
                                FileType::RegularFile,
                                ".buckconfig".into(),
                            ));
                        }
                        if !has_buckroot {
                            entries.push((
                                INodeNo(BUCKROOT_INO),
                                FileType::RegularFile,
                                ".buckroot".into(),
                            ));
                        }

                        for (i, (inode, kind, name)) in
                            entries.iter().enumerate().skip(offset as usize)
                        {
                            if reply.add(*inode, (i + 1) as u64, *kind, name) {
                                break;
                            }
                        }
                        reply.ok();
                        return;
                    }
                }
                reply.error(Errno::ENOTDIR);
            }
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                if let Err(errno) =
                    self.core.check_inode_policy(ino_raw, OperationType::Readdir)
                {
                    reply.error(to_fuser_errno(errno));
                    return;
                }
                if let Some(path) = self.core.resolve_real_path(ino_raw) {
                    if let Ok(read_dir) = fs::read_dir(&path) {
                        let mut entries: Vec<(INodeNo, FileType, String)> = vec![
                            (ino, FileType::Directory, ".".into()),
                            (INodeNo(ROOT_INO), FileType::Directory, "..".into()),
                        ];

                        for entry in read_dir.flatten() {
                            let child_path = entry.path();
                            let child_ino = INodeNo(self.core.get_or_alloc_inode(&child_path));
                            let kind = entry
                                .file_type()
                                .map(std_to_fuser_file_type)
                                .unwrap_or(FileType::RegularFile);
                            if let Some(name) = entry.file_name().to_str() {
                                entries.push((child_ino, kind, name.to_string()));
                            }
                        }

                        for (i, (inode, kind, name)) in
                            entries.iter().enumerate().skip(offset as usize)
                        {
                            if reply.add(*inode, (i + 1) as u64, *kind, name) {
                                break;
                            }
                        }
                        reply.ok();
                        return;
                    }
                }
                reply.error(Errno::ENOTDIR);
            }
            Some(InodePath::Virtual { .. }) => {
                reply.error(Errno::ENOTDIR);
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        debug!("readlink(ino={:?})", ino);

        if let Some(path) = self.core.resolve_real_path(ino.0) {
            if let Ok(target) = fs::read_link(&path) {
                if let Some(s) = target.to_str() {
                    reply.data(s.as_bytes());
                    return;
                }
            }
        }
        reply.error(Errno::ENOENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;

    /// Helper to create a test filesystem with default state machine
    fn test_fs(config: CompositionConfig, repo_root: PathBuf) -> CompositionFs {
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        CompositionFs::new(config, repo_root, state_machine)
    }

    #[test]
    fn test_composition_fs_new() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));
        assert_eq!(fs.core.repo_root, PathBuf::from("/home/user/repo"));
    }

    #[test]
    fn test_inode_allocation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        assert!(matches!(
            fs.core.get_inode_path(ROOT_INO),
            Some(InodePath::Root)
        ));
        assert!(matches!(
            fs.core.get_inode_path(SOURCE_INO),
            Some(InodePath::Source)
        ));
        assert!(matches!(
            fs.core.get_inode_path(CELL_PREFIX_INO),
            Some(InodePath::CellPrefix)
        ));

        assert!(fs.core.find_cell_inode("godeps").is_some());
        assert!(fs.core.find_cell_inode("rustdeps").is_some());
        assert!(fs.core.find_cell_inode("nonexistent").is_none());
    }

    #[test]
    fn test_virtual_dir_attr() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let attr = to_fuser_attr(&fs.core.virtual_dir_attr(ROOT_INO));
        assert_eq!(attr.ino, INodeNo(ROOT_INO));
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_buckconfig_generation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let content = fs.core.get_virtual_file_content(VirtualFile::BuckConfig);

        assert!(content.contains("[cells]"));
        assert!(content.contains("root = ."));
        assert!(content.contains("prelude = prelude"));
        assert!(content.contains("godeps = ../external/godeps"));
        assert!(content.contains("rustdeps = ../external/rustdeps"));

        assert!(content.contains("[buildfile]"));
        assert!(content.contains("name = rules.star"));
    }

    #[test]
    fn test_buckroot_generation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let content = fs.core.get_virtual_file_content(VirtualFile::BuckRoot);
        assert!(!content.is_empty());
    }

    #[test]
    fn test_virtual_file_inodes() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        assert!(matches!(
            fs.core.get_inode_path(BUCKCONFIG_INO),
            Some(InodePath::Virtual {
                file: VirtualFile::BuckConfig
            })
        ));
        assert!(matches!(
            fs.core.get_inode_path(BUCKROOT_INO),
            Some(InodePath::Virtual {
                file: VirtualFile::BuckRoot
            })
        ));
    }

    #[test]
    fn test_virtual_file_attr() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let content = fs.core.get_virtual_file_content(VirtualFile::BuckConfig);
        let attr = to_fuser_attr(
            &fs.core.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64),
        );

        assert_eq!(attr.ino, INodeNo(BUCKCONFIG_INO));
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, content.len() as u64);
        assert_eq!(attr.perm, 0o644);
    }

    #[test]
    fn test_layout_name_default() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));
        assert_eq!(fs.layout_name(), "buck2");
    }

    #[test]
    fn test_layout_config_driven() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_layout("unknown-layout");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));
        assert_eq!(fs.layout_name(), "buck2");
    }

    #[test]
    fn test_policy_check_settled_allows() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        state_machine.set_ready().unwrap();
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"), state_machine);

        let result = fs.core.check_cell_policy("godeps", OperationType::Read);
        assert!(result.is_ok());
    }

    #[test]
    fn test_policy_check_building_with_lenient() {
        use crate::policy::LenientPolicy;

        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());

        state_machine.set_ready().unwrap();
        state_machine.trigger_update(vec!["godeps".into()]).unwrap();
        state_machine
            .start_build(vec![PathBuf::from("/firefly/turnkey/external/godeps")])
            .unwrap();

        let fs = CompositionFs::with_policy(
            config,
            PathBuf::from("/home/user/repo"),
            state_machine,
            Box::new(LenientPolicy::new()),
        );

        let result = fs.core.check_cell_policy("godeps", OperationType::Read);
        assert!(result.is_ok());
    }

    #[test]
    fn test_policy_check_building_with_ci_policy() {
        use crate::policy::CIPolicy;

        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());

        state_machine.set_ready().unwrap();
        state_machine.trigger_update(vec!["godeps".into()]).unwrap();
        state_machine
            .start_build(vec![PathBuf::from("/firefly/turnkey/external/godeps")])
            .unwrap();

        let fs = CompositionFs::with_policy(
            config,
            PathBuf::from("/home/user/repo"),
            state_machine,
            Box::new(CIPolicy::new()),
        );

        let result = fs.core.check_cell_policy("godeps", OperationType::Read);
        assert!(matches!(result, Err(errno) if errno == crate::policy::EAGAIN));
    }

    #[test]
    fn test_classify_inode() {
        use crate::policy::FileClass;

        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"), state_machine);

        assert!(matches!(
            fs.core.classify_inode(ROOT_INO),
            Some(FileClass::VirtualDirectory)
        ));
        assert!(matches!(
            fs.core.classify_inode(CELL_PREFIX_INO),
            Some(FileClass::VirtualDirectory)
        ));
        assert!(matches!(
            fs.core.classify_inode(SOURCE_INO),
            Some(FileClass::SourcePassthrough)
        ));
        assert!(matches!(
            fs.core.classify_inode(BUCKCONFIG_INO),
            Some(FileClass::VirtualGenerated)
        ));
        assert!(matches!(
            fs.core.classify_inode(BUCKROOT_INO),
            Some(FileClass::VirtualGenerated)
        ));
    }

    #[test]
    fn test_atomic_cell_path_update() {
        use crate::state::CellUpdate;

        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/old-godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(
            config,
            PathBuf::from("/home/user/repo"),
            Arc::clone(&state_machine),
        );

        {
            let cell_paths = fs.core.cell_paths.read().unwrap();
            assert_eq!(
                cell_paths.get("godeps"),
                Some(&PathBuf::from("/nix/store/old-godeps"))
            );
        }

        state_machine.set_ready().unwrap();
        state_machine.trigger_update(vec!["godeps".into()]).unwrap();
        state_machine
            .start_build(vec![PathBuf::from("/firefly/turnkey/external/godeps")])
            .unwrap();

        let updates = vec![CellUpdate {
            cell_name: "godeps".into(),
            new_source_path: PathBuf::from("/nix/store/new-godeps"),
            old_source_path: Some(PathBuf::from("/nix/store/old-godeps")),
        }];
        state_machine.build_complete_with_updates(updates).unwrap();

        let count = fs.apply_pending_updates();
        assert_eq!(count, Some(1));

        {
            let cell_paths = fs.core.cell_paths.read().unwrap();
            assert_eq!(
                cell_paths.get("godeps"),
                Some(&PathBuf::from("/nix/store/new-godeps"))
            );
        }

        assert!(!fs.has_pending_updates());
    }

    #[test]
    fn test_no_pending_updates_when_not_transitioning() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(
            config,
            PathBuf::from("/home/user/repo"),
            Arc::clone(&state_machine),
        );

        state_machine.set_ready().unwrap();

        assert!(!fs.has_pending_updates());
        assert!(fs.apply_pending_updates().is_none());
    }

    #[test]
    fn test_edit_overlay_disabled_by_default() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));
        assert!(fs.core.edit_overlay.is_none());
    }

    #[test]
    fn test_edit_overlay_enabled_with_config() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps").with_editable(true))
            .with_editing(true);
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        assert!(fs.core.edit_overlay.is_some());

        let overlay = fs.core.edit_overlay.as_ref().unwrap();
        assert!(overlay.is_cell_editable("godeps"));
    }

    #[test]
    fn test_get_cell_info() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/abc-godeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let path = PathBuf::from("/nix/store/abc-godeps/vendor/github.com/foo/bar/lib.go");
        let info = fs.core.get_cell_info(&path);
        assert!(info.is_some());
        let (cell_name, relative) = info.unwrap();
        assert_eq!(cell_name, "godeps");
        assert_eq!(relative, PathBuf::from("vendor/github.com/foo/bar/lib.go"));

        let other_path = PathBuf::from("/nix/store/other/file.txt");
        assert!(fs.core.get_cell_info(&other_path).is_none());
    }

    #[test]
    fn test_check_edit_allowed_rejects_non_editable() {
        let config = CompositionConfig::new("/firefly/turnkey", "/tmp/test-repo")
            .with_cell(CellConfig::new("godeps", "/tmp/test-cell"))
            .with_editing(true);

        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(config, PathBuf::from("/tmp/test-repo"), state_machine);

        let cell_path = PathBuf::from("/tmp/test-cell/vendor/foo/lib.go");
        let ino = fs.core.get_or_alloc_inode(&cell_path);

        let result = fs.core.check_edit_allowed(ino);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), libc::EROFS);
    }

    #[test]
    fn test_check_edit_allowed_accepts_editable() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let cell_source = temp.path().join("nix/store/godeps");

        fs::create_dir_all(cell_source.join("vendor/foo")).unwrap();
        let mut f = File::create(cell_source.join("vendor/foo/lib.go")).unwrap();
        f.write_all(b"package foo\n").unwrap();

        fs::create_dir_all(&repo_root).unwrap();

        let config = CompositionConfig::new("/firefly/turnkey", &repo_root)
            .with_cell(CellConfig::new("godeps", &cell_source).with_editable(true))
            .with_editing(true);

        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(config, repo_root, state_machine);

        let cell_path = cell_source.join("vendor/foo/lib.go");
        let ino = fs.core.get_or_alloc_inode(&cell_path);

        let result = fs.core.check_edit_allowed(ino);
        assert!(result.is_ok());

        let (cell_name, relative, original) = result.unwrap();
        assert_eq!(cell_name, "godeps");
        assert_eq!(relative, PathBuf::from("vendor/foo/lib.go"));
        assert_eq!(original, cell_path);
    }
}
