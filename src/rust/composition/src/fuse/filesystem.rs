//! FUSE filesystem implementation
//!
//! This implements the low-level FUSE operations for the composition view.
//! The filesystem presents a unified view with:
//! - `/<source_dir_name>/` - Overlay on repository root with virtual .buckroot/.buckconfig
//! - `/<cell_prefix>/<cell>/` - View of dependency cells (e.g., "external/godeps")
//!
//! # Edit Layer (Copy-on-Write)
//!
//! When editing is enabled (`config.enable_editing`), writes to editable cells
//! are captured in an overlay directory (`.turnkey/edits/`). This allows editing
//! external dependencies without modifying the read-only Nix store:
//!
//! 1. First write to a file triggers copy-on-write from Nix store to overlay
//! 2. Subsequent reads return the overlay copy
//! 3. Edits can be reverted or converted to patches
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
    BsdFileFlags, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation,
    INodeNo, LockOwner, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyOpen,
    ReplyWrite, Request, TimeOrNow, WriteFlags,
};
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

use super::edit_overlay::EditOverlay;
use crate::layout::{default_layout, layout_by_name, BoxedLayout, CellInfo, LayoutContext};
use crate::performance::CacheConfig;
use crate::policy::{
    default_policy, BoxedPolicy, FileClass, OperationType, PolicyDecision,
    SystemState as PolicyState,
};
use crate::state::ConsistencyStateMachine;
use crate::{BackendStatus, CompositionConfig};


/// Reserved inode numbers
const ROOT_INO: INodeNo = INodeNo(1);
const SOURCE_INO: INodeNo = INodeNo(2);
const CELL_PREFIX_INO: INodeNo = INodeNo(3);
const BUCKCONFIG_INO: INodeNo = INodeNo(4);
const BUCKROOT_INO: INodeNo = INodeNo(5);
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
    inode_map: RwLock<HashMap<INodeNo, InodePath>>,
    /// Path to inode mapping (for lookups)
    path_map: RwLock<HashMap<PathBuf, INodeNo>>,
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
    /// Edit overlay for copy-on-write editing of cell files
    ///
    /// Only present when `config.enable_editing` is true.
    edit_overlay: Option<EditOverlay>,
    /// Layout for build system configuration
    ///
    /// Determines directory structure and config file generation.
    layout: BoxedLayout,
    /// Cached layout context for efficient config generation
    layout_context: LayoutContext,
    /// Cached config files from the layout
    cached_configs: RwLock<HashMap<String, String>>,
    /// Cache configuration for performance tuning
    cache_config: CacheConfig,
}

impl CompositionFs {
    /// Create a new composition filesystem
    pub fn new(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
    ) -> Self {
        Self::with_options(config, repo_root, state_machine, default_policy(), CacheConfig::default())
    }

    /// Create a new composition filesystem with a custom policy
    pub fn with_policy(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
        policy: BoxedPolicy,
    ) -> Self {
        Self::with_options(config, repo_root, state_machine, policy, CacheConfig::default())
    }

