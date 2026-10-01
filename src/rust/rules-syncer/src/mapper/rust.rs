//! The Rust plug-in: a crate's deps and features from its Cargo.toml

use super::cargo::{CargoWorkspace, load_cargo_workspace};
use super::{
    Dimensions, Language, PackageMapping, Request, Rule, TargetKind, exists, platform, read_toml,
};
use anyhow::{Context, Result};
use conditions::Space;
use project_sync::config;
use rules_star::File;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The project's Rust configuration
#[derive(Debug, Clone, Default)]
pub struct RustConfig {
    /// The Buck2 cell of external deps, e.g. "rustdeps"
    pub external_cell: String,
    /// The crate names of rust-deps.toml
    pub external_deps: HashSet<String>,
    /// The package names of the workspace's members, keyed to their
    /// directories relative to the project root
    pub workspace_packages: HashMap<String, String>,
    /// The workspace root, whose `[workspace.dependencies]` workspace =
    /// true entries inherit from
    pub(super) workspace: Option<CargoWorkspace>,
}

/// The attributes of turnkey's prelude Rust rules that make a target a
/// variant: a Cargo-style request for features
pub(super) const CARGO_VARIANT_ATTRIBUTES: &[&str] = &["cargo_features", "default_features"];

/// The Rust rule kinds. A Rust target can ask for features in Cargo's
/// terms, and a dep pinning a crate's version stands for the crate's
/// target.
fn rust_rule(rule: &str) -> Option<Rule> {
    let kind = match rule {
        "rust_library" => TargetKind::Library,
        "rust_binary" => TargetKind::Binary,
        "rust_test" => TargetKind::Test,
        _ => return None,
    };
    Some(Rule {
        kind,
        deps_attribute: "deps",
        variant: CARGO_VARIANT_ATTRIBUTES,
        canonical: Some(unversioned),
    })
}

/// A label with an @version suffix stripped from its package: the
/// rustdeps cell's "rustdeps//vendor/tokio@1.50.0:tokio" pins a version of
/// "rustdeps//vendor/tokio:tokio".
pub fn unversioned(label: &str) -> String {
    let (pkg, name) = match label.split_once(':') {
        Some((pkg, name)) => (pkg, Some(name)),
        None => (label, None),
    };
    let slash = pkg.rfind('/');
    let pkg = match pkg.rfind('@') {
        Some(at) if at > 0 && slash.is_none_or(|slash| at > slash) => &pkg[..at],
        _ => pkg,
    };
    match name {
        Some(name) => format!("{pkg}:{name}"),
        None => pkg.to_string(),
    }
}

/// Resolves a Rust crate's deps from its Cargo.toml
pub struct RustLanguage {
    pub(super) project_root: String,
    pub(super) cfg: Option<RustConfig>,
    /// The configurations a member target's variant is read in: the
    /// platforms, all a crate's deps depend on
    pub(super) space: Space,
    /// The members' parsed rules.star, by member directory
    rules: RefCell<HashMap<String, File>>,
}

impl RustLanguage {
    pub fn new(mcfg: &super::Config, lang: &config::Language) -> Self {
        RustLanguage {
            project_root: mcfg.project_root.clone(),
            cfg: detect_rust_config(&mcfg.project_root, lang),
            space: mcfg.conditions.space(),
            rules: RefCell::new(HashMap::new()),
        }
    }

    /// The parsed rules.star of the member in `member_dir`
    pub(super) fn member_rules<T>(
        &self,
        member_dir: &str,
        f: impl FnOnce(&File) -> T,
    ) -> Result<T, rules_star::Error> {
        if let Some(file) = self.rules.borrow().get(member_dir) {
            return Ok(f(file));
        }
        let path = gostd::path::join(&[&self.project_root, member_dir, "rules.star"]);
        let file = rules_star::parse_file(std::path::Path::new(&path))?;
        let result = f(&file);
        self.rules.borrow_mut().insert(member_dir.to_string(), file);
        Ok(result)
    }
}

impl Language for RustLanguage {
    fn name(&self) -> &str {
        "rust"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        rust_rule(rule)
    }

    fn source_patterns(&self) -> &'static [&'static str] {
        &["*.rs", "Cargo.toml"]
    }

    /// A crate's target-specific tables, and its dependencies' member
    /// targets' variants, depend on the platform.
    fn dimensions(&self, _: &str) -> Result<Dimensions> {
        Ok(platform())
    }

    fn resolve_deps(&self, crate_dir: &str, req: &Request) -> Result<PackageMapping> {
        self.resolve_crate(crate_dir, req)
            .context("reading Cargo.toml")
    }
}

/// The project's Rust configuration, with the language's cell and deps
/// file: none without a Cargo.toml at its root
fn detect_rust_config(project_root: &str, lang: &config::Language) -> Option<RustConfig> {
    if !exists(&gostd::path::join(&[project_root, "Cargo.toml"])) {
        return None;
    }
    let mut cfg = RustConfig {
        external_cell: lang.cell.clone(),
        ..RustConfig::default()
    };
    // A root Cargo.toml without [workspace] is a single crate: nothing to
    // inherit from and no members.
    if let Ok((ws, members)) = load_cargo_workspace(project_root, project_root) {
        cfg.workspace = Some(ws);
        cfg.workspace_packages = members;
    }
    if let Ok(deps) = load_rust_deps(&gostd::path::join(&[project_root, &lang.deps_file])) {
        cfg.external_deps = deps;
    }
    Some(cfg)
}

/// rust-deps.toml's `[deps]` table, by key
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct RustDeps {
    deps: BTreeMap<String, toml::Value>,
}

/// The crate names of rust-deps.toml, whose keys are "crate@version"
fn load_rust_deps(path: &str) -> Result<HashSet<String>> {
    let file: RustDeps = read_toml(path)?;
    Ok(file
        .deps
        .into_keys()
        .map(|key| match key.find('@') {
            Some(at) if at > 0 => key[..at].to_string(),
            _ => key,
        })
        .collect())
}
