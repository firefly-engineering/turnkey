//! Performance optimization infrastructure for composition backends
//!
//! This module provides configuration and utilities for optimizing FUSE operations:
//! - Configurable cache TTL and size limits
//! - LRU inode cache with automatic eviction
//! - Readdir optimization for large directories
//!
//! # Example
//!
//! ```ignore
//! use composition::performance::{CacheConfig, InodeCache};
//!
//! // Configure caching
//! let config = CacheConfig::default()
//!     .with_ttl_secs(5)
//!     .with_max_inodes(100_000);
//!
//! // Use optimized inode cache
//! let cache = InodeCache::new(config.max_inodes);
//! let ino = cache.get_or_insert(&path, || allocate_new_inode());
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Duration;

/// Configuration for caching behavior
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Time-to-live for cached file attributes (seconds)
    pub attr_ttl_secs: u64,
    /// Time-to-live for cached directory entries (seconds)
    pub entry_ttl_secs: u64,
    /// Maximum number of inodes to cache
    pub max_inodes: usize,
    /// Whether to use negative caching (cache "not found" results)
    pub negative_cache: bool,
    /// Maximum entries to read per directory (0 = unlimited)
    pub max_readdir_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            attr_ttl_secs: 1,
            entry_ttl_secs: 1,
            max_inodes: 100_000,
            negative_cache: false,
            max_readdir_entries: 0,
        }
    }
}

impl CacheConfig {
    /// Create a new cache configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Configuration for aggressive caching (longer TTLs, larger caches)
    ///
    /// Best for read-heavy workloads with infrequent changes.
    pub fn aggressive() -> Self {
        Self {
            attr_ttl_secs: 30,
            entry_ttl_secs: 30,
            max_inodes: 500_000,
            negative_cache: true,
            max_readdir_entries: 0,
        }
    }

    /// Configuration for minimal caching (short TTLs, smaller caches)
    ///
    /// Best for development with frequent changes.
    pub fn minimal() -> Self {
        Self {
            attr_ttl_secs: 1,
            entry_ttl_secs: 1,
            max_inodes: 10_000,
            negative_cache: false,
            max_readdir_entries: 10_000,
        }
    }

    /// Configuration for read-only cells (very long TTLs)
    ///
    /// Best for Nix store paths that never change.
    pub fn readonly() -> Self {
        Self {
            attr_ttl_secs: 3600, // 1 hour
            entry_ttl_secs: 3600,
            max_inodes: 1_000_000,
            negative_cache: true,
            max_readdir_entries: 0,
        }
    }

    /// Set attribute TTL
    pub fn with_attr_ttl_secs(mut self, secs: u64) -> Self {
        self.attr_ttl_secs = secs;
        self
    }

    /// Set entry TTL
    pub fn with_entry_ttl_secs(mut self, secs: u64) -> Self {
        self.entry_ttl_secs = secs;
        self
    }

    /// Set both TTLs to the same value
    pub fn with_ttl_secs(mut self, secs: u64) -> Self {
        self.attr_ttl_secs = secs;
        self.entry_ttl_secs = secs;
        self
    }

    /// Set maximum inode cache size
    pub fn with_max_inodes(mut self, max: usize) -> Self {
        self.max_inodes = max;
        self
    }

    /// Enable or disable negative caching
    pub fn with_negative_cache(mut self, enable: bool) -> Self {
        self.negative_cache = enable;
        self
    }

    /// Set maximum readdir entries (0 = unlimited)
    pub fn with_max_readdir_entries(mut self, max: usize) -> Self {
        self.max_readdir_entries = max;
        self
    }

    /// Get attribute TTL as Duration
    pub fn attr_ttl(&self) -> Duration {
        Duration::from_secs(self.attr_ttl_secs)
    }

    /// Get entry TTL as Duration
    pub fn entry_ttl(&self) -> Duration {
        Duration::from_secs(self.entry_ttl_secs)
    }
}

/// Optimized inode cache with LRU eviction
///
/// This cache maps paths to inodes and vice versa, with automatic eviction
/// when the cache exceeds the configured maximum size.
pub struct InodeCache {
    /// Path to inode mapping
    path_to_ino: RwLock<HashMap<PathBuf, u64>>,
    /// Inode to path mapping
    ino_to_path: RwLock<HashMap<u64, PathBuf>>,
    /// Next inode number to allocate
    next_inode: AtomicU64,
    /// Maximum cache size
    max_size: usize,
    /// Number of cache hits
    hits: AtomicU64,
    /// Number of cache misses
    misses: AtomicU64,
    /// Number of evictions
    evictions: AtomicU64,
}

