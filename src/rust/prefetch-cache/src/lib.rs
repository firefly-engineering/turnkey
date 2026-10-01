//! Prefetch cache for Nix hash lookups
//!
//! This library provides a shared cache for storing Nix SRI hashes
//! computed during prefetch operations, and the cached lookup itself:
//! [`PrefetchCache::prefetch`] answers from the cache or runs
//! `nix-prefetch-url`. nix-prefetch-cached and the deps generators (through
//! deps-gen-kit) use it to avoid redundant fetching.
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
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

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
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cache directory: {}", parent.display())
            })?;
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
                    eprintln!(
                        "prefetch-cache: failed to parse cache ({}), starting fresh",
                        e
                    );
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

        // Another process may have added entries since this cache was
        // loaded: keep them, with this cache's entries on top
        if let Some(on_disk) = Self::read(&self.cache_path) {
            for (key, entry) in on_disk.entries {
                self.cache.entries.entry(key).or_insert(entry);
            }
        }

        let content =
            serde_json::to_string_pretty(&self.cache).context("Failed to serialize cache")?;

        fs::write(&self.cache_path, content).with_context(|| {
            format!("Failed to write cache file: {}", self.cache_path.display())
        })?;

        self.dirty = false;
        Ok(())
    }

    /// Get the path to the cache file
    pub fn path(&self) -> &Path {
        &self.cache_path
    }

    /// The Nix SRI hash of a URL (of its unpacked contents when `unpack` is
    /// set): from the cache, or else from `nix-prefetch-url`, and then
    /// cached and saved at once, so another process sees it too
    pub fn prefetch(&mut self, url: &str, unpack: bool) -> Result<Prefetched> {
        let key = url_key(url, unpack);
        if let Some(entry) = self.get(&key) {
            return Ok(Prefetched {
                hash: entry.hash.clone(),
                cached: true,
            });
        }

        let hash = nix_prefetch_url(url, unpack)?;
        self.set(key, hash.clone());
        if let Err(e) = self.save() {
            eprintln!("prefetch-cache: warning: failed to save cache: {}", e);
        }
        Ok(Prefetched {
            hash,
            cached: false,
        })
    }

    /// The Nix SRI hashes of many URLs, one result per URL in order:
    /// [`PrefetchCache::prefetch`] for a batch. Hits come from the cache;
    /// the distinct misses are fetched with `fetch`, at most `jobs` at a
    /// time, and the cache is saved once at the end. A failed URL fails its
    /// own result only.
    ///
    /// `fetch` is [`nix_prefetch_url`] or a wrapper of it (to report
    /// progress, or a stand-in in tests).
    pub fn prefetch_all<F>(
        &mut self,
        urls: &[&str],
        unpack: bool,
        jobs: usize,
        fetch: F,
    ) -> Vec<Result<Prefetched>>
    where
        F: Fn(&str, bool) -> Result<String> + Sync,
    {
        let mut misses: Vec<&str> = urls
            .iter()
            .copied()
            .filter(|url| !self.contains(&url_key(url, unpack)))
            .collect();
        misses.sort_unstable();
        misses.dedup();

        // An error isn't Clone, and a URL listed twice gets two results
        let fetched: HashMap<&str, Result<String, String>> = misses
            .iter()
            .copied()
            .zip(fetch_all(&misses, jobs, |url| {
                fetch(url, unpack).map_err(|e| format!("{e:#}"))
            }))
            .collect();
        for (url, result) in &fetched {
            if let Ok(hash) = result {
                self.set(url_key(url, unpack), hash.clone());
            }
        }
        if let Err(e) = self.save() {
            eprintln!("prefetch-cache: warning: failed to save cache: {}", e);
        }

        urls.iter()
            .map(|url| match fetched.get(url) {
                Some(Ok(hash)) => Ok(Prefetched {
                    hash: hash.clone(),
                    cached: false,
                }),
                Some(Err(e)) => Err(anyhow::anyhow!("{e}")),
                None => Ok(Prefetched {
                    hash: self
                        .get(&url_key(url, unpack))
                        .expect("not a miss, so cached")
                        .hash
                        .clone(),
                    cached: true,
                }),
            })
            .collect()
    }

    /// The cache file at `path`, if it is there and readable
    fn read(path: &Path) -> Option<CacheFile> {
        let content = fs::read_to_string(path).ok()?;
        let cache: CacheFile = serde_json::from_str(&content).ok()?;
        (cache.version == CACHE_VERSION).then_some(cache)
    }
}

/// The hash [`PrefetchCache::prefetch`] found, and whether it came from the
/// cache
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefetched {
    pub hash: String,
    pub cached: bool,
}

/// The cache key of a URL's hash: a packed and an unpacked hash of the same
/// URL differ
pub fn url_key(url: &str, unpack: bool) -> String {
    if unpack {
        format!("unpack:{}", url)
    } else {
        url.to_string()
    }
}

