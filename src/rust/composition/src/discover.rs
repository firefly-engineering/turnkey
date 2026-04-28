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

/// Build all `*-cell` packages from the flake and return a map of
/// cell name → Nix store path.
///
/// Discovers available packages, filters for `*-cell` entries, builds
/// them all in one invocation, and strips the `-cell` suffix from names.
pub fn build_all_cells(
    client: &dyn NixClient,
    system: &str,
) -> Result<HashMap<String, PathBuf>, nix_eval::NixError> {
    let all_packages = client.list_packages(system)?;

    let cell_packages: Vec<&str> = all_packages
        .iter()
        .filter(|n| n.ends_with("-cell"))
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
    let cells = build_all_cells(client, system)?;

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