    /// Create a new composition filesystem with custom policy and cache configuration
    pub fn with_options(
        config: CompositionConfig,
        repo_root: PathBuf,
        state_machine: Arc<ConsistencyStateMachine>,
        policy: BoxedPolicy,
        cache_config: CacheConfig,
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
        let mut editable_cells = Vec::new();
        for cell in &config.cells {
            inode_map.insert(INodeNo(next_ino), InodePath::Cell { name: cell.name.clone() });
            cell_paths.insert(cell.name.clone(), cell.source_path.clone());
            if cell.editable {
                editable_cells.push(cell.name.clone());
            }
            next_ino += 1;
        }

        // Create edit overlay if editing is enabled
        let edit_overlay = if config.enable_editing {
            let edits_dir = repo_root.join(&config.edits_dir);
            Some(EditOverlay::new(edits_dir, editable_cells))
        } else {
            None
        };

        // Create layout based on config
        let layout = layout_by_name(&config.layout).unwrap_or_else(default_layout);

        // Build layout context
        let layout_context = LayoutContext {
            mount_point: config.mount_point.clone(),
            repo_root: repo_root.clone(),
            source_dir_name: config.source_dir_name.clone(),
            cell_prefix: config.cell_prefix.clone(),
            cells: config
                .cells
                .iter()
                .map(|c| CellInfo::new(&c.name, &c.source_path).with_editable(c.editable))
                .collect(),
        };

        // Generate and cache config files
        let configs: HashMap<String, String> = layout
            .generate_config(&layout_context)
            .into_iter()
            .map(|c| (c.name, c.content))
            .collect();

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
            edit_overlay,
            layout,
            layout_context,
            cached_configs: RwLock::new(configs),
            cache_config,
        }
    }

    /// Get the attribute TTL based on cache configuration
    #[inline]
    fn attr_ttl(&self) -> Duration {
        self.cache_config.attr_ttl()
    }

    /// Convert std::fs::FileType to fuser::FileType
    ///
    /// This avoids making separate is_dir()/is_symlink() calls which would
    /// each trigger a syscall. Instead, we use the file_type() from the
    /// directory entry which is often cached by the OS.
    #[inline]
    fn to_fuse_file_type(ft: std::fs::FileType) -> FileType {
        if ft.is_dir() {
            FileType::Directory
        } else if ft.is_symlink() {
            FileType::Symlink
        } else {
            FileType::RegularFile
        }
    }

    /// Get or allocate an inode for a real path
    ///
    /// This method is optimized to minimize lock contention:
    /// 1. Try read-only lookup first (most common case - cache hit)
    /// 2. If miss, acquire write lock and double-check before inserting
    ///
    /// The double-check pattern handles race conditions where another
    /// thread may have inserted the same path while we were waiting
    /// for the write lock.
    fn get_or_alloc_inode(&self, path: &PathBuf) -> INodeNo {
        // Fast path: read-only lookup (most common case)
        {
            let path_map = self.path_map.read().unwrap();
            if let Some(&ino) = path_map.get(path) {
                return ino;
            }
        }

        // Slow path: need to insert
        // Acquire write locks for both maps
        let mut path_map = self.path_map.write().unwrap();
        let mut inode_map = self.inode_map.write().unwrap();

        // Double-check: another thread may have inserted while we waited
        if let Some(&ino) = path_map.get(path) {
            return ino;
        }

        // Allocate a new inode
        let ino = INodeNo(self.next_inode.fetch_add(1, Ordering::SeqCst));
        inode_map.insert(ino, InodePath::Real { path: path.clone() });
        path_map.insert(path.clone(), ino);
        ino
    }

    /// Get the InodePath for an inode
    fn get_inode_path(&self, ino: INodeNo) -> Option<InodePath> {
        let inode_map = self.inode_map.read().unwrap();
        inode_map.get(&ino).cloned()
    }

    /// Resolve an inode to a real filesystem path (if applicable)
    fn resolve_real_path(&self, ino: INodeNo) -> Option<PathBuf> {
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
    fn metadata_to_attr(&self, ino: INodeNo, meta: &fs::Metadata) -> FileAttr {
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
    fn virtual_dir_attr(&self, ino: INodeNo) -> FileAttr {
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
    fn virtual_file_attr(&self, ino: INodeNo, size: u64) -> FileAttr {
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

    /// Get the content of a virtual file from the layout's cached configs
    ///
    /// Virtual files are generated by the layout plugin. For Buck2, this includes
    /// `.buckconfig` and `.buckroot`. The content is cached at construction time
    /// and can be regenerated when cells change.
    fn get_virtual_file_content(&self, file: VirtualFile) -> String {
        let configs = self.cached_configs.read().unwrap();
        match file {
            VirtualFile::BuckConfig => configs
                .get(".buckconfig")
                .cloned()
                .unwrap_or_default(),
            VirtualFile::BuckRoot => configs
                .get(".buckroot")
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Regenerate cached config files from the layout
    ///
    /// This should be called after cell paths are updated to ensure
    /// virtual files reflect the new state.
    #[allow(dead_code)]
    fn regenerate_configs(&self) {
        let configs: HashMap<String, String> = self
            .layout
            .generate_config(&self.layout_context)
            .into_iter()
            .map(|c| (c.name, c.content))
            .collect();

        let mut cached = self.cached_configs.write().unwrap();
        *cached = configs;
    }

    /// Get the layout name
    #[allow(dead_code)]
    pub fn layout_name(&self) -> &'static str {
        self.layout.name()
    }

    /// Find the inode for a cell by name
    fn find_cell_inode(&self, name: &str) -> Option<INodeNo> {
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
    fn classify_inode(&self, ino: INodeNo) -> Option<FileClass> {
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
    fn check_policy(&self, class: &FileClass, op: OperationType) -> Result<(), Errno> {
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
                    return Err(Errno::EAGAIN);
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
                Err(Errno::from_i32(errno))
            }
        }
    }

    /// Check policy for an inode and operation
    ///
    /// Convenience method that classifies the inode and checks the policy.
    fn check_inode_policy(&self, ino: INodeNo, op: OperationType) -> Result<(), Errno> {
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
    fn check_cell_policy(&self, cell_name: &str, op: OperationType) -> Result<(), Errno> {
        let class = FileClass::CellContent {
            cell: cell_name.to_string(),
        };
        self.check_policy(&class, op)
    }

    /// Get cell info for a path under a cell
    ///
    /// Returns (cell_name, relative_path_within_cell) if the path is under a cell.
    fn get_cell_info(&self, path: &PathBuf) -> Option<(String, PathBuf)> {
        let cell_paths = self.cell_paths.read().unwrap();
        for (cell_name, cell_source) in cell_paths.iter() {
            if let Ok(relative) = path.strip_prefix(cell_source) {
                return Some((cell_name.clone(), relative.to_path_buf()));
            }
        }
        None
    }

    /// Check if a file is in an editable cell and has been edited
    ///
    /// Returns the overlay path if the file should be read from the overlay.
    fn get_edit_overlay_path(&self, path: &PathBuf) -> Option<PathBuf> {
        let overlay = self.edit_overlay.as_ref()?;
        let (cell_name, relative) = self.get_cell_info(path)?;
        overlay.get_read_path(&cell_name, &relative)
    }

    /// Check if editing is allowed for a given inode
    ///
    /// Returns (cell_name, relative_path, original_path) if the inode is in an
    /// editable cell, otherwise returns an error.
    fn check_edit_allowed(&self, ino: INodeNo) -> Result<(String, PathBuf, PathBuf), Errno> {
        // Must have editing enabled
        let overlay = self.edit_overlay.as_ref().ok_or(Errno::EROFS)?;

        // Must be a real path in a cell
        let original_path = self.resolve_real_path(ino).ok_or(Errno::ENOENT)?;
        let (cell_name, relative) = self.get_cell_info(&original_path).ok_or(Errno::EROFS)?;

        // Cell must be editable
        if !overlay.is_cell_editable(&cell_name) {
            return Err(Errno::EROFS);
        }

        Ok((cell_name, relative, original_path))
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
        let inodes_to_remove: Vec<INodeNo> = path_map
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
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("lookup(parent={:?}, name={:?})", parent, name_str);

        match self.get_inode_path(parent) {
            Some(InodePath::Root) => {
                // Looking up in mount root - only source and cell prefix directories
                if name_str == self.config.source_dir_name {
                    // Source directory (e.g., "root") - this is the overlay on repo
                    reply.entry(&self.attr_ttl(), &self.virtual_dir_attr(SOURCE_INO), Generation(0));
                } else if name_str == self.config.cell_prefix {
                    // Cell prefix directory (e.g., "external")
                    reply.entry(&self.attr_ttl(), &self.virtual_dir_attr(CELL_PREFIX_INO), Generation(0));
                } else {
                    reply.error(Errno::ENOENT);
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
                            reply.entry(&self.attr_ttl(), &self.metadata_to_attr(ino, &meta), Generation(0));
                            return;
                        }
                    }
                }
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Virtual { .. }) => {
                // Virtual files don't have children
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Source) => {
                // Source is an overlay: check virtual files first, then real files
                // Virtual files shadow any real files with the same name
                if name_str == ".buckconfig" {
                    let content = self.get_virtual_file_content(VirtualFile::BuckConfig);
                    reply.entry(
                        &self.attr_ttl(),
                        &self.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64),
                        Generation(0),
                    );
                    return;
                }
                if name_str == ".buckroot" {
                    let content = self.get_virtual_file_content(VirtualFile::BuckRoot);
                    reply.entry(
                        &self.attr_ttl(),
                        &self.virtual_file_attr(BUCKROOT_INO, content.len() as u64),
                        Generation(0),
                    );
                    return;
                }
                // Fall through to real file lookup
                if let Some(parent_path) = self.resolve_real_path(parent) {
                    let child_path = parent_path.join(name);
                    if let Ok(meta) = fs::symlink_metadata(&child_path) {
                        let ino = self.get_or_alloc_inode(&child_path);
                        reply.entry(&self.attr_ttl(), &self.metadata_to_attr(ino, &meta), Generation(0));
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
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
                        reply.entry(&self.attr_ttl(), &self.metadata_to_attr(ino, &meta), Generation(0));
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

        match self.get_inode_path(ino) {
            Some(InodePath::Root) | Some(InodePath::CellPrefix) => {
                reply.attr(&self.attr_ttl(), &self.virtual_dir_attr(ino));
            }
            Some(InodePath::Virtual { file }) => {
                let content = self.get_virtual_file_content(file);
                reply.attr(&self.attr_ttl(), &self.virtual_file_attr(ino, content.len() as u64));
            }
            Some(InodePath::Source) => {
                // Source passthrough - no consistency check needed
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        reply.attr(&self.attr_ttl(), &self.metadata_to_attr(ino, &meta));
                        return;
                    }
                }
                reply.error(Errno::ENOENT);
            }
            Some(InodePath::Cell { .. }) | Some(InodePath::Real { .. }) => {
                // Check policy for cell paths (not source passthrough)
                if let Err(errno) = self.check_inode_policy(ino, OperationType::Getattr) {
                    reply.error(errno);
                    return;
                }
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        reply.attr(&self.attr_ttl(), &self.metadata_to_attr(ino, &meta));
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
        // We don't use file handles, just allow the open
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        debug!("opendir(ino={:?})", ino);
        // We don't use directory handles, just allow the open
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
        debug!("read(ino={:?}, offset={}, size={})", ino, offset, size);

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

        // Get the real path
        if let Some(path) = self.resolve_real_path(ino) {
            // Check if this file has been edited (overlay takes precedence)
            let read_path = self.get_edit_overlay_path(&path).unwrap_or(path);

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
        debug!("write(ino={:?}, offset={}, size={})", ino, offset, data.len());

        // Check policy for cell paths before writing
        if let Err(errno) = self.check_inode_policy(ino, OperationType::Write) {
            reply.error(errno);
            return;
        }

        // Check if this is in an editable cell
        let (cell_name, relative, original_path) = match self.check_edit_allowed(ino) {
            Ok(info) => info,
            Err(errno) => {
                debug!("write denied: errno={:?}", errno);
                reply.error(errno);
                return;
            }
        };

        // Perform the write via edit overlay
        let overlay = match &self.edit_overlay {
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
                warn!("Write failed for {}/{}: {}", cell_name, relative.display(), e);
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
        debug!("setattr(ino={:?}, size={:?})", ino, size);

        // Handle truncate via size parameter
        if let Some(new_size) = size {
            // Check if this is in an editable cell
            let (cell_name, relative, original_path) = match self.check_edit_allowed(ino) {
                Ok(info) => info,
                Err(errno) => {
                    reply.error(errno);
                    return;
                }
            };

            let overlay = match &self.edit_overlay {
                Some(o) => o,
                None => {
                    reply.error(Errno::EROFS);
                    return;
                }
            };

            // Truncate via overlay
            if let Err(e) = overlay.truncate(&cell_name, &relative, &original_path, new_size) {
                warn!(
                    "Truncate failed for {}/{}: {}",
                    cell_name,
                    relative.display(),
                    e
                );
                reply.error(Errno::EIO);
                return;
            }
        }

        // Return updated attributes
        match self.get_inode_path(ino) {
            Some(InodePath::Virtual { file }) => {
                let content = self.get_virtual_file_content(file);
                reply.attr(&self.attr_ttl(), &self.virtual_file_attr(ino, content.len() as u64));
            }
            Some(_) => {
                if let Some(path) = self.resolve_real_path(ino) {
                    // Check overlay for edited files
                    let attr_path = self.get_edit_overlay_path(&path).unwrap_or(path);
                    if let Ok(meta) = fs::symlink_metadata(&attr_path) {
                        reply.attr(&self.attr_ttl(), &self.metadata_to_attr(ino, &meta));
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
        debug!("readdir(ino={:?}, offset={})", ino, offset);

        match self.get_inode_path(ino) {
            Some(InodePath::Root) => {
                // Mount root only contains source (overlay) and cell prefix directories
                let source_name = self.config.source_dir_name.clone();
                let cell_prefix = self.config.cell_prefix.clone();

                let entries: Vec<(INodeNo, FileType, String)> = vec![
                    (ROOT_INO, FileType::Directory, ".".into()),
                    (ROOT_INO, FileType::Directory, "..".into()),
                    (SOURCE_INO, FileType::Directory, source_name),
                    (CELL_PREFIX_INO, FileType::Directory, cell_prefix),
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
                    if reply.add(*inode, (i + 1) as u64, *kind, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Some(InodePath::Source) => {
                // Source is an overlay: merge real entries with virtual files
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(read_dir) = fs::read_dir(&path) {
                        let mut entries: Vec<(INodeNo, FileType, String)> = vec![
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
                                // Use entry.file_type() instead of child_path.is_dir()/is_symlink()
                                // to avoid extra syscalls. The file_type is often cached by the OS.
                                let kind = entry
                                    .file_type()
                                    .map(Self::to_fuse_file_type)
                                    .unwrap_or(FileType::RegularFile);
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
                // Real directories (cells or nested paths) - no overlay
                // Check policy for cell paths (not source passthrough)
                if let Err(errno) = self.check_inode_policy(ino, OperationType::Readdir) {
                    reply.error(errno);
                    return;
                }
                if let Some(path) = self.resolve_real_path(ino) {
                    if let Ok(read_dir) = fs::read_dir(&path) {
                        let mut entries: Vec<(INodeNo, FileType, String)> = vec![
                            (ino, FileType::Directory, ".".into()),
                            (ROOT_INO, FileType::Directory, "..".into()), // Simplified parent
                        ];

                        for entry in read_dir.flatten() {
                            let child_path = entry.path();
                            let child_ino = self.get_or_alloc_inode(&child_path);
                            // Use entry.file_type() instead of child_path.is_dir()/is_symlink()
                            // to avoid extra syscalls. The file_type is often cached by the OS.
                            let kind = entry
                                .file_type()
                                .map(Self::to_fuse_file_type)
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
                // Virtual files are not directories
                reply.error(Errno::ENOTDIR);
            }
            None => {
                reply.error(Errno::ENOENT);
            }
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        debug!("readlink(ino={:?})", ino);

        if let Some(path) = self.resolve_real_path(ino) {
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

        let content = fs.get_virtual_file_content(VirtualFile::BuckConfig);

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

        let content = fs.get_virtual_file_content(VirtualFile::BuckRoot);
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

        let content = fs.get_virtual_file_content(VirtualFile::BuckConfig);
        let attr = fs.virtual_file_attr(BUCKCONFIG_INO, content.len() as u64);

        assert_eq!(attr.ino, BUCKCONFIG_INO);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, content.len() as u64);
        assert_eq!(attr.perm, 0o644);
    }

    #[test]
    fn test_layout_name_default() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        // Default layout should be buck2
        assert_eq!(fs.layout_name(), "buck2");
    }

    #[test]
    fn test_layout_config_driven() {
        // Custom layout name falls back to default buck2 if unknown
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_layout("unknown-layout");
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        // Unknown layouts fall back to buck2
        assert_eq!(fs.layout_name(), "buck2");
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
        assert!(matches!(result, Err(errno) if i32::from(errno) == crate::policy::EAGAIN));
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

    #[test]
    fn test_edit_overlay_disabled_by_default() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        // Edit overlay should be None when not enabled
        assert!(fs.edit_overlay.is_none());
    }

    #[test]
    fn test_edit_overlay_enabled_with_config() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/godeps").with_editable(true))
            .with_editing(true);
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        // Edit overlay should be present when enabled
        assert!(fs.edit_overlay.is_some());

        // And the cell should be editable
        let overlay = fs.edit_overlay.as_ref().unwrap();
        assert!(overlay.is_cell_editable("godeps"));
    }

    #[test]
    fn test_get_cell_info() {
        let config = CompositionConfig::new("/firefly/turnkey", "/home/user/repo")
            .with_cell(CellConfig::new("godeps", "/nix/store/abc-godeps"));
        let fs = test_fs(config, PathBuf::from("/home/user/repo"));

        // Path within a cell
        let path = PathBuf::from("/nix/store/abc-godeps/vendor/github.com/foo/bar/lib.go");
        let info = fs.get_cell_info(&path);
        assert!(info.is_some());
        let (cell_name, relative) = info.unwrap();
        assert_eq!(cell_name, "godeps");
        assert_eq!(relative, PathBuf::from("vendor/github.com/foo/bar/lib.go"));

        // Path not in any cell
        let other_path = PathBuf::from("/nix/store/other/file.txt");
        assert!(fs.get_cell_info(&other_path).is_none());
    }

    #[test]
    fn test_check_edit_allowed_rejects_non_editable() {
        // Cell not marked as editable
        let config = CompositionConfig::new("/firefly/turnkey", "/tmp/test-repo")
            .with_cell(CellConfig::new("godeps", "/tmp/test-cell"))
            .with_editing(true);

        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(config, PathBuf::from("/tmp/test-repo"), state_machine);

        // Allocate an inode for a path in the cell
        let cell_path = PathBuf::from("/tmp/test-cell/vendor/foo/lib.go");
        let ino = fs.get_or_alloc_inode(&cell_path);

        // Should be rejected since cell is not editable
        let result = fs.check_edit_allowed(ino);
        assert!(result.is_err());
        assert_eq!(i32::from(result.unwrap_err()), libc::EROFS);
    }

    #[test]
    fn test_check_edit_allowed_accepts_editable() {
        use tempfile::TempDir;
        use std::io::Write;

        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let cell_source = temp.path().join("nix/store/godeps");

        // Create the cell directory and file
        fs::create_dir_all(cell_source.join("vendor/foo")).unwrap();
        let mut f = File::create(cell_source.join("vendor/foo/lib.go")).unwrap();
        f.write_all(b"package foo\n").unwrap();

        fs::create_dir_all(&repo_root).unwrap();

        let config = CompositionConfig::new("/firefly/turnkey", &repo_root)
            .with_cell(CellConfig::new("godeps", &cell_source).with_editable(true))
            .with_editing(true);

        let state_machine = Arc::new(ConsistencyStateMachine::new());
        let fs = CompositionFs::new(config, repo_root, state_machine);

        // Allocate an inode for a path in the cell
        let cell_path = cell_source.join("vendor/foo/lib.go");
        let ino = fs.get_or_alloc_inode(&cell_path);

        // Should be accepted since cell is editable
        let result = fs.check_edit_allowed(ino);
        assert!(result.is_ok());

        let (cell_name, relative, original) = result.unwrap();
        assert_eq!(cell_name, "godeps");
        assert_eq!(relative, PathBuf::from("vendor/foo/lib.go"));
        assert_eq!(original, cell_path);
    }
}
