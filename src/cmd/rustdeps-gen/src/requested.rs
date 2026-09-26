//! The workspace members' dependency requests.
//!
//! Feature unification in the rustdeps cell starts from what the workspace
//! asks of its registry dependencies: which version, which features, and
//! whether their default features are on. Those specs live in the members'
//! Cargo.toml files, which the cell build never sees, so rustdeps-gen
//! records them in rust-deps.toml as `[[requested]]` entries.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One dependency spec, as Cargo would resolve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The package name (after `package = ...` renames)
    pub name: String,
    /// The version requirement, if the spec has one
    pub version: Option<String>,
    pub default_features: bool,
    pub features: BTreeSet<String>,
}

/// The dependency tables of a manifest that feed Cargo's resolve: normal,
/// dev and build dependencies, and their `[target.*]` variants.
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Every registry dependency request of the workspace rooted at
/// `root_manifest`, merged per (name, version requirement).
pub fn workspace_requests(root_manifest: &Path) -> Result<Vec<Request>> {
    let root = read_manifest(root_manifest)?;
    let workspace = root.get("workspace").and_then(|w| w.as_table());
    let workspace_deps = workspace
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table())
        .cloned()
        .unwrap_or_default();

    let mut requests = Vec::new();
    if root.contains_key("package") {
        requests.extend(member_requests(&root, &workspace_deps));
    }
    let root_dir = root_manifest.parent().unwrap_or(Path::new("."));
    for member_dir in workspace_members(root_dir, workspace)? {
        let member = read_manifest(&member_dir.join("Cargo.toml"))?;
        requests.extend(member_requests(&member, &workspace_deps));
    }
    Ok(merge(requests))
}

fn read_manifest(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    text.parse()
        .with_context(|| format!("Failed to parse {}", path.display()))
}

/// The member directories `[workspace]` lists: `members` globs, less
/// `exclude`, keeping only directories that hold a Cargo.toml.
fn workspace_members(root_dir: &Path, workspace: Option<&toml::Table>) -> Result<Vec<PathBuf>> {
    let strings = |key: &str| string_array(workspace.and_then(|w| w.get(key)));
    let excluded: BTreeSet<PathBuf> = strings("exclude")
        .iter()
        .map(|e| root_dir.join(e))
        .collect();

    let mut members = BTreeSet::new();
    for pattern in strings("members") {
        let pattern = root_dir.join(&pattern);
        let pattern = pattern
            .to_str()
            .context("Non-UTF-8 workspace member path")?;
        for dir in glob::glob(pattern).with_context(|| format!("Bad member glob {pattern}"))? {
            let dir = dir?;
            if dir.join("Cargo.toml").is_file() && !excluded.contains(&dir) {
                members.insert(dir);
            }
        }
    }
    Ok(members.into_iter().collect())
}

/// The registry dependency requests in one member's manifest, with
/// `workspace = true` specs resolved against `[workspace.dependencies]`.
pub fn member_requests(member: &toml::Table, workspace_deps: &toml::Table) -> Vec<Request> {
    let targets = member
        .get("target")
        .and_then(|t| t.as_table())
        .into_iter()
        .flat_map(|t| t.values().filter_map(|v| v.as_table()));
    std::iter::once(member)
        .chain(targets)
        .flat_map(|scope| DEPENDENCY_TABLES.iter().filter_map(|t| scope.get(*t)))
        .filter_map(|table| table.as_table())
        .flat_map(|table| table.iter())
        .filter_map(|(key, spec)| request(key, spec, workspace_deps))
        .collect()
}

fn request(key: &str, spec: &toml::Value, workspace_deps: &toml::Table) -> Option<Request> {
    let inherits = spec.get("workspace").and_then(|w| w.as_bool()) == Some(true);
    if !inherits {
        return registry_request(key, spec);
    }
    let mut request = registry_request(key, workspace_deps.get(key)?)?;
    request.features.extend(string_array(spec.get("features")));
    // A member may turn on defaults the workspace spec turns off, but not
    // the other way around (Cargo warns and keeps them on).
    if default_features(spec) == Some(true) {
        request.default_features = true;
    }
    Some(request)
}

/// A spec as written in a table; None for path and git dependencies, which
/// are not fetched from the registry.
fn registry_request(key: &str, spec: &toml::Value) -> Option<Request> {
    if let Some(version) = spec.as_str() {
        return Some(Request {
            name: key.to_string(),
            version: Some(version.to_string()),
            default_features: true,
            features: BTreeSet::new(),
        });
    }
    let spec = spec.as_table()?;
    if spec.contains_key("path") || spec.contains_key("git") {
        return None;
    }
    let spec = toml::Value::Table(spec.clone());
    Some(Request {
        name: spec
            .get("package")
            .and_then(|p| p.as_str())
            .unwrap_or(key)
            .to_string(),
        version: spec
            .get("version")
            .and_then(|v| v.as_str())
            .map(String::from),
        default_features: default_features(&spec).unwrap_or(true),
        features: string_array(spec.get("features")),
    })
}

fn default_features(spec: &toml::Value) -> Option<bool> {
    spec.get("default-features")
        .or_else(|| spec.get("default_features"))
        .and_then(|d| d.as_bool())
}