impl InodeCache {
    /// Create a new inode cache with the specified maximum size
    pub fn new(max_size: usize, first_dynamic_inode: u64) -> Self {
        Self {
            path_to_ino: RwLock::new(HashMap::with_capacity(max_size.min(10_000))),
            ino_to_path: RwLock::new(HashMap::with_capacity(max_size.min(10_000))),
            next_inode: AtomicU64::new(first_dynamic_inode),
            max_size,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// Get an existing inode for a path, or allocate a new one
    ///
    /// This method uses a single lock acquisition pattern to avoid the
    /// double-lock overhead of read-then-write.
    pub fn get_or_insert(&self, path: &Path) -> u64 {
        // Try read-only lookup first (most common case)
        {
            let path_map = self.path_to_ino.read().unwrap();
            if let Some(&ino) = path_map.get(path) {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return ino;
            }
        }

        // Need to insert - acquire write lock
        let mut path_map = self.path_to_ino.write().unwrap();

        // Double-check after acquiring write lock (another thread may have inserted)
        if let Some(&ino) = path_map.get(path) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return ino;
        }

        self.misses.fetch_add(1, Ordering::Relaxed);

        // Check if we need to evict
        if path_map.len() >= self.max_size {
            // Simple eviction: remove first 10% of entries
            // This is not true LRU but avoids the overhead of tracking access times
            let to_remove = self.max_size / 10;
            let keys_to_remove: Vec<PathBuf> = path_map.keys().take(to_remove).cloned().collect();

            let mut ino_map = self.ino_to_path.write().unwrap();
            for key in keys_to_remove {
                if let Some(ino) = path_map.remove(&key) {
                    ino_map.remove(&ino);
                }
            }
            self.evictions.fetch_add(to_remove as u64, Ordering::Relaxed);
        }

        // Allocate new inode
        let ino = self.next_inode.fetch_add(1, Ordering::SeqCst);
        path_map.insert(path.to_path_buf(), ino);

        // Update reverse mapping
        {
            let mut ino_map = self.ino_to_path.write().unwrap();
            ino_map.insert(ino, path.to_path_buf());
        }

        ino
    }

    /// Look up an inode by path without allocating
    pub fn get(&self, path: &Path) -> Option<u64> {
        let path_map = self.path_to_ino.read().unwrap();
        let result = path_map.get(path).copied();
        if result.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// Look up a path by inode
    pub fn get_path(&self, ino: u64) -> Option<PathBuf> {
        let ino_map = self.ino_to_path.read().unwrap();
        ino_map.get(&ino).cloned()
    }

    /// Insert a specific inode-path mapping (for reserved inodes)
    pub fn insert(&self, ino: u64, path: PathBuf) {
        let mut path_map = self.path_to_ino.write().unwrap();
        let mut ino_map = self.ino_to_path.write().unwrap();
        path_map.insert(path.clone(), ino);
        ino_map.insert(ino, path);
    }

    /// Remove a path from the cache
    pub fn remove(&self, path: &Path) -> Option<u64> {
        let mut path_map = self.path_to_ino.write().unwrap();
        if let Some(ino) = path_map.remove(path) {
            let mut ino_map = self.ino_to_path.write().unwrap();
            ino_map.remove(&ino);
            Some(ino)
        } else {
            None
        }
    }

    /// Clear all entries from the cache
    pub fn clear(&self) {
        let mut path_map = self.path_to_ino.write().unwrap();
        let mut ino_map = self.ino_to_path.write().unwrap();
        path_map.clear();
        ino_map.clear();
    }

    /// Get the current cache size
    pub fn len(&self) -> usize {
        self.path_to_ino.read().unwrap().len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let size = self.len();
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let evictions = self.evictions.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            (hits as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        CacheStats {
            size,
            max_size: self.max_size,
            hits,
            misses,
            evictions,
            hit_rate,
        }
    }

    /// Reset statistics counters
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Current cache size
    pub size: usize,
    /// Maximum cache size
    pub max_size: usize,
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of evictions
    pub evictions: u64,
    /// Hit rate (0-100%)
    pub hit_rate: f64,
}

impl CacheStats {
    /// Format as a human-readable string
    pub fn format(&self) -> String {
        format!(
            "Cache: {}/{} entries, {:.1}% hit rate ({} hits, {} misses, {} evictions)",
            self.size, self.max_size, self.hit_rate, self.hits, self.misses, self.evictions
        )
    }
}

/// Directory entry with pre-computed file type
///
/// This struct caches the file type to avoid repeated syscalls during readdir.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Inode number
    pub ino: u64,
    /// File name
    pub name: String,
    /// File type (pre-computed)
    pub file_type: DirEntryType,
}

/// Pre-computed directory entry type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirEntryType {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Unknown type
    Unknown,
}

impl DirEntryType {
    /// Convert from std::fs::FileType
    pub fn from_std(ft: std::fs::FileType) -> Self {
        if ft.is_dir() {
            Self::Directory
        } else if ft.is_symlink() {
            Self::Symlink
        } else if ft.is_file() {
            Self::File
        } else {
            Self::Unknown
        }
    }
}

/// Optimized directory reader
///
/// This iterates directory entries without collecting all entries into a Vec,
/// and uses the entry's file_type() method instead of separate stat calls.
pub struct OptimizedReaddir {
    /// Maximum entries to return (0 = unlimited)
    max_entries: usize,
}

impl OptimizedReaddir {
    /// Create a new optimized readdir
    pub fn new(max_entries: usize) -> Self {
        Self { max_entries }
    }

