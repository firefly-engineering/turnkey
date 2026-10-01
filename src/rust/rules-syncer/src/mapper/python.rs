//! The Python plug-in: deps from the imports deps-extract finds in a
//! package's sources, with the markers and extras a uv workspace member
//! declares applied

use super::pymarkers::{PyMember, load_py_members};
use super::uv::{load_uv_workspace_modules, workspace_module_dir};
use super::{
    DependencyType, Dimensions, ImportLanguage, Language, MappedDep, PackageMapping, Request, Rule,
    TargetKind, exists, platform, read_toml, resolve_with_deps_extract, skipped_dep, unmapped_dep,
};
use anyhow::Result;
use deps_extract::extraction::{Import, ImportKind};
use project_sync::config;
use project_sync::launch::Launcher;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The project's Python configuration
#[derive(Debug, Clone, Default)]
pub struct PythonConfig {
    /// The packages the uv workspace's members provide, as dotted module
    /// names, keyed to the member's directory relative to the project
    /// root: {"turnkey.cfg": "src/python/cfg"}
    pub workspace_modules: HashMap<String, String>,
    /// The Buck2 cell of external deps, e.g. "pydeps"
    pub external_cell: String,
    /// The package names of python-deps.toml
    pub external_deps: HashSet<String>,
}

/// The Python rule kinds. A Python target builds its package with extras.
fn python_rule(rule: &str) -> Option<Rule> {
    let kind = match rule {
        "python_library" => TargetKind::Library,
        "python_binary" => TargetKind::Binary,
        "python_test" => TargetKind::Test,
        _ => return None,
    };
    Some(Rule {
        kind,
        deps_attribute: "deps",
        variant: &["extras"],
        canonical: None,
    })
}

/// Resolves a Python package's deps from the imports deps-extract finds in
/// its sources
pub struct PythonLanguage {
    pub(super) project_root: String,
    pub(super) cfg: Option<PythonConfig>,
    /// The uv workspace's members, with their declared deps
    pub(super) members: Vec<PyMember>,
    /// deps-extract's mapping of each package dir
    extracted: RefCell<HashMap<String, PackageMapping>>,
    /// The Python toolchain's version, once looked up
    pub(super) version: RefCell<Option<String>>,
    pub(super) launcher: Launcher,
}

impl PythonLanguage {
    pub fn new(mcfg: &super::Config, lang: &config::Language) -> Self {
        let project_root = mcfg.project_root.clone();
        let cfg = detect_python_config(&project_root, lang);
        let members = if cfg.is_some() {
            load_py_members(&project_root).unwrap_or_default()
        } else {
            Vec::new()
        };
        PythonLanguage {
            project_root,
            cfg,
            members,
            extracted: RefCell::new(HashMap::new()),
            version: RefCell::new(None),
            launcher: mcfg.launcher.clone(),
        }
    }

    /// Maps an external Python import to a Buck2 target.
    fn map_external(&self, cfg: &PythonConfig, module_path: &str) -> MappedDep {
        // A package of a uv workspace member maps to the member's target:
        // "turnkey.cargo.toml" -> "//src/python/cargo:cargo"
        if let Some(dir) = workspace_module_dir(&cfg.workspace_modules, module_path) {
            return MappedDep {
                target: format!("//{dir}:{}", gostd::path::base(dir)),
                kind: DependencyType::Internal,
                import_path: module_path.to_string(),
            };
        }
        // The top-level package, e.g. "requests" ->
        // "pydeps//vendor/requests:requests"
        let top = match module_path.find('.') {
            Some(i) if i > 0 => &module_path[..i],
            _ => module_path,
        };
        if !cfg.external_deps.contains(top) {
            return unmapped_dep(module_path);
        }
        MappedDep {
            target: format!("{}//vendor/{top}:{top}", cfg.external_cell),
            kind: DependencyType::External,
            import_path: module_path.to_string(),
        }
    }
}

impl Language for PythonLanguage {
    fn name(&self) -> &str {
        "python"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        python_rule(rule)
    }

    fn source_patterns(&self) -> &'static [&'static str] {
        &["*.py"]
    }

    /// A dependency's platform marker (sys_platform, ...) makes the deps
    /// depend on the platform.
    fn dimensions(&self, _: &str) -> Result<Dimensions> {
        Ok(platform())
    }

    /// Maps the imports deps-extract finds in `pkg_dir`; in a uv workspace
    /// member, then applies the markers and extras its pyproject.toml
    /// declares (`apply_markers`) for the configuration.
    fn resolve_deps(&self, pkg_dir: &str, req: &Request) -> Result<PackageMapping> {
        let cached = self.extracted.borrow().get(pkg_dir).cloned();
        let mapping = match cached {
            Some(mapping) => mapping,
            None => {
                let mapping = resolve_with_deps_extract(
                    self,
                    &self.launcher,
                    self.name(),
                    &self.project_root,
                    pkg_dir,
                )?;
                self.extracted
                    .borrow_mut()
                    .insert(pkg_dir.to_string(), mapping.clone());
                mapping
            }
        };
        let Ok(rel) = gostd::path::rel(&self.project_root, pkg_dir) else {
            return Ok(mapping);
        };
        match self.member_of(&rel) {
            Some(member) => Ok(self.apply_markers(mapping, member, req)),
            None => Ok(mapping),
        }
    }
}

impl ImportLanguage for PythonLanguage {
    fn map_import(&self, imp: &Import) -> MappedDep {
        let Some(cfg) = &self.cfg else {
            return unmapped_dep(&imp.path);
        };
        match imp.kind {
            ImportKind::Stdlib => skipped_dep(&imp.path),
            // Relative imports (.module, ..module) are modules of the same
            // target, and absolute internal imports can't be resolved
            // without more context: neither is a dep.
            ImportKind::Internal => skipped_dep(&imp.path),
            ImportKind::External => self.map_external(cfg, &imp.path),
        }
    }
}

/// The project's Python configuration, with the language's cell and deps
/// file: none without a pyproject.toml at its root
fn detect_python_config(project_root: &str, lang: &config::Language) -> Option<PythonConfig> {
    if !exists(&gostd::path::join(&[project_root, "pyproject.toml"])) {
        return None;
    }
    Some(PythonConfig {
        workspace_modules: load_uv_workspace_modules(project_root).unwrap_or_default(),
        external_cell: lang.cell.clone(),
        external_deps: load_deps_keys(&gostd::path::join(&[project_root, &lang.deps_file]))
            .unwrap_or_default(),
    })
}

/// A deps file's `[deps]` table, by key
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct DepsKeys {
    deps: BTreeMap<String, toml::Value>,
}

/// The keys of a deps file's `[deps]` table: the package names of
/// python-deps.toml
fn load_deps_keys(path: &str) -> Result<HashSet<String>> {
    let file: DepsKeys = read_toml(path)?;
    Ok(file.deps.into_keys().collect())
}
