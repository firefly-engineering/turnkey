//! Cell indexes, as nix/lib/deps-cell's mkCellIndex writes them

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::GCROOTS_DIR;
use gostd::path;

/// A cell index: what a deps cell holds
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// The cell's name: it is `.turnkey/<cell>`
    #[serde(default, deserialize_with = "nullable")]
    pub cell: String,
    /// The SHA-256 of the deps file the cell was built from, in hex
    #[serde(default, deserialize_with = "nullable")]
    pub deps_file_sha256: String,
    /// The cell's `.buckconfig`
    #[serde(default, deserialize_with = "nullable")]
    pub buckconfig: String,
    /// Each package's path in the cell, and the package
    #[serde(default, deserialize_with = "nullable")]
    pub packages: BTreeMap<String, Package>,
    /// Each alias package's path, and the package it forwards to
    #[serde(default, deserialize_with = "nullable")]
    pub aliases: BTreeMap<String, String>,
    /// Each forwarding alias package's path, and the label outside the cell
    /// its one target, named as the label's, forwards to
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub forwards: BTreeMap<String, String>,
    /// The cell root package's build file, written as is; empty for a cell
    /// with no root package. A path at the cell root is in no package to
    /// compose.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub root: String,
}

/// One package of the cell
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    /// Its store path
    #[serde(default, deserialize_with = "nullable")]
    pub store: String,
    /// Its directory in the store path (empty for the store path itself)
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub subdir: String,
    /// Its targets
    #[serde(default, deserialize_with = "nullable")]
    pub targets: Vec<String>,
    /// Whether its store path's entries are also linked into its alias
    /// package, for tools that read the cell in place rather than through
    /// buck2 (native forge, through the root remappings.txt)
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "is_false"
    )]
    pub expose: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A `null` decodes as the zero value, as `encoding/json` decodes it into
/// a fresh struct's field
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Why a cell index couldn't be read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError(String);

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReadError {}

/// Reads a cell index
pub fn read_index(path: &Path) -> Result<Index, ReadError> {
    let data =
        std::fs::read(path).map_err(|e| ReadError(format!("reading {}: {e}", path.display())))?;
    let index: Index = serde_json::from_slice(&data)
        .map_err(|e| ReadError(format!("parsing cell index {}: {e}", path.display())))?;
    if index.cell.is_empty() {
        return Err(ReadError(format!(
            "cell index {} names no cell",
            path.display()
        )));
    }
    Ok(index)
}

/// Reads the index a cell was last materialized from: the one its GC root
/// points at
pub fn current_index(root: &Path, cell: &str) -> Result<Index, ReadError> {
    let link = root.join(".turnkey").join(GCROOTS_DIR).join(cell);
    let target = std::fs::read_link(&link).map_err(|e| {
        ReadError(format!(
            "the {cell} cell isn't materialized (no {}): {e}",
            link.display()
        ))
    })?;
    read_index(&target)
}

/// Why [`Index::resolve`] found no store path
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The path is in no package of the cell
    NotInCell {
        /// The path, cleaned
        path: String,
        /// The cell
        cell: String,
    },
    /// The path is under a forwarding alias package: what it forwards to is
    /// the repo's own code, not the cell's
    FirstParty {
        /// The forwarding alias package
        package: String,
        /// The label it forwards to
        label: String,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::NotInCell { path, cell } => {
                write!(f, "{path} is in no package of the {cell} cell")
            }
            ResolveError::FirstParty { package, label } => {
                write!(f, "{package} forwards to {label}: first-party code")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Where [`Index::resolve`] found a path of the cell
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The path of the store path's root in the cell, as the index names it
    pub root: String,
    /// The store path
    pub store: String,
    /// The path within the store path
    pub rest: String,
}

impl Index {
    /// Finds the store path holding a path in the cell (`vendor/<pkg>/...`):
    /// the path of the store path's root in the cell, as the index names it
    /// (an alias package resolves to the package it forwards to:
    /// `vendor/anyhow` to `vendor/anyhow@1.0.100`), the store path, and the
    /// path within it. The root of a store path holding several packages is
    /// the path its packages' subdirectories are under (a Go module's:
    /// `vendor/<module path>`), so a file in a directory that is no package
    /// resolves too. A path under a forwarding alias package is
    /// [`ResolveError::FirstParty`].
    pub fn resolve(&self, rel_path: &str) -> Result<Resolved, ResolveError> {
        let rel_path = path::clean(rel_path);
        // The longest candidate the path is under; of two as long, the
        // first considered
        let mut best: Option<(String, String, String)> = None;
        let mut consider = |candidate: String, store: &str, label: &str| {
            let under = rel_path
                .strip_prefix(candidate.as_str())
                .is_some_and(|rest| rest.starts_with('/'));
            if under
                && best
                    .as_ref()
                    .is_none_or(|(b, _, _)| candidate.len() > b.len())
            {
                best = Some((candidate, store.to_string(), label.to_string()));
            }
        };
        for (path, pkg) in &self.packages {
            consider(root_of(path, pkg), &pkg.store, "");
        }
        for (path, target) in &self.aliases {
            if let Some(pkg) = self.packages.get(target) {
                consider(path.clone(), &pkg.store, "");
            }
        }
        for (path, label) in &self.forwards {
            consider(path.clone(), "", label);
        }
        let Some((best, store, forward)) = best else {
            return Err(ResolveError::NotInCell {
                path: rel_path,
                cell: self.cell.clone(),
            });
        };
        if !forward.is_empty() {
            return Err(ResolveError::FirstParty {
                package: best,
                label: forward,
            });
        }
        let rest = rel_path[best.len() + 1..].to_string();
        // An alias resolves to its package's root
        let root = match self.aliases.get(&best) {
            Some(target) => {
                let pkg = self.packages.get(target).cloned().unwrap_or_default();
                root_of(target, &pkg)
            }
            None => best,
        };
        Ok(Resolved { root, store, rest })
    }
}

/// The path of a package's store path's root in the cell
fn root_of(path: &str, pkg: &Package) -> String {
    if pkg.subdir.is_empty() {
        return path.to_string();
    }
    path.strip_suffix(&format!("/{}", pkg.subdir))
        .unwrap_or(path)
        .to_string()
}
