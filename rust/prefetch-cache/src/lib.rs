//! Prefetch cache for Nix hash lookups
//!
//! This library provides a shared cache for storing Nix SRI hashes
//! computed during prefetch operations. It's used by godeps-gen,
//! rustdeps-gen, and pydeps-gen to avoid redundant fetching.
//!
//! Cache location (in order of precedence):
//! 1. `--cache-dir` CLI flag
//! 2. `TURNKEY_CACHE_DIR` environment variable
//! 3. `~/.cache/turnkey/` (default)
//!
//! The cache file is `prefetch-cache.json`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Current cache format version
const CACHE_VERSION: u32 = 1;

/// Default cache filename
const CACHE_FILENAME: &str = "prefetch-cache.json";

/// Environment variable for cache directory override
const CACHE_DIR_ENV: &str = "TURNKEY_CACHE_DIR";

/// Cache entry storing a prefetched hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// The Nix SRI hash (e.g., "sha256-...")
    pub hash: String,
    /// When this entry was fetched
    pub fetched_at: DateTime<Utc>,
}

/// The cache file format
#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    /// Format version for future compatibility
    version: u32,
    /// Map of cache keys to entries
    entries: HashMap<String, CacheEntry>,
}

impl Default for CacheFile {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: HashMap::new(),
        }
    }
}

/// Prefetch cache for storing and retrieving Nix hashes
pub struct PrefetchCache {
    /// Path to the cache file
    cache_path: PathBuf,
    /// In-memory cache contents
    cache: CacheFile,
    /// Whether the cache has been modified
    dirty: bool,
}

impl PrefetchCache {
    /// Create a new cache with the default location (~/.cache/turnkey/)
    pub fn new() -> Result<Self> {
        let cache_dir = Self::default_cache_dir()?;
        Self::with_dir(&cache_dir)
    }

    /// Create a new cache with a custom directory
    pub fn with_dir(cache_dir: &Path) -> Result<Self> {
        let cache_path = cache_dir.join(CACHE_FILENAME);

        // Create cache directory if it doesn't exist
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache directory: {}", parent.display()))?;
        }

        // Load existing cache or create new one
        let cache = if cache_path.exists() {
            let content = fs::read_to_string(&cache_path)
                .with_context(|| format!("Failed to read cache file: {}", cache_path.display()))?;

            match serde_json::from_str::<CacheFile>(&content) {
                Ok(cache) => {
                    // Check version compatibility
                    if cache.version != CACHE_VERSION {
                        eprintln!(
                            "prefetch-cache: cache version mismatch (found {}, expected {}), starting fresh",
                            cache.version, CACHE_VERSION
                        );
                        CacheFile::default()
                    } else {
                        cache
                    }
                }
                Err(e) => {
                    eprintln!("prefetch-cache: failed to parse cache ({}), starting fresh", e);
                    CacheFile::default()
                }
            }
        } else {
            CacheFile::default()
        };

        Ok(Self {
            cache_path,
            cache,
            dirty: false,
        })
    }

    /// Get the default cache directory
    pub fn default_cache_dir() -> Result<PathBuf> {
        // Check environment variable first
        if let Ok(dir) = std::env::var(CACHE_DIR_ENV) {
            return Ok(PathBuf::from(dir));
        }

        // Use platform-specific cache directory
        dirs::cache_dir()
            .map(|d| d.join("turnkey"))
            .ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))
    }

    /// Build a cache key for a package
    ///
    /// Format: `{source}/{name}/{version}`
    /// Examples:
    /// - `crates.io/serde/1.0.228`
    /// - `pypi.org/requests/2.31.0`
    /// - `proxy.golang.org/github.com/spf13/cobra/v1.8.0`
    pub fn make_key(source: &str, name: &str, version: &str) -> String {
        format!("{}/{}/{}", source, name, version)
    }

    /// Get a cached hash for a package
    pub fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.cache.entries.get(key)
    }

    /// Store a hash in the cache
    pub fn set(&mut self, key: String, hash: String) {
        self.cache.entries.insert(
            key,
            CacheEntry {
                hash,
                fetched_at: Utc::now(),
            },
        );
        self.dirty = true;
    }

    /// Check if a key exists in the cache
    pub fn contains(&self, key: &str) -> bool {
        self.cache.entries.contains_key(key)
    }

    /// Get the number of entries in the cache
    pub fn len(&self) -> usize {
        self.cache.entries.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.entries.is_empty()
    }

    /// Save the cache to disk if modified
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let content = serde_json::to_string_pretty(&self.cache)
            .context("Failed to serialize cache")?;

        fs::write(&self.cache_path, content)
            .with_context(|| format!("Failed to write cache file: {}", self.cache_path.display()))?;

        self.dirty = false;
        Ok(())
    }

    /// Get the path to the cache file
    pub fn path(&self) -> &Path {
        &self.cache_path
    }
}

impl Drop for PrefetchCache {
    fn drop(&mut self) {
        // Auto-save on drop
        if self.dirty {
            if let Err(e) = self.save() {
                eprintln!("prefetch-cache: warning: failed to save cache: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cache_roundtrip() {
        let dir = tempdir().unwrap();
        let mut cache = PrefetchCache::with_dir(dir.path()).unwrap();

        // Add some entries
        let key = PrefetchCache::make_key("crates.io", "serde", "1.0.228");
        cache.set(key.clone(), "sha256-abc123".to_string());

        assert!(cache.contains(&key));
        assert_eq!(cache.get(&key).unwrap().hash, "sha256-abc123");

        // Save and reload
        cache.save().unwrap();
        drop(cache);

        let cache2 = PrefetchCache::with_dir(dir.path()).unwrap();
        assert!(cache2.contains(&key));
        assert_eq!(cache2.get(&key).unwrap().hash, "sha256-abc123");
    }

    #[test]
    fn test_make_key() {
        assert_eq!(
            PrefetchCache::make_key("crates.io", "serde", "1.0.228"),
            "crates.io/serde/1.0.228"
        );
        assert_eq!(
            PrefetchCache::make_key("pypi.org", "requests", "2.31.0"),
            "pypi.org/requests/2.31.0"
        );
    }
}