/// `f` applied to every item, on at most `jobs` threads at a time, with
/// the results in the items' order
pub fn fetch_all<T, R, F>(items: &[T], jobs: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let result = f(item);
                    results.lock().unwrap()[i] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.expect("every item is fetched"))
        .collect()
}

/// Run nix-prefetch-url, uncached, and return the hash in SRI form
pub fn nix_prefetch_url(url: &str, unpack: bool) -> Result<String> {
    let mut cmd = Command::new("nix-prefetch-url");
    cmd.args(["--type", "sha256"]);

    if unpack {
        cmd.arg("--unpack");
    }

    cmd.arg(url);

    let output = cmd.output().context("Failed to run nix-prefetch-url")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nix-prefetch-url failed: {}", stderr);
    }

    let base32_hash = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 from nix-prefetch-url")?
        .trim()
        .to_string();

    // Convert to SRI format
    let sri_output = Command::new("nix")
        .args(["hash", "to-sri", "--type", "sha256", &base32_hash])
        .output()
        .context("Failed to run nix hash to-sri")?;

    if !sri_output.status.success() {
        // Fallback to base32 if conversion fails (shouldn't happen)
        return Ok(base32_hash);
    }

    Ok(String::from_utf8(sri_output.stdout)
        .context("Invalid UTF-8 from nix hash")?
        .trim()
        .to_string())
}

impl Drop for PrefetchCache {
    fn drop(&mut self) {
        // Auto-save on drop
        if self.dirty
            && let Err(e) = self.save()
        {
            eprintln!("prefetch-cache: warning: failed to save cache: {}", e);
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
    fn test_prefetch_answers_from_cache() {
        let dir = tempdir().unwrap();
        let mut cache = PrefetchCache::with_dir(dir.path()).unwrap();
        let url = "https://example.com/a.tar.gz";
        cache.set(url_key(url, true), "sha256-unpacked".to_string());

        // A hit never runs nix-prefetch-url, which a test can't reach
        assert_eq!(
            cache.prefetch(url, true).unwrap(),
            Prefetched {
                hash: "sha256-unpacked".to_string(),
                cached: true
            }
        );
        assert_ne!(url_key(url, true), url_key(url, false));
    }

    #[test]
    fn test_save_keeps_entries_another_process_added() {
        let dir = tempdir().unwrap();
        let mut ours = PrefetchCache::with_dir(dir.path()).unwrap();
        let mut theirs = PrefetchCache::with_dir(dir.path()).unwrap();

        theirs.set("theirs".to_string(), "sha256-t".to_string());
        theirs.save().unwrap();
        ours.set("ours".to_string(), "sha256-o".to_string());
        ours.save().unwrap();

        let reloaded = PrefetchCache::with_dir(dir.path()).unwrap();
        assert!(reloaded.contains("theirs"));
        assert!(reloaded.contains("ours"));
    }

    #[test]
    fn test_prefetch_all_fetches_each_miss_once_and_keeps_order() {
        let dir = tempdir().unwrap();
        let mut cache = PrefetchCache::with_dir(dir.path()).unwrap();
        cache.set(url_key("https://x/hit", true), "sha256-hit".to_string());

        let calls = Mutex::new(Vec::new());
        let results = cache.prefetch_all(
            &[
                "https://x/a",
                "https://x/hit",
                "https://x/bad",
                "https://x/a",
            ],
            true,
            4,
            |url, unpack| {
                assert!(unpack);
                calls.lock().unwrap().push(url.to_string());
                match url {
                    "https://x/bad" => anyhow::bail!("no such zip"),
                    _ => Ok(format!("sha256-{}", &url[10..])),
                }
            },
        );

        let mut calls = calls.into_inner().unwrap();
        calls.sort();
        assert_eq!(calls, ["https://x/a", "https://x/bad"]);

        let a = Prefetched {
            hash: "sha256-a".to_string(),
            cached: false,
        };
        assert_eq!(results[0].as_ref().unwrap(), &a);
        assert!(results[1].as_ref().unwrap().cached);
        assert!(
            results[2]
                .as_ref()
                .unwrap_err()
                .to_string()
                .contains("no such zip")
        );
        assert_eq!(results[3].as_ref().unwrap(), &a);

        // Saved once, with the hit kept and the failure not cached
        let reloaded = PrefetchCache::with_dir(dir.path()).unwrap();
        assert_eq!(reloaded.len(), 2);
        assert!(reloaded.contains(&url_key("https://x/a", true)));
        assert!(!reloaded.contains(&url_key("https://x/bad", true)));
    }

    #[test]
    fn test_fetch_all_bounds_concurrency_and_keeps_order() {
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let items: Vec<usize> = (0..50).collect();
        let results = fetch_all(&items, 3, |i| {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            thread::sleep(std::time::Duration::from_millis(1));
            running.fetch_sub(1, Ordering::SeqCst);
            i * 2
        });
        assert_eq!(results, (0..50).map(|i| i * 2).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= 3);
        assert!(fetch_all(&[] as &[usize], 0, |i| *i).is_empty());
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
