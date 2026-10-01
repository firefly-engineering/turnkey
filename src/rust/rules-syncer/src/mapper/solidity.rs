//! The Solidity plug-in: deps from the imports deps-extract finds in a
//! package's sources

use super::typescript::{load_package_names, npm_package_name};
use super::{
    DependencyType, Dimensions, ImportLanguage, Language, MappedDep, PackageMapping, Request, Rule,
    TargetKind, exists, resolve_with_deps_extract, skipped_dep, unmapped_dep,
};
use anyhow::Result;
use deps_extract::extraction::{Import, ImportKind};
use project_sync::config;
use project_sync::launch::Launcher;
use std::collections::HashSet;

/// The project's Solidity configuration
#[derive(Debug, Clone, Default)]
pub struct SolidityConfig {
    /// The Buck2 cell of external deps, e.g. "soldeps"
    pub external_cell: String,
    /// The names of the packages in solidity-deps.toml
    pub external_deps: HashSet<String>,
}

/// The Solidity rule kinds. A solidity_contract's deps are not synced.
fn solidity_rule(rule: &str) -> Option<Rule> {
    let (kind, deps_attribute) = match rule {
        "solidity_library" => (TargetKind::Library, "deps"),
        "solidity_contract" => (TargetKind::NotSynced, ""),
        "solidity_test" => (TargetKind::Test, "deps"),
        _ => return None,
    };
    Some(Rule {
        kind,
        deps_attribute,
        ..Rule::default()
    })
}

/// Resolves a Solidity package's deps from the imports deps-extract finds
/// in its sources
pub struct SolidityLanguage {
    project_root: String,
    cfg: Option<SolidityConfig>,
    launcher: Launcher,
}

impl SolidityLanguage {
    pub fn new(mcfg: &super::Config, lang: &config::Language) -> Self {
        SolidityLanguage {
            project_root: mcfg.project_root.clone(),
            cfg: detect_solidity_config(&mcfg.project_root, lang),
            launcher: mcfg.launcher.clone(),
        }
    }
}

impl Language for SolidityLanguage {
    fn name(&self) -> &str {
        "solidity"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        solidity_rule(rule)
    }

    fn source_patterns(&self) -> &'static [&'static str] {
        &["*.sol"]
    }

    fn dimensions(&self, _: &str) -> Result<Dimensions> {
        Ok(Dimensions::default())
    }

    fn resolve_deps(&self, pkg_dir: &str, _: &Request) -> Result<PackageMapping> {
        resolve_with_deps_extract(
            self,
            &self.launcher,
            self.name(),
            &self.project_root,
            pkg_dir,
        )
    }
}

impl ImportLanguage for SolidityLanguage {
    fn map_import(&self, imp: &Import) -> MappedDep {
        let Some(cfg) = &self.cfg else {
            return unmapped_dep(&imp.path);
        };
        match imp.kind {
            // Solidity has no stdlib
            ImportKind::Stdlib => skipped_dep(&imp.path),
            // Relative imports like ./Foo.sol or ../Bar.sol are files of the
            // same target, not deps
            ImportKind::Internal => skipped_dep(&imp.path),
            ImportKind::External => {
                let pkg = npm_package_name(&imp.path);
                if !cfg.external_deps.contains(&pkg) {
                    return unmapped_dep(&imp.path);
                }
                // A package's target is named after it at the cell root,
                // e.g. "forge-std" is "soldeps//:forge_std",
                // "@openzeppelin/contracts" "soldeps//:openzeppelin_contracts"
                MappedDep {
                    target: format!("{}//:{}", cfg.external_cell, package_target(&pkg)),
                    kind: DependencyType::External,
                    import_path: imp.path.clone(),
                }
            }
        }
    }
}

/// The project's Solidity configuration, with the language's cell and deps
/// file: none without a foundry.toml or a hardhat.config.js/ts at its root
fn detect_solidity_config(project_root: &str, lang: &config::Language) -> Option<SolidityConfig> {
    let found = ["foundry.toml", "hardhat.config.js", "hardhat.config.ts"]
        .iter()
        .any(|f| exists(&gostd::path::join(&[project_root, f])));
    if !found {
        return None;
    }
    Some(SolidityConfig {
        external_cell: lang.cell.clone(),
        external_deps: load_package_names(&gostd::path::join(&[project_root, &lang.deps_file]))
            .unwrap_or_default(),
    })
}

/// A Solidity package's target name: "@openzeppelin/contracts" is
/// "openzeppelin_contracts", "forge-std" "forge_std"
fn package_target(pkg: &str) -> String {
    pkg.strip_prefix('@')
        .unwrap_or(pkg)
        .replace(['/', '-'], "_")
}