fn string_array(value: Option<&toml::Value>) -> BTreeSet<String> {
    value
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Merge requests for the same name and version requirement: defaults are
/// on if any asks for them, and features are unioned. Sorted by name, then
/// version requirement.
pub fn merge(requests: impl IntoIterator<Item = Request>) -> Vec<Request> {
    let mut merged: BTreeMap<(String, Option<String>), Request> = BTreeMap::new();
    for r in requests {
        match merged.entry((r.name.clone(), r.version.clone())) {
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(r);
            }
            std::collections::btree_map::Entry::Occupied(mut e) => {
                let m = e.get_mut();
                m.default_features |= r.default_features;
                m.features.extend(r.features);
            }
        }
    }
    merged.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    fn req(
        name: &str,
        version: Option<&str>,
        default_features: bool,
        features: &[&str],
    ) -> Request {
        Request {
            name: name.to_string(),
            version: version.map(str::to_string),
            default_features,
            features: features.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn plain_and_detailed_specs() {
        let member = table(
            r#"
            [dependencies]
            anyhow = "1"
            tokio = { version = "1", default-features = false, features = ["rt"] }
            "#,
        );
        let mut got = member_requests(&member, &toml::Table::new());
        got.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            got,
            vec![
                req("anyhow", Some("1"), true, &[]),
                req("tokio", Some("1"), false, &["rt"]),
            ]
        );
    }

    #[test]
    fn renamed_dep_requests_the_package() {
        let member = table(r#"dependencies.futures_01 = { package = "futures", version = "0.1" }"#);
        assert_eq!(
            member_requests(&member, &toml::Table::new()),
            vec![req("futures", Some("0.1"), true, &[])]
        );
    }

    #[test]
    fn workspace_inheritance_merges_features() {
        let ws = table(
            r#"
            tokio = { version = "1", features = ["rt"] }
            prost = { version = "0.13", default-features = false }
            "#,
        );
        let member = table(
            r#"
            [dependencies]
            tokio = { workspace = true, features = ["macros"] }
            prost.workspace = true
            "#,
        );
        let mut got = member_requests(&member, &ws);
        got.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            got,
            vec![
                req("prost", Some("0.13"), false, &[]),
                req("tokio", Some("1"), true, &["macros", "rt"]),
            ]
        );
    }

    #[test]
    fn member_can_turn_inherited_defaults_on_but_not_off() {
        let ws = table(
            r#"
            off = { version = "1", default-features = false }
            on = "1"
            "#,
        );
        let member = table(
            r#"
            [dependencies]
            off = { workspace = true, default-features = true }
            on = { workspace = true, default-features = false }
            "#,
        );
        assert!(
            member_requests(&member, &ws)
                .iter()
                .all(|r| r.default_features)
        );
    }

    #[test]
    fn inherited_rename_requests_the_package() {
        let ws = table(r#"http02 = { package = "http", version = "0.2" }"#);
        let member = table(r#"dependencies.http02.workspace = true"#);
        assert_eq!(
            member_requests(&member, &ws),
            vec![req("http", Some("0.2"), true, &[])]
        );
    }

    #[test]
    fn dev_build_and_target_tables_count() {
        let member = table(
            r#"
            dev-dependencies.a = "1"
            build-dependencies.b = "1"
            target.'cfg(unix)'.dependencies.c = "1"
            target.'cfg(unix)'.dev-dependencies.d = "1"
            "#,
        );
        let names: BTreeSet<_> = member_requests(&member, &toml::Table::new())
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["a", "b", "c", "d"].map(String::from).into());
    }

    #[test]
    fn path_and_git_deps_are_not_registry_requests() {
        let ws = table(r#"local = { path = "src/local" }"#);
        let member = table(
            r#"
            [dependencies]
            sibling = { path = "../sibling", version = "0.1" }
            forked = { git = "https://example.com/forked" }
            local.workspace = true
            "#,
        );
        assert_eq!(member_requests(&member, &ws), vec![]);
    }

    #[test]
    fn merge_unions_same_name_and_version() {
        let merged = merge([
            req("tokio", Some("1"), false, &["rt"]),
            req("tokio", Some("1"), true, &["macros"]),
            req("tokio", Some("0.2"), false, &[]),
        ]);
        assert_eq!(
            merged,
            vec![
                req("tokio", Some("0.2"), false, &[]),
                req("tokio", Some("1"), true, &["macros", "rt"]),
            ]
        );
    }

    #[test]
    fn workspace_members_from_globs_and_root_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "Cargo.toml",
            r#"
            [package]
            name = "root"
            [dependencies]
            a = "1"
            [workspace]
            members = ["crates/*"]
            exclude = ["crates/skipped"]
            [workspace.dependencies]
            b = { version = "2", default-features = false }
            "#,
        );
        write(
            "crates/one/Cargo.toml",
            "[package]\nname = \"one\"\n[dependencies]\nb.workspace = true\n",
        );
        write(
            "crates/skipped/Cargo.toml",
            "[package]\nname = \"skipped\"\n[dependencies]\nc = \"3\"\n",
        );
        // A directory the glob matches without a manifest is not a member.
        std::fs::create_dir_all(root.join("crates/empty")).unwrap();

        assert_eq!(
            workspace_requests(&root.join("Cargo.toml")).unwrap(),
            vec![
                req("a", Some("1"), true, &[]),
                req("b", Some("2"), false, &[])
            ]
        );
    }
}