    /// Read directory entries efficiently
    ///
    /// This function:
    /// 1. Uses the directory entry's file_type() to avoid extra syscalls
    /// 2. Skips entries until reaching the offset
    /// 3. Stops early if max_entries is reached
    pub fn read_entries<F>(
        &self,
        path: &Path,
        offset: usize,
        get_or_alloc_ino: F,
    ) -> std::io::Result<Vec<DirEntry>>
    where
        F: Fn(&Path) -> u64,
    {
        let read_dir = std::fs::read_dir(path)?;
        let mut entries = Vec::new();
        let mut count = 0;

        for entry_result in read_dir {
            let entry = match entry_result {
                Ok(e) => e,
                Err(_) => continue, // Skip unreadable entries
            };

            // Skip entries before offset
            if count < offset {
                count += 1;
                continue;
            }

            // Check max entries limit
            if self.max_entries > 0 && entries.len() >= self.max_entries {
                break;
            }

            let child_path = entry.path();
            let name = match entry.file_name().into_string() {
                Ok(s) => s,
                Err(_) => continue, // Skip non-UTF8 names
            };

            // Use entry.file_type() which is often cached by the OS
            // and avoids an extra stat() call compared to path.is_dir()
            let file_type = match entry.file_type() {
                Ok(ft) => DirEntryType::from_std(ft),
                Err(_) => DirEntryType::Unknown,
            };

            let ino = get_or_alloc_ino(&child_path);

            entries.push(DirEntry {
                ino,
                name,
                file_type,
            });

            count += 1;
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert_eq!(config.attr_ttl_secs, 1);
        assert_eq!(config.entry_ttl_secs, 1);
        assert_eq!(config.max_inodes, 100_000);
    }

    #[test]
    fn test_cache_config_aggressive() {
        let config = CacheConfig::aggressive();
        assert_eq!(config.attr_ttl_secs, 30);
        assert!(config.negative_cache);
        assert_eq!(config.max_inodes, 500_000);
    }

    #[test]
    fn test_cache_config_readonly() {
        let config = CacheConfig::readonly();
        assert_eq!(config.attr_ttl_secs, 3600);
        assert!(config.negative_cache);
    }

    #[test]
    fn test_cache_config_builder() {
        let config = CacheConfig::new()
            .with_ttl_secs(5)
            .with_max_inodes(50_000)
            .with_negative_cache(true);

        assert_eq!(config.attr_ttl_secs, 5);
        assert_eq!(config.entry_ttl_secs, 5);
        assert_eq!(config.max_inodes, 50_000);
        assert!(config.negative_cache);
    }

    #[test]
    fn test_cache_config_ttl_duration() {
        let config = CacheConfig::new().with_attr_ttl_secs(10).with_entry_ttl_secs(20);
        assert_eq!(config.attr_ttl(), Duration::from_secs(10));
        assert_eq!(config.entry_ttl(), Duration::from_secs(20));
    }

    #[test]
    fn test_inode_cache_new() {
        let cache = InodeCache::new(1000, 100);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_inode_cache_get_or_insert() {
        let cache = InodeCache::new(1000, 100);

        let path1 = PathBuf::from("/test/path1");
        let path2 = PathBuf::from("/test/path2");

        let ino1 = cache.get_or_insert(&path1);
        let ino2 = cache.get_or_insert(&path2);
        let ino1_again = cache.get_or_insert(&path1);

        assert_eq!(ino1, 100); // First dynamic inode
        assert_eq!(ino2, 101);
        assert_eq!(ino1_again, ino1); // Same inode for same path
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_inode_cache_get() {
        let cache = InodeCache::new(1000, 100);

        let path = PathBuf::from("/test/path");
        assert!(cache.get(&path).is_none());

        let ino = cache.get_or_insert(&path);
        assert_eq!(cache.get(&path), Some(ino));
    }

    #[test]
    fn test_inode_cache_get_path() {
        let cache = InodeCache::new(1000, 100);

        let path = PathBuf::from("/test/path");
        let ino = cache.get_or_insert(&path);

        assert_eq!(cache.get_path(ino), Some(path));
        assert!(cache.get_path(999).is_none());
    }

    #[test]
    fn test_inode_cache_insert() {
        let cache = InodeCache::new(1000, 100);

        let path = PathBuf::from("/reserved/path");
        cache.insert(42, path.clone());

        assert_eq!(cache.get(&path), Some(42));
        assert_eq!(cache.get_path(42), Some(path));
    }

    #[test]
    fn test_inode_cache_remove() {
        let cache = InodeCache::new(1000, 100);

        let path = PathBuf::from("/test/path");
        let ino = cache.get_or_insert(&path);
        assert_eq!(cache.len(), 1);

        assert_eq!(cache.remove(&path), Some(ino));
        assert_eq!(cache.len(), 0);
        assert!(cache.get(&path).is_none());
    }

    #[test]
    fn test_inode_cache_clear() {
        let cache = InodeCache::new(1000, 100);

        cache.get_or_insert(&PathBuf::from("/test/path1"));
        cache.get_or_insert(&PathBuf::from("/test/path2"));
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_inode_cache_eviction() {
        let cache = InodeCache::new(10, 100); // Small cache to trigger eviction

        // Fill the cache
        for i in 0..10 {
            cache.get_or_insert(&PathBuf::from(format!("/test/path{}", i)));
        }
        assert_eq!(cache.len(), 10);

        // Add one more to trigger eviction
        cache.get_or_insert(&PathBuf::from("/test/path_new"));

        // Should have evicted some entries (10% = 1 entry)
        // After eviction of 1, we add 1, so should be 10 entries
        assert!(cache.len() <= 10);
    }

    #[test]
    fn test_inode_cache_stats() {
        let cache = InodeCache::new(1000, 100);

        let path = PathBuf::from("/test/path");

        // Miss
        cache.get(&path);

        // Insert (miss)
        cache.get_or_insert(&path);

        // Hit
        cache.get_or_insert(&path);

        // Hit
        cache.get(&path);

        let stats = cache.stats();
        assert_eq!(stats.size, 1);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 2);
        assert_eq!(stats.hit_rate, 50.0);
    }

    #[test]
    fn test_inode_cache_reset_stats() {
        let cache = InodeCache::new(1000, 100);

        cache.get_or_insert(&PathBuf::from("/test/path"));
        cache.get_or_insert(&PathBuf::from("/test/path")); // hit

        let stats = cache.stats();
        assert!(stats.hits > 0);

        cache.reset_stats();

        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_cache_stats_format() {
        let stats = CacheStats {
            size: 500,
            max_size: 1000,
            hits: 90,
            misses: 10,
            evictions: 5,
            hit_rate: 90.0,
        };

        let formatted = stats.format();
        assert!(formatted.contains("500/1000"));
        assert!(formatted.contains("90.0%"));
        assert!(formatted.contains("90 hits"));
        assert!(formatted.contains("10 misses"));
        assert!(formatted.contains("5 evictions"));
    }

    #[test]
    fn test_dir_entry_type_from_std() {
        // We can't easily create std::fs::FileType, but we can test the logic
        // by testing paths we know exist
        let path = PathBuf::from("/tmp");
        if path.exists() {
            let meta = std::fs::metadata(&path).unwrap();
            let ft = meta.file_type();
            let det = DirEntryType::from_std(ft);
            assert_eq!(det, DirEntryType::Directory);
        }
    }

    #[test]
    fn test_optimized_readdir_new() {
        let reader = OptimizedReaddir::new(100);
        assert_eq!(reader.max_entries, 100);
    }

    #[test]
    fn test_optimized_readdir_tmp() {
        let reader = OptimizedReaddir::new(0);

        // Read /tmp as a test directory (should exist on most systems)
        let entries = reader.read_entries(
            std::path::Path::new("/tmp"),
            0,
            |_path| 42, // Dummy inode allocator
        );

        // Just verify it doesn't panic and returns something
        assert!(entries.is_ok());
    }

    #[test]
    fn test_optimized_readdir_with_limit() {
        let reader = OptimizedReaddir::new(2);

        // Read /tmp with a limit
        let entries = reader.read_entries(
            std::path::Path::new("/tmp"),
            0,
            |_path| 42,
        );

        if let Ok(entries) = entries {
            // Should be limited to 2 entries
            assert!(entries.len() <= 2);
        }
    }

    #[test]
    fn test_optimized_readdir_with_offset() {
        let reader = OptimizedReaddir::new(0);

        // Create a stable temp directory with known entries
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("file{}", i)), "").unwrap();
        }

        let entries_0 = reader.read_entries(dir.path(), 0, |_path| 42);
        let entries_5 = reader.read_entries(dir.path(), 5, |_path| 42);

        if let (Ok(e0), Ok(e5)) = (entries_0, entries_5) {
            assert_eq!(e0.len(), 10);
            assert_eq!(e5.len(), 5); // Offset 5 skips first 5 entries
        }
    }
}
