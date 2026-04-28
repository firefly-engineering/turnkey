//! Cell discovery and lifecycle management
//!
//! Manages the Nix-backed cells in `.turnkey/`:
//! - **Bootstrap**: Runs `nix develop` to generate/refresh symlinks
//! - **Discover**: Reads symlinks to build cell configuration
//! - **Refresh**: Re-bootstraps and re-discovers when dependencies change
//!
//! The daemon always bootstraps on startup to ensure symlinks are current,
//! then watches for changes that require re-generation.

use std::path::{Path, PathBuf};
use std::process::Command;

use log::{info, warn};

use crate::config::{CellConfig, CompositionConfig};

/// Names in `.turnkey/` that are NOT cells
const SKIP_NAMES: &[&str] = &[
    "sync.toml",
    "compose.toml",
    "books",
    "pycache",
    "edits",
    "patches",
    ".cell-targets",
    "tmp",
];

/// Bootstrap `.turnkey/` symlinks by running `nix develop`.
///
/// This triggers the devenv shell entry script which creates/updates
/// all cell symlinks, the toolchains cell, and config files.
/// Always runs — ensures symlinks match the current flake.lock and deps files.
pub fn bootstrap(repo_root: &Path) -> Result<(), DiscoverError> {
    info!(
        "Bootstrapping .turnkey/ via nix develop in {}",
        repo_root.display()
    );

    let output = Command::new("nix")
        .args(["develop", "--impure", "-c", "true"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| DiscoverError::Bootstrap {
            message: format!("failed to run nix develop: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DiscoverError::Bootstrap {
            message: format!("nix develop failed: {}", stderr.trim()),
        });
    }

    info!("Bootstrap complete");
    Ok(())
}

/// Discover cells from `.turnkey/` symlinks.
///
/// Reads all symlinks in `.turnkey/` that point to `/nix/store/` paths.
/// The `toolchains` symlink is included (Buck2 needs it as a cell).
pub fn discover_cells(
    mount_point: &Path,
    repo_root: &Path,
) -> Result<CompositionConfig, DiscoverError> {
    let turnkey_dir = repo_root.join(".turnkey");

    if !turnkey_dir.exists() {
        return Err(DiscoverError::NoTurnkeyDir(turnkey_dir));
    }

    let mut config = CompositionConfig::new(mount_point, repo_root);

    let entries = std::fs::read_dir(&turnkey_dir).map_err(|e| DiscoverError::Io {
        path: turnkey_dir.clone(),
        source: e,
    })?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if SKIP_NAMES.contains(&name_str.as_ref()) {
            continue;
        }

        let path = entry.path();

        if path.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let target_str = target.to_string_lossy();
                if target_str.starts_with("/nix/store/") {
                    info!("Discovered cell: {} -> {}", name_str, target_str);
                    let cell = CellConfig::new(name_str.as_ref(), &target);
                    config = config.with_cell(cell);
                }
            }
        }
    }

    if config.cells.is_empty() {
        warn!("No cells discovered in {}", turnkey_dir.display());
    }

    Ok(config)
}

/// Bootstrap and then discover cells.
///
/// Always bootstraps first to ensure symlinks are current, then reads them.
pub fn bootstrap_and_discover(
    mount_point: &Path,
    repo_root: &Path,
) -> Result<CompositionConfig, DiscoverError> {
    bootstrap(repo_root)?;
    discover_cells(mount_point, repo_root)
}

#[derive(Debug)]
pub enum DiscoverError {
    NoTurnkeyDir(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Bootstrap {
        message: String,
    },
}

impl std::fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoverError::NoTurnkeyDir(p) => {
                write!(f, ".turnkey/ directory not found at {}", p.display())
            }
            DiscoverError::Io { path, source } => {
                write!(f, "failed to read {}: {}", path.display(), source)
            }
            DiscoverError::Bootstrap { message } => {
                write!(f, "bootstrap failed: {}", message)
            }
        }
    }
}

impl std::error::Error for DiscoverError {}
