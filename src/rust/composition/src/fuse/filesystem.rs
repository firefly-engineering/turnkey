//! FUSE filesystem implementation
//!
//! This implements the low-level FUSE operations for the composition view.
//! The filesystem presents a unified view with:
//! - `/src/` - Pass-through to the repository source
//! - `/external/<cell>/` - Read-only view of dependency cells from Nix store

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyOpen,
    Request,
};
use libc::{ENOENT, ENOTDIR};
use log::debug;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::CompositionConfig;

/// Time-to-live for cached attributes
const TTL: Duration = Duration::from_secs(1);

/// Reserved inode numbers
const ROOT_INO: u64 = 1;
const SRC_INO: u64 = 2;
const EXTERNAL_INO: u64 = 3;
const BUCKCONFIG_INO: u64 = 4;
const BUCKROOT_INO: u64 = 5;
const FIRST_DYNAMIC_INO: u64 = 1000;

/// Virtual file types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VirtualFile {
    BuckConfig,
    BuckRoot,
}

/// Represents the underlying path for an inode
#[derive(Debug, Clone)]
enum InodePath {
    /// Virtual root directory
    Root,
    /// src/ directory - pass-through to repo
    Src,
    /// external/ directory - contains cells
    External,
    /// A cell directory under external/
    Cell { name: String },
    /// A virtual file (generated content)
    Virtual { file: VirtualFile },
    /// A real path on the filesystem
    Real { path: PathBuf },
}

/// The FUSE filesystem for composition views
pub struct CompositionFs {
    /// Configuration for this composition
    config: CompositionConfig,
    /// Path to the repository source (for src/ pass-through)
    src_path: PathBuf,
    /// Inode to path mapping
    inode_map: RwLock<HashMap<u64, InodePath>>,
    /// Path to inode mapping (for lookups)
    path_map: RwLock<HashMap<PathBuf, u64>>,
    /// Next inode number to allocate
    next_inode: AtomicU64,
    /// Current user ID
    uid: u32,
    /// Current group ID
    gid: u32,
}

impl CompositionFs {
    /// Create a new composition filesystem
    pub fn new(config: CompositionConfig, repo_root: PathBuf) -> Self {
        let src_path = repo_root.join("src");

        let mut inode_map = HashMap::new();
        inode_map.insert(ROOT_INO, InodePath::Root);
        inode_map.insert(SRC_INO, InodePath::Src);
        inode_map.insert(EXTERNAL_INO, InodePath::External);
        inode_map.insert(
            BUCKCONFIG_INO,
            InodePath::Virtual {
                file: VirtualFile::BuckConfig,
            },
        );
        inode_map.insert(
            BUCKROOT_INO,
            InodePath::Virtual {
                file: VirtualFile::BuckRoot,
            },
        );

        // Pre-allocate inodes for configured cells
        let mut next_ino = FIRST_DYNAMIC_INO;
        for cell in &config.cells {
            inode_map.insert(next_ino, InodePath::Cell { name: cell.name.clone() });
            next_ino += 1;
        }

        Self {
            config,
            src_path,
            inode_map: RwLock::new(inode_map),
            path_map: RwLock::new(HashMap::new()),
            next_inode: AtomicU64::new(next_ino),
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        }
    }

    /// Get or allocate an inode for a real path
    fn get_or_alloc_inode(&self, path: &PathBuf) -> u64 {
        // Check if we already have an inode for this path
        {
            let path_map = self.path_map.read().unwrap();
            if let Some(&ino) = path_map.get(path) {
                return ino;
            }
        }

        // Allocate a new inode
        let ino = self.next_inode.fetch_add(1, Ordering::SeqCst);
        {
            let mut inode_map = self.inode_map.write().unwrap();
            let mut path_map = self.path_map.write().unwrap();
            inode_map.insert(ino, InodePath::Real { path: path.clone() });
            path_map.insert(path.clone(), ino);
        }
        ino
    }

    /// Get the InodePath for an inode
    fn get_inode_path(&self, ino: u64) -> Option<InodePath> {
        let inode_map = self.inode_map.read().unwrap();
        inode_map.get(&ino).cloned()
    }

