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

use anyhow::{Result, bail};
use clap::Parser;
use prefetch_cache::{PrefetchCache, nix_prefetch_url};

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

    let cache = if args.no_cache {
        None
    } else {
        match PrefetchCache::new() {
            Ok(cache) => Some(cache),
            Err(e) => {
                if args.verbose {
                    eprintln!("warning: cache unavailable: {}", e);
                }
                None
            }
        }
    };

    let hash = match cache {
        Some(mut cache) => {
            let prefetched = cache.prefetch(&args.url, args.unpack)?;
            if args.verbose {
                let status = if prefetched.cached { "hit" } else { "miss" };
                eprintln!("cache {}: {}", status, args.url);
            }
            prefetched.hash
        }
        // Caching disabled or unavailable - run nix-prefetch-url
        None => nix_prefetch_url(&args.url, args.unpack)?,
    };

    println!("{}", hash);
    Ok(())
}
