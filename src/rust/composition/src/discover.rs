//! Cell discovery and lifecycle management
//!
//! Builds and discovers Nix-backed cells for the composition daemon:
//! - **Build**: Runs `nix build .#<cell>-cell` to produce each cell derivation
//! - **Discover**: Lists available `*-cell` packages from the flake
//! - **Refresh**: Rebuilds cells when dependencies change
//!
//! This uses Nix directly to build cell derivations, without going through
//! the devenv shell. Each cell is built to a specific out-link path,
//! allowing the daemon to control cell placement.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use log::{debug, error, info, warn};

use crate::config::{CellConfig, CompositionConfig};

/// Discover available cell packages from the flake.
///
/// Runs `nix eval .#packages.<system> --apply builtins.attrNames` to list
/// all packages, then filters for `*-cell` entries.
fn list_cell_packages(repo_root: &Path) -> Result<Vec<String>, DiscoverError> {
    let system = current_system();

    let output = Command::new("nix")
        .args([
            "eval",
            &format!(".#packages.{}", system),
            "--apply", "builtins.attrNames",
            "--json",
            "--impure",
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|e| DiscoverError::Nix {
            message: format!("failed to run nix eval: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DiscoverError::Nix {
            message: format!("nix eval failed: {}", stderr.trim()),
        });
    }

    let names: Vec<String> = serde_json::from_slice(&output.stdout).map_err(|e| {
        DiscoverError::Nix {
            message: format!("failed to parse nix eval output: {}", e),
        }
    })?;

    Ok(names
        .into_iter()
        .filter(|n| n.ends_with("-cell"))
        .collect())
}

/// Build a single cell and return its Nix store path.
///
/// Runs `nix build .#<package> --impure --no-link --print-out-paths`.
fn build_cell(repo_root: &Path, package: &str) -> Result<PathBuf, DiscoverError> {
    info!("Building cell: {}", package);

    let output = Command::new("nix")
        .args([
            "build",
            &format!(".#{}", package),
            "--impure",
            "--no-link",
            "--print-out-paths",
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|e| DiscoverError::Nix {
            message: format!("failed to run nix build .#{}: {}", package, e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DiscoverError::Nix {
            message: format!("nix build .#{} failed: {}", package, stderr.trim()),
        });
    }

    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path_str.is_empty() {
        return Err(DiscoverError::Nix {
            message: format!("nix build .#{} returned no output path", package),
        });
    }

    debug!("  {} -> {}", package, path_str);
    Ok(PathBuf::from(path_str))
}

/// Build all cells in a single `nix build` invocation and return a map of
/// cell name → Nix store path.
///
/// Discovers available `*-cell` packages from the flake, builds them all
/// at once (avoiding Nix lock contention from sequential builds), and
/// returns the mapping. The cell name has the `-cell` suffix stripped
/// (e.g., `godeps-cell` → `godeps`).
pub fn build_all_cells(repo_root: &Path) -> Result<HashMap<String, PathBuf>, DiscoverError> {
    let packages = list_cell_packages(repo_root)?;

    if packages.is_empty() {
        warn!("No *-cell packages found in the flake");
        return Ok(HashMap::new());
    }

    info!("Building {} cells: {}", packages.len(), packages.join(", "));

    // Build all cells in one nix build invocation
    let mut args = vec![
        "build".to_string(),
        "--impure".to_string(),
        "--no-link".to_string(),
        "--print-out-paths".to_string(),
    ];
    for pkg in &packages {
        args.push(format!(".#{}", pkg));
    }

    let output = Command::new("nix")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .map_err(|e| DiscoverError::Nix {
            message: format!("failed to run nix build: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DiscoverError::Nix {
            message: format!("nix build failed: {}", stderr.trim()),
        });
    }

    // Parse output paths (one per line, in same order as packages)
    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    let mut cells = HashMap::new();
    for (pkg, path) in packages.iter().zip(paths.iter()) {
        let name = pkg.strip_suffix("-cell").unwrap_or(pkg).to_string();
        debug!("  {} -> {}", name, path);
        cells.insert(name, PathBuf::from(path));
    }

    info!("Built {} cells", cells.len());
    Ok(cells)
}

/// Build all cells and return a CompositionConfig.
pub fn build_and_configure(
    mount_point: &Path,
    repo_root: &Path,
) -> Result<CompositionConfig, DiscoverError> {
    let cells = build_all_cells(repo_root)?;

    let mut config = CompositionConfig::new(mount_point, repo_root);
    for (name, path) in &cells {
        config = config.with_cell(CellConfig::new(name, path));
    }

    Ok(config)
}

/// Get the current Nix system string (e.g., "aarch64-darwin", "x86_64-linux").
fn current_system() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "aarch64-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "x86_64-darwin" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "x86_64-linux" }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "aarch64-linux" }
}

#[derive(Debug)]
pub enum DiscoverError {
    Nix { message: String },
}

impl std::fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoverError::Nix { message } => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for DiscoverError {}