    /// Resolve an inode to a real filesystem path (if applicable)
    fn resolve_real_path(&self, ino: u64) -> Option<PathBuf> {
        match self.get_inode_path(ino)? {
            InodePath::Root | InodePath::External | InodePath::Virtual { .. } => None,
            InodePath::Src => Some(self.src_path.clone()),
            InodePath::Cell { name } => self
                .config
                .cells
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.source_path.clone()),
            InodePath::Real { path } => Some(path),
        }
    }

    /// Create FileAttr from filesystem metadata
    fn metadata_to_attr(&self, ino: u64, meta: &fs::Metadata) -> FileAttr {
        let kind = if meta.is_dir() {
            FileType::Directory
        } else if meta.is_symlink() {
            FileType::Symlink
        } else {
            FileType::RegularFile
        };

        FileAttr {
            ino,
            size: meta.len(),
            blocks: meta.blocks(),
            atime: meta.accessed().unwrap_or(UNIX_EPOCH),
            mtime: meta.modified().unwrap_or(UNIX_EPOCH),
            ctime: SystemTime::UNIX_EPOCH
                + Duration::from_secs(meta.ctime() as u64),
            crtime: UNIX_EPOCH,
            kind,
            perm: (meta.mode() & 0o7777) as u16,
            nlink: meta.nlink() as u32,
            uid: meta.uid(),
            gid: meta.gid(),
            rdev: meta.rdev() as u32,
            blksize: meta.blksize() as u32,
            flags: 0,
        }
    }

    /// Create a virtual directory attribute
    fn virtual_dir_attr(&self, ino: u64) -> FileAttr {
        FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    /// Create a virtual file attribute
    fn virtual_file_attr(&self, ino: u64, size: u64) -> FileAttr {
        FileAttr {
            ino,
            size,
            blocks: (size + 511) / 512,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: FileType::RegularFile,
            perm: 0o644,
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    /// Generate the content of .buckconfig
    fn generate_buckconfig(&self) -> String {
        let mut content = String::new();

        // Cell definitions
        content.push_str("[cells]\n");
        content.push_str("    root = .\n");
        content.push_str("    prelude = prelude\n");

        // Add cells for each dependency
        for cell in &self.config.cells {
            content.push_str(&format!("    {} = external/{}\n", cell.name, cell.name));
        }

        content.push('\n');

        // Buildfile configuration
        content.push_str("[buildfile]\n");
        content.push_str("    name = BUCK\n");

        content
    }

    /// Generate the content of .buckroot
    fn generate_buckroot(&self) -> String {
        // .buckroot is just a marker file, can be empty or contain a comment
        "# Buck2 repository root marker\n".to_string()
    }

    /// Get the content of a virtual file
    fn get_virtual_file_content(&self, file: VirtualFile) -> String {
        match file {
            VirtualFile::BuckConfig => self.generate_buckconfig(),
            VirtualFile::BuckRoot => self.generate_buckroot(),
        }
    }

    /// Find the inode for a cell by name
    fn find_cell_inode(&self, name: &str) -> Option<u64> {
        let inode_map = self.inode_map.read().unwrap();
        for (&ino, path) in inode_map.iter() {
            if let InodePath::Cell { name: cell_name } = path {
                if cell_name == name {
                    return Some(ino);
                }
            }
        }
        None
    }
}

impl Filesystem for CompositionFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("lookup(parent={}, name={:?})", parent, name_str);

        match self.get_inode_path(parent) {
            Some(InodePath::Root) => {
                // Looking up in root directory
                match name_str.as_ref() {
                    "src" => {
                        if let Some(path) = self.resolve_real_path(SRC_INO) {
                            if let Ok(meta) = fs::metadata(&path) {
                                reply.entry(&TTL, &self.metadata_to_attr(SRC_INO, &meta), 0);
                                return;
                            }
                        }
                        // src/ doesn't exist on disk, return virtual
                        reply.entry(&TTL, &self.virtual_dir_attr(SRC_INO), 0);
                    }
                    "external" => {
                        reply.entry(&TTL, &self.virtual_dir_attr(EXTERNAL_INO), 0);
                    }
                    ".buckconfig" => {
                        let content = self.generate_buckconfig();
                        reply.entry(
                            &TTL,
                            &self.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64),
                            0,
                        );
                    }
                    ".buckroot" => {
                        let content = self.generate_buckroot();
                        reply.entry(
                            &TTL,
                            &self.virtual_file_attr(BUCKROOT_INO, content.len() as u64),
                            0,
                        );
                    }
                    _ => {
                        reply.error(ENOENT);
                    }
                }
            }
            Some(InodePath::External) => {
                // Looking up a cell in external/
                if let Some(ino) = self.find_cell_inode(&name_str) {
                    if let Some(path) = self.resolve_real_path(ino) {
                        if let Ok(meta) = fs::metadata(&path) {
                            reply.entry(&TTL, &self.metadata_to_attr(ino, &meta), 0);
                            return;
                        }
                    }
                }
                reply.error(ENOENT);
            }
            Some(InodePath::Virtual { .. }) => {
                // Virtual files don't have children
                reply.error(ENOENT);
            }
            Some(InodePath::Src) | Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                // Looking up in a real directory
                if let Some(parent_path) = self.resolve_real_path(parent) {
                    let child_path = parent_path.join(name);
                    if let Ok(meta) = fs::symlink_metadata(&child_path) {
                        let ino = self.get_or_alloc_inode(&child_path);
                        reply.entry(&TTL, &self.metadata_to_attr(ino, &meta), 0);
                        return;
                    }
                }
                reply.error(ENOENT);
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("getattr(ino={})", ino);

        match self.get_inode_path(ino) {
            Some(InodePath::Root) | Some(InodePath::External) => {
                reply.attr(&TTL, &self.virtual_dir_attr(ino));
            }
            Some(InodePath::Virtual { file }) => {
                let content = self.get_virtual_file_content(file);
                reply.attr(&TTL, &self.virtual_file_attr(ino, content.len() as u64));
            }
            Some(InodePath::Src) | Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        reply.attr(&TTL, &self.metadata_to_attr(ino, &meta));
                        return;
                    }
                }
                reply.error(ENOENT);
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        debug!("open(ino={})", ino);
        // We don't use file handles, just allow the open
        reply.opened(0, 0);
    }

    fn opendir(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        debug!("opendir(ino={})", ino);
        // We don't use directory handles, just allow the open
        reply.opened(0, 0);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("read(ino={}, offset={}, size={})", ino, offset, size);

        // Check for virtual files first
        if let Some(InodePath::Virtual { file }) = self.get_inode_path(ino) {
            let content = self.get_virtual_file_content(file);
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

        // Read from real path
        if let Some(path) = self.resolve_real_path(ino) {
            match File::open(&path) {
                Ok(mut file) => {
                    use std::io::Seek;
                    if file.seek(std::io::SeekFrom::Start(offset as u64)).is_ok() {
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
        reply.error(ENOENT);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("readdir(ino={}, offset={})", ino, offset);

        match self.get_inode_path(ino) {
            Some(InodePath::Root) => {
                let entries: Vec<(u64, FileType, &str)> = vec![
                    (ROOT_INO, FileType::Directory, "."),
                    (ROOT_INO, FileType::Directory, ".."),
                    (SRC_INO, FileType::Directory, "src"),
                    (EXTERNAL_INO, FileType::Directory, "external"),
                    (BUCKCONFIG_INO, FileType::RegularFile, ".buckconfig"),
                    (BUCKROOT_INO, FileType::RegularFile, ".buckroot"),
                ];

                for (i, (inode, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                    if reply.add(*inode, (i + 1) as i64, *kind, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Some(InodePath::External) => {
                let mut entries: Vec<(u64, FileType, String)> = vec![
                    (EXTERNAL_INO, FileType::Directory, ".".into()),
                    (ROOT_INO, FileType::Directory, "..".into()),
                ];

                // Add configured cells
                for cell in &self.config.cells {
                    if let Some(cell_ino) = self.find_cell_inode(&cell.name) {
                        entries.push((cell_ino, FileType::Directory, cell.name.clone()));
                    }
                }

                for (i, (inode, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                    if reply.add(*inode, (i + 1) as i64, *kind, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Some(InodePath::Src) | Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(read_dir) = fs::read_dir(&path) {
                        let mut entries: Vec<(u64, FileType, String)> = vec![
                            (ino, FileType::Directory, ".".into()),
                            (ROOT_INO, FileType::Directory, "..".into()), // Simplified parent
                        ];

                        for entry in read_dir.flatten() {
                            let child_path = entry.path();
                            let child_ino = self.get_or_alloc_inode(&child_path);
                            let kind = if child_path.is_dir() {
                                FileType::Directory
                            } else if child_path.is_symlink() {
                                FileType::Symlink
                            } else {
                                FileType::RegularFile
                            };
                            if let Some(name) = entry.file_name().to_str() {
                                entries.push((child_ino, kind, name.to_string()));
                            }
                        }

                        for (i, (inode, kind, name)) in
                            entries.iter().enumerate().skip(offset as usize)
                        {
                            if reply.add(*inode, (i + 1) as i64, *kind, name) {
                                break;
                            }
                        }
                        reply.ok();
                        return;
                    }
                }
                reply.error(ENOTDIR);
            }
            Some(InodePath::Virtual { .. }) => {
                // Virtual files are not directories
                reply.error(ENOTDIR);
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn readlink(&mut self, _req: &Request, ino: u64, reply: fuser::ReplyData) {
        debug!("readlink(ino={})", ino);

        if let Some(path) = self.resolve_real_path(ino) {
            if let Ok(target) = fs::read_link(&path) {
                if let Some(s) = target.to_str() {
                    reply.data(s.as_bytes());
                    return;
                }
            }
        }
        reply.error(ENOENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CellConfig;

    #[test]
    fn test_composition_fs_new() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));
        assert_eq!(fs.src_path, PathBuf::from("/home/user/repo/src"));
    }

    #[test]
    fn test_inode_allocation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));

        // Check reserved inodes
        assert!(matches!(fs.get_inode_path(ROOT_INO), Some(InodePath::Root)));
        assert!(matches!(fs.get_inode_path(SRC_INO), Some(InodePath::Src)));
        assert!(matches!(
            fs.get_inode_path(EXTERNAL_INO),
            Some(InodePath::External)
        ));

        // Check cells got allocated inodes
        assert!(fs.find_cell_inode("godeps").is_some());
        assert!(fs.find_cell_inode("rustdeps").is_some());
        assert!(fs.find_cell_inode("nonexistent").is_none());
    }

    #[test]
    fn test_virtual_dir_attr() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));

        let attr = fs.virtual_dir_attr(ROOT_INO);
        assert_eq!(attr.ino, ROOT_INO);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_buckconfig_generation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));

        let content = fs.generate_buckconfig();

        // Check cell definitions
        assert!(content.contains("[cells]"));
        assert!(content.contains("root = ."));
        assert!(content.contains("prelude = prelude"));
        assert!(content.contains("godeps = external/godeps"));
        assert!(content.contains("rustdeps = external/rustdeps"));

        // Check buildfile configuration
        assert!(content.contains("[buildfile]"));
        assert!(content.contains("name = BUCK"));
    }

    #[test]
    fn test_buckroot_generation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));

        let content = fs.generate_buckroot();
        assert!(!content.is_empty());
    }

    #[test]
    fn test_virtual_file_inodes() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));

        // Check virtual file inodes are allocated
        assert!(matches!(
            fs.get_inode_path(BUCKCONFIG_INO),
            Some(InodePath::Virtual {
                file: VirtualFile::BuckConfig
            })
        ));
        assert!(matches!(
            fs.get_inode_path(BUCKROOT_INO),
            Some(InodePath::Virtual {
                file: VirtualFile::BuckRoot
            })
        ));
    }

    #[test]
    fn test_virtual_file_attr() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"));

        let content = fs.generate_buckconfig();
        let attr = fs.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64);

        assert_eq!(attr.ino, BUCKCONFIG_INO);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, content.len() as u64);
        assert_eq!(attr.perm, 0o644);
    }
}
