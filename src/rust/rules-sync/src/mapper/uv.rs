//! The packages a uv workspace's members provide

use super::pymarkers::RootPyproject;
use super::read_toml;
use anyhow::{Result, anyhow};
use std::collections::{HashMap, HashSet};

/// The packages the members of the uv workspace declared by the
/// pyproject.toml in `project_root` provide, as dotted module names keyed
/// to the member's directory relative to `project_root`:
/// {"turnkey.cfg": "src/python/cfg", ...}.
///
/// Packages come from each member's source layout (the member directory,
/// or its src/ directory), so a namespace package shared by several
/// members (PEP 420, a directory without __init__.py) contributes one
/// entry per member subpackage. A module two members both provide is
/// ambiguous and left out.
pub fn load_uv_workspace_modules(project_root: &str) -> Result<HashMap<String, String>> {
    let path = gostd::path::join(&[project_root, "pyproject.toml"]);
    let pyproject: RootPyproject = read_toml(&path)?;
    let mut modules: HashMap<String, String> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for pattern in &pyproject.tool.uv.workspace.members {
        let dirs = super::glob(&gostd::path::join(&[project_root, pattern]))
            .map_err(|err| anyhow!("workspace member {}: {err}", gostd::strconv::quote(pattern)))?;
        for member_dir in dirs {
            if std::fs::metadata(gostd::path::join(&[&member_dir, "pyproject.toml"])).is_err() {
                continue;
            }
            let Ok(rel) = gostd::path::rel(project_root, &member_dir) else {
                continue;
            };
            let src = gostd::path::join(&[&member_dir, "src"]);
            let source_root = if std::fs::metadata(&src).is_ok_and(|m| m.is_dir()) {
                src
            } else {
                member_dir.clone()
            };
            for module in member_packages(&source_root, "") {
                if modules.get(&module).is_some_and(|other| *other != rel) {
                    ambiguous.insert(module.clone());
                }
                modules.insert(module, rel.clone());
            }
        }
    }
    for module in ambiguous {
        modules.remove(&module);
    }
    Ok(modules)
}

/// The entries of `dir`, sorted by name, as `os.ReadDir` lists them: each
/// name and whether it is a directory (not following a symlink)
fn read_dir(dir: &str) -> Option<Vec<(String, bool)>> {
    let mut entries: Vec<(String, bool)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            (e.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    entries.sort();
    Some(entries)
}

/// The dotted names of the packages under `dir`, with `prefix` prepended.
/// A directory holding Python files is a package; one holding only
/// directories is a namespace, whose subdirectories are searched in turn.
fn member_packages(dir: &str, prefix: &str) -> Vec<String> {
    let mut packages = Vec::new();
    for (name, is_dir) in read_dir(dir).unwrap_or_default() {
        if !is_dir || !is_python_identifier(&name) || name == "__pycache__" {
            continue;
        }
        let sub = gostd::path::join(&[dir, &name]);
        if has_python_files(&sub) {
            packages.push(format!("{prefix}{name}"));
        } else {
            packages.extend(member_packages(&sub, &format!("{prefix}{name}.")));
        }
    }
    packages
}

/// Whether `dir` directly holds a .py file
fn has_python_files(dir: &str) -> bool {
    read_dir(dir)
        .unwrap_or_default()
        .iter()
        .any(|(name, is_dir)| !is_dir && name.ends_with(".py"))
}

/// Whether `name` can be a Python package name: ASCII letters, digits and
/// underscores, not starting with a digit
fn is_python_identifier(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// The directory of the workspace member that provides `module`, the
/// member whose package is the longest prefix of it: "turnkey.cargo.toml"
/// is in "src/python/cargo".
pub fn workspace_module_dir<'a>(
    modules: &'a HashMap<String, String>,
    module: &str,
) -> Option<&'a str> {
    let mut name = module;
    while !name.is_empty() {
        if let Some(dir) = modules.get(name) {
            return Some(dir);
        }
        name = &name[..name.rfind('.')?];
    }
    None
}
