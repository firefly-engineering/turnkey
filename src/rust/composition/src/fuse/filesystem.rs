//! FUSE filesystem implementation
//!
//! This implements the low-level FUSE operations for the composition view.
//! The filesystem presents a unified view with:
//! - `/<source_dir_name>/` - Overlay on repository root with virtual .buckroot/.buckconfig
//! - `/<cell_prefix>/<cell>/` - Read-only view of dependency cells (e.g., "external/godeps")
//!
//! The source directory is an OVERLAY: it shows all files from the repo root,
//! plus virtual files (.buckroot, .buckconfig) that shadow any real files with
//! the same names. This allows Buck2 targets like `//docs/user-manual` to work
//! identically whether using the FUSE mount or the symlink approach.
//!
//! # Consistency During Updates
//!
//! When dependency cells are being rebuilt, the filesystem handles reads based
//! on the configured `ConsistencyMode`:
//!
//! - `BlockUntilReady`: Block the read until the update completes (default)
//! - `AllowStale`: Return potentially stale data with a warning
//! - `FailIfUpdating`: Return EAGAIN so the caller can retry

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyOpen,
    Request,
};
use libc::{EAGAIN, ENOENT, ENOTDIR};
use log::{debug, warn};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::policy::{
    default_policy, BoxedPolicy, FileClass, OperationType, PolicyDecision,
    SystemState as PolicyState,
};
use crate::state::ConsistencyStateMachine;
use crate::{BackendStatus, CompositionConfig};

/// Time-to-live for cached attributes
const TTL: Duration = Duration::from_secs(1);

/// Reserved inode numbers
const ROOT_INO: u64 = 1;
const SOURCE_INO: u64 = 2;
const CELL_PREFIX_INO: u64 = 3;
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
    /// Virtual root directory of the mount
    Root,
    /// Source directory - pass-through to repo root
    Source,
    /// Cell prefix directory - contains cells (e.g., ".turnkey")
    CellPrefix,
    /// A cell directory under cell prefix
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
    /// Path to the repository root (for source/ pass-through)
    repo_root: PathBuf,
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
    /// State machine for consistency during updates
    state_machine: Arc<ConsistencyStateMachine>,
    /// Access policy for file operations
    policy: BoxedPolicy,
    /// Mutable cell source paths (can be updated atomically during transitions)
    ///
    /// This is separate from config.cells to allow atomic updates without
    /// replacing the entire config. Maps cell name -> current source path.
    cell_paths: RwLock<HashMap<String, PathBuf>>,
}

impl CompositionFs {
    /// Create a new composition filesystem
    pub fn new(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
    ) -> Self {
        Self::with_policy(config, repo_root, state_machine, default_policy())
    }

