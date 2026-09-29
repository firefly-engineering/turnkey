//! Cell discovery and lifecycle management
//!
//! Builds and discovers Nix-backed cells for the composition daemon
//! using the `nix_eval::NixClient` trait. The implementation is decoupled
//! from how Nix is invoked — currently via CLI, replaceable with a direct
//! daemon client.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use log::{info, warn};
use nix_eval::NixClient;

use crate::config::{CellConfig, CompositionConfig};

/// The file `tk materialize` writes into a write-once deps cell
/// (src/go/pkg/materialize: the deps file's hash)
const WRITE_ONCE_MARKER: &str = ".deps-file-sha256";

/// The write-once deps cells of a repo (ADR 0004): each `.turnkey/<cell>`
/// that is a real directory holding the materializer's marker, sorted by name
pub fn write_once_cells(repo_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(repo_root.join(".turnkey")) else {
        return Vec::new();
    };
    let mut cells: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.path().join(WRITE_ONCE_MARKER).is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    cells.sort();
    cells
}

/// Build all `*-cell` packages from the flake and return a map of
/// cell name → Nix store path.
///
/// Discovers available packages, filters for `*-cell` entries, builds
/// them all in one invocation, and strips the `-cell` suffix from names.
pub fn build_all_cells(
    client: &dyn NixClient,
    system: &str,
    repo_root: &Path,
) -> Result<HashMap<String, PathBuf>, nix_eval::NixError> {
    let all_packages = client.list_packages(system)?;

    // A write-once cell isn't served from its export: the mount maps it to
    // the repo's own directory (write_once_cells)
    let write_once = write_once_cells(repo_root);
    let cell_packages: Vec<&str> = all_packages
        .iter()
        .filter(|n| {
            n.strip_suffix("-cell")
                .is_some_and(|name| !write_once.iter().any(|w| w == name))
        })
        .map(|s| s.as_str())
        .collect();

    if cell_packages.is_empty() {
        warn!("No *-cell packages found in the flake");
        return Ok(HashMap::new());
    }

    info!(
        "Building {} cells: {}",
        cell_packages.len(),
        cell_packages.join(", ")
    );

    let built = client.build(&cell_packages)?;

    // Strip the -cell suffix from package names
    Ok(built
        .into_iter()
        .map(|(k, v)| {
            let name = k.strip_suffix("-cell").unwrap_or(&k).to_string();
            (name, v)
        })
        .collect())
}

/// Build all cells and the toolchain profile, return a fully configured `CompositionConfig`.
pub fn build_and_configure(
    client: &dyn NixClient,
    mount_point: &Path,
    repo_root: &Path,
) -> Result<CompositionConfig, nix_eval::NixError> {
    let system = nix_eval::current_system();
    let cells = build_all_cells(client, system, repo_root)?;

    // Also build the toolchain profile for bin/ exposure
    let all_packages = client.list_packages(system)?;
    let toolchain_profile = if all_packages.iter().any(|p| p == "toolchain-profile") {
        info!("Building toolchain profile...");
        match client.build(&["toolchain-profile"]) {
            Ok(built) => built.get("toolchain-profile").cloned(),
            Err(e) => {
                warn!("Failed to build toolchain profile: {}", e);
                None
            }
        }
    } else {
        None
    };

    let mut config = CompositionConfig::new(mount_point, repo_root);
    for (name, path) in &cells {
        config = config.with_cell(CellConfig::new(name, path));
    }
    for name in write_once_cells(repo_root) {
        info!("Serving write-once cell {} from .turnkey/{}", name, name);
        config = config.with_write_once_cell(name);
    }
    config.toolchain_profile = toolchain_profile;

    // Also discover the toolchains cell from .turnkey/toolchains symlink
    // (built by devenv, not exposed as a flake package)
    let toolchains_link = repo_root.join(".turnkey/toolchains");
    if toolchains_link.is_symlink() {
        if let Ok(target) = std::fs::read_link(&toolchains_link) {
            if target.to_string_lossy().starts_with("/nix/store/") {
                info!("Discovered toolchains cell from .turnkey/toolchains symlink");
                config = config.with_cell(CellConfig::new("toolchains", &target));
            }
        }
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn write_once_cells_are_marked_real_directories() {
        let repo = tempfile::TempDir::new().unwrap();
        let turnkey = repo.path().join(".turnkey");
        // A materialized cell, one being switched over (no marker yet), a
        // cell that is still a store symlink, and a symlink to a marked
        // directory
        fs::create_dir_all(turnkey.join("rustdeps")).unwrap();
        fs::write(turnkey.join("rustdeps").join(WRITE_ONCE_MARKER), "abc\n").unwrap();
        fs::create_dir_all(turnkey.join("pydeps")).unwrap();
        let store = repo.path().join("store-godeps");
        fs::create_dir_all(&store).unwrap();
        fs::write(store.join(WRITE_ONCE_MARKER), "").unwrap();
        std::os::unix::fs::symlink(&store, turnkey.join("godeps")).unwrap();

        assert_eq!(write_once_cells(repo.path()), vec!["rustdeps".to_string()]);
        assert!(write_once_cells(&repo.path().join("nowhere")).is_empty());
    }
}
