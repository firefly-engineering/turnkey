//! nix-prefetch-cached: Caching wrapper around nix-prefetch-url
//!
//! This tool wraps `nix-prefetch-url` with a persistent cache to avoid
//! redundant network fetches. It's designed to be a drop-in replacement
//! for common `nix-prefetch-url` usage patterns.
//!
//! Cache location:
//! - Default: ~/.cache/turnkey/prefetch-cache.json
//! - Override with TURNKEY_CACHE_DIR env var
//!
//! Usage:
//!   nix-prefetch-cached [--unpack] [--type sha256] <url>
//!
//! Output is always in SRI format (sha256-...) for Nix compatibility.

use anyhow::{bail, Context, Result};
use clap::Parser;
use prefetch_cache::PrefetchCache;
use std::process::Command;

/// Caching wrapper around nix-prefetch-url
#[derive(Parser, Debug)]
#[command(name = "nix-prefetch-cached")]
#[command(about = "Caching wrapper around nix-prefetch-url")]
struct Args {
    /// URL to prefetch
    url: String,

    /// Unpack the archive (like nix-prefetch-url --unpack)
    #[arg(long)]
    unpack: bool,

    /// Hash type (only sha256 is supported)
    #[arg(long, default_value = "sha256")]
    r#type: String,

    /// Disable caching (always fetch fresh)
    #[arg(long)]
    no_cache: bool,

    /// Print cache status to stderr
    #[arg(long, short)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Only support sha256
    if args.r#type != "sha256" {
        bail!("Only sha256 hash type is supported");
    }

    // Build cache key from URL and unpack flag
    // The key needs to distinguish between packed and unpacked hashes
    let cache_key = if args.unpack {
        format!("unpack:{}", args.url)
    } else {
        args.url.clone()
    };

    // Try to use cache
    if !args.no_cache {
        match PrefetchCache::new() {
            Ok(cache) => {
                if let Some(entry) = cache.get(&cache_key) {
                    if args.verbose {
                        eprintln!("cache hit: {}", args.url);
                    }
                    println!("{}", entry.hash);
                    return Ok(());
                }
                if args.verbose {
                    eprintln!("cache miss: {}", args.url);
                }
            }
            Err(e) => {
                if args.verbose {
                    eprintln!("warning: cache unavailable: {}", e);
                }
            }
        }
    }

    // Cache miss or caching disabled - run nix-prefetch-url
    let hash = prefetch_url(&args.url, args.unpack)?;

    // Store in cache
    if !args.no_cache {
        if let Ok(mut cache) = PrefetchCache::new() {
            cache.set(cache_key, hash.clone());
            if let Err(e) = cache.save() {
                if args.verbose {
                    eprintln!("warning: failed to save cache: {}", e);
                }
            }
        }
    }

    println!("{}", hash);
    Ok(())
}

/// Run nix-prefetch-url and return SRI hash
fn prefetch_url(url: &str, unpack: bool) -> Result<String> {
    let mut cmd = Command::new("nix-prefetch-url");
    cmd.args(["--type", "sha256"]);

    if unpack {
        cmd.arg("--unpack");
    }

    cmd.arg(url);

    let output = cmd.output().context("Failed to run nix-prefetch-url")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix-prefetch-url failed: {}", stderr);
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
