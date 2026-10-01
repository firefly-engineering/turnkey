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
//!   nix-prefetch-cached --batch [--unpack] [--jobs N] < urls
//!
//! Output is always in SRI format (sha256-...) for Nix compatibility.
//!
//! With --batch, URLs are read one per line from stdin and one line is
//! printed per URL, in order: its hash, or `error: <reason>` if it failed.
//! A failed URL doesn't fail the others or the exit status. The cache is
//! loaded and saved once, and misses are fetched in parallel.

use anyhow::{Context, Result, bail};
use clap::Parser;
use prefetch_cache::{PrefetchCache, fetch_all, nix_prefetch_url};
use std::io::{self, BufRead, Write};

/// Caching wrapper around nix-prefetch-url
#[derive(Parser, Debug)]
#[command(name = "nix-prefetch-cached")]
#[command(about = "Caching wrapper around nix-prefetch-url")]
struct Args {
    /// URL to prefetch
    #[arg(required_unless_present = "batch", conflicts_with = "batch")]
    url: Option<String>,

    /// Read URLs from stdin, one per line, and print one result per line
    #[arg(long)]
    batch: bool,

    /// With --batch, how many cache misses to fetch at once
    #[arg(long, short, default_value_t = 8, requires = "batch")]
    jobs: usize,

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

    if args.batch {
        return batch(&args, cache);
    }
    let url = args
        .url
        .as_deref()
        .expect("clap requires a URL without --batch");

    let hash = match cache {
        Some(mut cache) => {
            let prefetched = cache.prefetch(url, args.unpack)?;
            if args.verbose {
                let status = if prefetched.cached { "hit" } else { "miss" };
                eprintln!("cache {}: {}", status, url);
            }
            prefetched.hash
        }
        // Caching disabled or unavailable - run nix-prefetch-url
        None => nix_prefetch_url(url, args.unpack)?,
    };

    println!("{}", hash);
    Ok(())
}

/// --batch: hash every URL on stdin, one output line per input line
fn batch(args: &Args, cache: Option<PrefetchCache>) -> Result<()> {
    let lines = io::stdin()
        .lock()
        .lines()
        .collect::<io::Result<Vec<_>>>()
        .context("Failed to read URLs from stdin")?;
    let urls: Vec<&str> = lines.iter().map(|l| l.trim()).collect();

    // Misses are slow, so each is reported as it starts
    let fetch = |url: &str, unpack: bool| {
        eprintln!("fetching {}", url);
        nix_prefetch_url(url, unpack)
    };
    let results: Vec<Result<String>> = match cache {
        Some(mut cache) => cache
            .prefetch_all(&urls, args.unpack, args.jobs, fetch)
            .into_iter()
            .zip(&urls)
            .map(|(result, url)| {
                result.map(|prefetched| {
                    if args.verbose {
                        let status = if prefetched.cached { "hit" } else { "miss" };
                        eprintln!("cache {}: {}", status, url);
                    }
                    prefetched.hash
                })
            })
            .collect(),
        None => fetch_all(&urls, args.jobs, |url| fetch(url, args.unpack)),
    };

    let mut out = io::stdout().lock();
    for result in results {
        match result {
            Ok(hash) => writeln!(out, "{}", hash)?,
            // One line per URL, whatever the reason spans
            Err(e) => writeln!(out, "error: {}", format!("{e:#}").replace('\n', " "))?,
        }
    }
    Ok(())
}