    /// Create a new composition filesystem with a custom policy
    pub fn with_policy(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
        policy: BoxedPolicy,
    ) -> Self {
        let mut inode_map = HashMap::new();
        inode_map.insert(ROOT_INO, InodePath::Root);
        inode_map.insert(SOURCE_INO, InodePath::Source);
        inode_map.insert(CELL_PREFIX_INO, InodePath::CellPrefix);
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

        // Pre-allocate inodes for configured cells and build cell_paths map
        let mut next_ino = FIRST_DYNAMIC_INO;
        let mut cell_paths = HashMap::new();
        for cell in &config.cells {
            inode_map.insert(next_ino, InodePath::Cell { name: cell.name.clone() });
            cell_paths.insert(cell.name.clone(), cell.source_path.clone());
            next_ino += 1;
        }

        Self {
            config,
            repo_root,
            inode_map: RwLock::new(inode_map),
            path_map: RwLock::new(HashMap::new()),
            next_inode: AtomicU64::new(next_ino),
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            state_machine,
            policy,
            cell_paths: RwLock::new(cell_paths),
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
            InodePath::Root | InodePath::CellPrefix | InodePath::Virtual { .. } => None,
            InodePath::Source => Some(self.repo_root.clone()),
            InodePath::Cell { name } => {
                // Use mutable cell_paths for atomic updates
                let cell_paths = self.cell_paths.read().unwrap();
                cell_paths.get(&name).cloned()
            }
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
    ///
    /// The .buckconfig lives inside the source directory (overlay on repo root).
    /// Paths are relative to where .buckconfig lives:
    /// - `root = .` (current directory, the repo root)
    /// - `prelude = prelude` (relative to repo root)
    /// - `<cell> = ../<cell_prefix>/<cell>` (sibling directory)
    fn generate_buckconfig(&self) -> String {
        let mut content = String::new();
        let cell_prefix = &self.config.cell_prefix;

        // Cell definitions
        content.push_str("[cells]\n");
        // The root cell is the current directory (where .buckconfig lives)
        content.push_str("    root = .\n");
        // Prelude is a subdirectory of the repo root
        content.push_str("    prelude = prelude\n");

        // Add cells for each dependency - they're in the sibling cell_prefix dir
        // e.g., "../external/godeps"
        for cell in &self.config.cells {
            content.push_str(&format!(
                "    {} = ../{}/{}\n",
                cell.name, cell_prefix, cell.name
            ));
        }

        content.push('\n');

        // Buildfile configuration
        content.push_str("[buildfile]\n");
        content.push_str("    name = rules.star\n");

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

    /// Classify an inode path into a FileClass for policy decisions
    fn classify_inode(&self, ino: u64) -> Option<FileClass> {
        match self.get_inode_path(ino)? {
            InodePath::Root | InodePath::CellPrefix => Some(FileClass::VirtualDirectory),
            InodePath::Source => Some(FileClass::SourcePassthrough),
            InodePath::Virtual { .. } => Some(FileClass::VirtualGenerated),
            InodePath::Cell { name } => Some(FileClass::CellContent { cell: name }),
            InodePath::Real { path } => {
                // Check if this path is under any cell's source_path
                for cell in &self.config.cells {
                    if path.starts_with(&cell.source_path) {
                        return Some(FileClass::CellContent {
                            cell: cell.name.clone(),
                        });
                    }
                }
                // Path is under repo_root (source passthrough)
                Some(FileClass::SourcePassthrough)
            }
        }
    }

    /// Get the current system state for policy decisions
    fn get_policy_state(&self) -> PolicyState {
        match self.state_machine.status() {
            BackendStatus::Stopped => PolicyState::Settled, // Treat as settled (not in use)
            BackendStatus::Ready => PolicyState::Settled,
            BackendStatus::Updating { .. } => PolicyState::Syncing,
            BackendStatus::Building { .. } => PolicyState::Building,
            BackendStatus::Transitioning => PolicyState::Transitioning,
            BackendStatus::Error { .. } => PolicyState::Error,
        }
    }

    /// Check if an operation is allowed by the policy
    ///
    /// Returns Ok(()) if the operation is allowed (possibly after waiting).
    /// Returns Err(errno) if the operation should be denied.
    fn check_policy(&self, class: &FileClass, op: OperationType) -> Result<(), i32> {
        let state = self.get_policy_state();
        let decision = self.policy.check(class, state, op);

        match decision {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::AllowStale => {
                if let Some(cell) = class.cell_name() {
                    warn!(
                        "Policy '{}': returning potentially stale data for cell '{}' during {:?}",
                        self.policy.name(),
                        cell,
                        state
                    );
                }
                Ok(())
            }
            PolicyDecision::Block { timeout } => {
                debug!(
                    "Policy '{}': blocking for up to {:?} until stable",
                    self.policy.name(),
                    timeout
                );
                if let Err(e) = self.state_machine.wait_ready(Some(timeout)) {
                    warn!("Timeout waiting for stable state: {:?}", e);
                    return Err(EAGAIN);
                }
                Ok(())
            }
            PolicyDecision::Deny { errno } => {
                debug!(
                    "Policy '{}': denying {:?} on {:?} in state {:?}",
                    self.policy.name(),
                    op,
                    class,
                    state
                );
                Err(errno)
            }
        }
    }

    /// Check policy for an inode and operation
    ///
    /// Convenience method that classifies the inode and checks the policy.
    fn check_inode_policy(&self, ino: u64, op: OperationType) -> Result<(), i32> {
        if let Some(class) = self.classify_inode(ino) {
            self.check_policy(&class, op)
        } else {
            // Unknown inode, allow by default
            Ok(())
        }
    }

    /// Check policy for a cell access
    ///
    /// Convenience method for cell-related operations.
    fn check_cell_policy(&self, cell_name: &str, op: OperationType) -> Result<(), i32> {
        let class = FileClass::CellContent {
            cell: cell_name.to_string(),
        };
        self.check_policy(&class, op)
    }

    /// Apply pending cell updates from the state machine
    ///
    /// This method should be called during the Transitioning state to
    /// atomically update cell source paths. It:
    ///
    /// 1. Takes the pending updates from the state machine
    /// 2. Updates the cell_paths map atomically
    /// 3. Invalidates cached inodes for affected cells
    ///
    /// Returns the number of cells updated, or None if there were no updates.
    pub fn apply_pending_updates(&self) -> Option<usize> {
        // Take pending updates from the state machine
        let updates = self.state_machine.take_pending_updates()?;

        if updates.is_empty() {
            return Some(0);
        }

        // Collect cell names that need cache invalidation
        let affected_cells: Vec<String> = updates.keys().cloned().collect();

        // Update cell paths atomically
        {
            let mut cell_paths = self.cell_paths.write().unwrap();
            for (cell_name, update) in &updates {
                debug!(
                    "Updating cell '{}' path: {:?} -> {:?}",
                    cell_name, update.old_source_path, update.new_source_path
                );
                cell_paths.insert(cell_name.clone(), update.new_source_path.clone());
            }
        }

        // Invalidate cached inodes for affected cells
        self.invalidate_cell_caches(&affected_cells);

        Some(updates.len())
    }

    /// Invalidate cached inodes for the specified cells
    ///
    /// This clears the path_map entries for any paths that were under
    /// the old cell source paths, ensuring subsequent lookups will
    /// use the new paths.
    fn invalidate_cell_caches(&self, cell_names: &[String]) {
        // Get the cell source paths before invalidation
        let cell_paths = self.cell_paths.read().unwrap();
        let cell_source_paths: Vec<PathBuf> = cell_names
            .iter()
            .filter_map(|name| cell_paths.get(name).cloned())
            .collect();
        drop(cell_paths);

        // Remove cached path -> inode mappings for affected cells
        let mut path_map = self.path_map.write().unwrap();
        let mut inode_map = self.inode_map.write().unwrap();

        // Collect inodes to remove
        let inodes_to_remove: Vec<u64> = path_map
            .iter()
            .filter_map(|(path, &ino)| {
                // Check if this path is under any of the affected cell source paths
                for cell_path in &cell_source_paths {
                    if path.starts_with(cell_path) {
                        return Some(ino);
                    }
                }
                None
            })
            .collect();

        let removed_count = inodes_to_remove.len();

        // Remove from path_map
        path_map.retain(|path, _| {
            !cell_source_paths.iter().any(|cp| path.starts_with(cp))
        });

        // Remove from inode_map (but keep the Cell entries, only remove Real entries)
        for ino in inodes_to_remove {
            if let Some(InodePath::Real { .. }) = inode_map.get(&ino) {
                inode_map.remove(&ino);
            }
        }

        debug!(
            "Invalidated caches for {} cells, removed {} inode mappings",
            cell_names.len(),
            removed_count
        );
    }

    /// Check if there are pending updates that need to be applied
    pub fn has_pending_updates(&self) -> bool {
        self.state_machine.has_pending_updates()
    }
}

impl Filesystem for CompositionFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("lookup(parent={}, name={:?})", parent, name_str);

        match self.get_inode_path(parent) {
            Some(InodePath::Root) => {
                // Looking up in mount root - only source and cell prefix directories
                if name_str == self.config.source_dir_name {
                    // Source directory (e.g., "root") - this is the overlay on repo
                    reply.entry(&TTL, &self.virtual_dir_attr(SOURCE_INO), 0);
                } else if name_str == self.config.cell_prefix {
                    // Cell prefix directory (e.g., "external")
                    reply.entry(&TTL, &self.virtual_dir_attr(CELL_PREFIX_INO), 0);
                } else {
                    reply.error(ENOENT);
                }
            }
            Some(InodePath::CellPrefix) => {
                // Looking up a cell in external/
                if let Some(ino) = self.find_cell_inode(&name_str) {
                    // Check policy before accessing cell content
                    if let Err(errno) = self.check_cell_policy(&name_str, OperationType::Lookup) {
                        reply.error(errno);
                        return;
                    }
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
            Some(InodePath::Source) => {
                // Source is an overlay: check virtual files first, then real files
                // Virtual files shadow any real files with the same name
                if name_str == ".buckconfig" {
                    let content = self.generate_buckconfig();
                    reply.entry(
                        &TTL,
                        &self.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64),
                        0,
                    );
                    return;
                }
                if name_str == ".buckroot" {
                    let content = self.generate_buckroot();
                    reply.entry(
                        &TTL,
                        &self.virtual_file_attr(BUCKROOT_INO, content.len() as u64),
                        0,
                    );
                    return;
                }
                // Fall through to real file lookup
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
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                // Looking up in a real directory (cells or nested real paths)
                // Check policy for cell paths (not source passthrough)
                if let Err(errno) = self.check_inode_policy(parent, OperationType::Lookup) {
                    reply.error(errno);
                    return;
                }
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
            Some(InodePath::Root) | Some(InodePath::CellPrefix) => {
                reply.attr(&TTL, &self.virtual_dir_attr(ino));
            }
            Some(InodePath::Virtual { file }) => {
                let content = self.get_virtual_file_content(file);
                reply.attr(&TTL, &self.virtual_file_attr(ino, content.len() as u64));
            }
            Some(InodePath::Source) => {
                // Source passthrough - no consistency check needed
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        reply.attr(&TTL, &self.metadata_to_attr(ino, &meta));
                        return;
                    }
                }
                reply.error(ENOENT);
            }
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                // Check policy for cell paths (not source passthrough)
                if let Err(errno) = self.check_inode_policy(ino, OperationType::Getattr) {
                    reply.error(errno);
                    return;
                }
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

        // Check policy for cell paths before reading
        if let Err(errno) = self.check_inode_policy(ino, OperationType::Read) {
            reply.error(errno);
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
                // Mount root only contains source (overlay) and cell prefix directories
                let source_name = self.config.source_dir_name.clone();
                let cell_prefix = self.config.cell_prefix.clone();

                let entries: Vec<(u64, FileType, String)> = vec![
                    (ROOT_INO, FileType::Directory, ".".into()),
                    (ROOT_INO, FileType::Directory, "..".into()),
                    (SOURCE_INO, FileType::Directory, source_name),
                    (CELL_PREFIX_INO, FileType::Directory, cell_prefix),
                ];

                for (i, (inode, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                    if reply.add(*inode, (i + 1) as i64, *kind, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Some(InodePath::CellPrefix) => {
                let mut entries: Vec<(u64, FileType, String)> = vec![
                    (CELL_PREFIX_INO, FileType::Directory, ".".into()),
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
            Some(InodePath::Source) => {
                // Source is an overlay: merge real entries with virtual files
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(read_dir) = fs::read_dir(&path) {
                        let mut entries: Vec<(u64, FileType, String)> = vec![
                            (ino, FileType::Directory, ".".into()),
                            (ROOT_INO, FileType::Directory, "..".into()),
                        ];

                        // Track which virtual files we add (to avoid duplicates)
                        let mut has_buckconfig = false;
                        let mut has_buckroot = false;

                        for entry in read_dir.flatten() {
                            let child_path = entry.path();
                            if let Some(name) = entry.file_name().to_str() {
                                // Skip real files that are shadowed by virtual files
                                if name == ".buckconfig" {
                                    has_buckconfig = true;
                                    // Add virtual version instead
                                    entries.push((
                                        BUCKCONFIG_INO,
                                        FileType::RegularFile,
                                        ".buckconfig".into(),
                                    ));
                                    continue;
                                }
                                if name == ".buckroot" {
                                    has_buckroot = true;
                                    // Add virtual version instead
                                    entries.push((
                                        BUCKROOT_INO,
                                        FileType::RegularFile,
                                        ".buckroot".into(),
                                    ));
                                    continue;
                                }

                                let child_ino = self.get_or_alloc_inode(&child_path);
                                let kind = if child_path.is_dir() {
                                    FileType::Directory
                                } else if child_path.is_symlink() {
                                    FileType::Symlink
                                } else {
                                    FileType::RegularFile
                                };
                                entries.push((child_ino, kind, name.to_string()));
                            }
                        }

                        // Add virtual files if they don't exist in real fs
                        if !has_buckconfig {
                            entries.push((
                                BUCKCONFIG_INO,
                                FileType::RegularFile,
                                ".buckconfig".into(),
                            ));
                        }
                        if !has_buckroot {
                            entries.push((
                                BUCKROOT_INO,
                                FileType::RegularFile,
                                ".buckroot".into(),
                            ));
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
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                // Real directories (cells or nested paths) - no overlay
                // Check policy for cell paths (not source passthrough)
                if let Err(errno) = self.check_inode_policy(ino, OperationType::Readdir) {
                    reply.error(errno);
                    return;
                }
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
        // repo_root is the actual repository root (not src subdirectory)
        assert_eq!(fs.repo_root, PathBuf::from("/home/user/repo"));
    }

    #[test]
    fn test_inode_allocation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"))
            .with_cell(CellConfig::new("rustdeps", "/nix/store/rustdeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        // Check reserved inodes
        assert!(matches!(fs.get_inode_path(ROOT_INO), Some(InodePath::Root)));
        assert!(matches!(fs.get_inode_path(SOURCE_INO), Some(InodePath::Source)));
        assert!(matches!(
            fs.get_inode_path(CELL_PREFIX_INO),
            Some(InodePath::CellPrefix)
        ));

        // Check cells got allocated inodes
        assert!(fs.find_cell_inode("godeps").is_some());
        assert!(fs.find_cell_inode("rustdeps").is_some());
        assert!(fs.find_cell_inode("nonexistent").is_none());
    }

    #[test]
    fn test_virtual_dir_attr() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

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
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let content = fs.generate_buckconfig();

        // Check cell definitions
        // .buckconfig lives in the source dir (overlay on repo root)
        assert!(content.contains("[cells]"));
        // root = . (current directory, where .buckconfig lives)
        assert!(content.contains("root = ."));
        // prelude is a subdirectory
        assert!(content.contains("prelude = prelude"));
        // Cells are in sibling directory: ../external/<cell>
        assert!(content.contains("godeps = ../external/godeps"));
        assert!(content.contains("rustdeps = ../external/rustdeps"));

        // Check buildfile configuration
        assert!(content.contains("[buildfile]"));
        assert!(content.contains("name = rules.star"));
    }

    #[test]
    fn test_buckroot_generation() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let content = fs.generate_buckroot();
        assert!(!content.is_empty());
    }

    #[test]
    fn test_virtual_file_inodes() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

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
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        let content = fs.generate_buckconfig();
        let attr = fs.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64);

        assert_eq!(attr.ino, BUCKCONFIG_INO);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, content.len() as u64);
        assert_eq!(attr.perm, 0o644);
    }

    #[test]
    fn test_policy_check_settled_allows() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        state_machine.set_ready().unwrap();
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"), state_machine);

        // When system is settled, policy check should allow cell access
        let result = fs.check_cell_policy("godeps", OperationType::Read);
        assert!(result.is_ok());
    }

    #[test]
    fn test_policy_check_building_with_lenient() {
        use crate::policy::LenientPolicy;

        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());

        // Set up a building state
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

        // Lenient policy allows stale reads during building
        let result = fs.check_cell_policy("godeps", OperationType::Read);
        assert!(result.is_ok());
    }

    #[test]
    fn test_policy_check_building_with_ci_policy() {
        use crate::policy::CIPolicy;

        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());

        // Set up a building state
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

        // CI policy denies with EAGAIN during building
        let result = fs.check_cell_policy("godeps", OperationType::Read);
        assert!(matches!(result, Err(errno) if errno == crate::policy::EAGAIN));
    }

    #[test]
    fn test_classify_inode() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(config, PathBuf::from("/home/user/repo"), state_machine);

        // Virtual directories
        assert!(matches!(
            fs.classify_inode(ROOT_INO),
            Some(FileClass::VirtualDirectory)
        ));
        assert!(matches!(
            fs.classify_inode(CELL_PREFIX_INO),
            Some(FileClass::VirtualDirectory)
        ));

        // Source passthrough
        assert!(matches!(
            fs.classify_inode(SOURCE_INO),
            Some(FileClass::SourcePassthrough)
        ));

        // Virtual files
        assert!(matches!(
            fs.classify_inode(BUCKCONFIG_INO),
            Some(FileClass::VirtualGenerated)
        ));
        assert!(matches!(
            fs.classify_inode(BUCKROOT_INO),
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

        // Initial path
        {
            let cell_paths = fs.cell_paths.read().unwrap();
            assert_eq!(
                cell_paths.get("godeps"),
                Some(&PathBuf::from("/nix/store/old-godeps"))
            );
        }

        // Go through update cycle with new path
        state_machine.set_ready().unwrap();
        state_machine.trigger_update(vec!["godeps".into()]).unwrap();
        state_machine
            .start_build(vec![PathBuf::from("/firefly/turnkey/external/godeps")])
            .unwrap();

        // Complete build with new path
        let updates = vec![CellUpdate {
            cell_name: "godeps".into(),
            new_source_path: PathBuf::from("/nix/store/new-godeps"),
            old_source_path: Some(PathBuf::from("/nix/store/old-godeps")),
        }];
        state_machine.build_complete_with_updates(updates).unwrap();

        // Apply updates
        let count = fs.apply_pending_updates();
        assert_eq!(count, Some(1));

        // Path should be updated
        {
            let cell_paths = fs.cell_paths.read().unwrap();
            assert_eq!(
                cell_paths.get("godeps"),
                Some(&PathBuf::from("/nix/store/new-godeps"))
            );
        }

        // No more pending updates
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

        // Not in transitioning state
        assert!(!fs.has_pending_updates());
        assert!(fs.apply_pending_updates().is_none());
    }
}
