//! rustdeps-gen: Generate rust-deps.toml from Cargo.lock
//!
//! This tool parses Cargo.lock and generates a TOML file with Nix-compatible
//! hashes for use with turnkey's rust-deps-cell.nix.
//!
//! The checksums in Cargo.lock are for the .crate tarball, but Nix's fetchzip
//! computes hashes of the unpacked contents. Therefore, this tool prefetches
//! each crate to get the correct Nix hash.

use anyhow::{Context, Result};
use cargo_lock::{Lockfile, Package};
use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Generate rust-deps.toml from Cargo.lock for Buck2/Nix integration
#[derive(Parser, Debug)]
#[command(name = "rustdeps-gen")]
#[command(about = "Generate rust-deps.toml from Cargo.lock")]
struct Args {
    /// Path to Cargo.lock file
    #[arg(long, default_value = "Cargo.lock")]
    cargo_lock: PathBuf,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Skip prefetching (output will have incorrect hashes for fetchzip)
    #[arg(long)]
    no_prefetch: bool,
}

/// Represents a crate with its Nix hash
struct Crate {
    name: String,
    version: String,
    nix_hash: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Parse Cargo.lock
    let lockfile = Lockfile::load(&args.cargo_lock)
        .with_context(|| format!("Failed to load {}", args.cargo_lock.display()))?;

    // Filter to crates.io packages only
    let crates_io_packages: Vec<&Package> = lockfile
        .packages
        .iter()
        .filter(|p| is_crates_io(p))
        .collect();

    eprintln!(
        "Found {} crates from crates.io in Cargo.lock",
        crates_io_packages.len()
    );

    // Build crate list with hashes
    let mut crates: Vec<Crate> = Vec::new();

    if args.no_prefetch {
        eprintln!("WARNING: --no-prefetch produces incorrect hashes for fetchzip");
        eprintln!("The Cargo.lock checksum is for the tarball, not unpacked contents");

        for pkg in &crates_io_packages {
            let nix_hash = pkg
                .checksum
                .as_ref()
                .and_then(|cs| convert_checksum_to_sri(&cs.to_string()));

            crates.push(Crate {
                name: pkg.name.as_str().to_string(),
                version: pkg.version.to_string(),
                nix_hash,
            });
        }
    } else {
        eprintln!(
            "Prefetching {} crates from crates.io...",
            crates_io_packages.len()
        );

        for (i, pkg) in crates_io_packages.iter().enumerate() {
            let name = pkg.name.as_str();
            let version = pkg.version.to_string();

            eprintln!(
                "[{}/{}] prefetching {}@{}...",
                i + 1,
                crates_io_packages.len(),
                name,
                version
            );

            let nix_hash = match prefetch_crate(name, &version) {
                Ok(hash) => Some(hash),
                Err(e) => {
                    eprintln!("    warning: failed to prefetch: {}", e);
                    None
                }
            };

            crates.push(Crate {
                name: name.to_string(),
                version,
                nix_hash,
            });
        }
    }

    // Sort by name for consistent output
    crates.sort_by(|a, b| a.name.cmp(&b.name));

    // Write output
    let output: Box<dyn Write> = match &args.output {
        Some(path) => {
            let file = std::fs::File::create(path)
                .with_context(|| format!("Failed to create {}", path.display()))?;
            Box::new(file)
        }
        None => Box::new(std::io::stdout()),
    };

    write_toml(output, &crates)?;

    if let Some(path) = &args.output {
        eprintln!("Wrote {}", path.display());
    }

    Ok(())
}

/// Check if a package is from crates.io
fn is_crates_io(pkg: &Package) -> bool {
    pkg.source
        .as_ref()
        .map(|s| s.is_default_registry())
        .unwrap_or(false)
}

/// Convert hex checksum to SRI format (for --no-prefetch fallback)
fn convert_checksum_to_sri(hex: &str) -> Option<String> {
    if hex.len() != 64 {
        return None;
    }

    let bytes: Result<Vec<u8>, _> = (0..32)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
        .collect();

    bytes.ok().map(|b| {
        use base64::{engine::general_purpose::STANDARD, Engine};
        format!("sha256-{}", STANDARD.encode(&b))
    })
}

/// Prefetch a crate from crates.io and return its Nix hash (SRI format)
fn prefetch_crate(name: &str, version: &str) -> Result<String> {
    let url = format!(
        "https://crates.io/api/v1/crates/{}/{}/download",
        name, version
    );

    // Use nix-prefetch-cached for automatic caching
    // Falls back to nix-prefetch-url if wrapper not available
    let output = Command::new("nix-prefetch-cached")
        .args(["--unpack", &url])
        .output()
        .or_else(|_| {
            // Fallback to nix-prefetch-url if cached version not available
            Command::new("nix-prefetch-url")
                .args(["--type", "sha256", "--unpack", &url])
                .output()
        })
        .context("Failed to run nix-prefetch-cached or nix-prefetch-url")?;

    if !output.status.success() {
        anyhow::bail!(
            "prefetch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let hash = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 from prefetch")?
        .trim()
        .to_string();

    // nix-prefetch-cached already returns SRI format
    // If using fallback nix-prefetch-url, we need to convert
    if hash.starts_with("sha256-") {
        Ok(hash)
    } else {
        // Convert base32 to SRI format
        let sri_output = Command::new("nix")
            .args(["hash", "to-sri", "--type", "sha256", &hash])
            .output()
            .context("Failed to run nix hash to-sri")?;

        if !sri_output.status.success() {
            return Ok(hash);
        }

        Ok(String::from_utf8(sri_output.stdout)
            .context("Invalid UTF-8 from nix hash")?
            .trim()
            .to_string())
    }
}

/// Write crates as TOML
fn write_toml(mut w: impl Write, crates: &[Crate]) -> Result<()> {
    writeln!(w, "# Auto-generated by rustdeps-gen")?;
    writeln!(w, "# Source: Cargo.lock")?;
    writeln!(w, "#")?;
    writeln!(w, "# To regenerate: rustdeps-gen -o rust-deps.toml")?;
    writeln!(w, "#")?;
    writeln!(w, "# Key format: deps.\"crate-name@version\" to support multiple versions")?;
    writeln!(w)?;

    for c in crates {
        // Use "name@version" as key to handle multiple versions of same crate
        writeln!(w, "[deps.\"{}@{}\"]", c.name, c.version)?;
        writeln!(w, "name = \"{}\"", c.name)?;
        writeln!(w, "version = \"{}\"", c.version)?;
        match &c.nix_hash {
            Some(hash) => writeln!(w, "hash = \"{}\"", hash)?,
            None => writeln!(w, "hash = \"\"")?,
        }
        writeln!(w)?;
    }

    Ok(())
}
