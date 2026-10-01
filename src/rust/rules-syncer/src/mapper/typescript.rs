//! The TypeScript and JavaScript plug-in: deps from the imports
//! deps-extract finds in a package's sources

use super::{
    DependencyType, Dimensions, ImportLanguage, Language, MappedDep, PackageMapping, Request, Rule,
    TargetKind, exists, read_toml, resolve_with_deps_extract, skipped_dep, sort_deps, unmapped_dep,
};
use anyhow::Result;
use deps_extract::extraction::{Import, ImportKind};
use project_sync::config;
use project_sync::launch::Launcher;
use serde::Deserialize;
use std::collections::HashSet;

/// The project's TypeScript configuration
#[derive(Debug, Clone, Default)]
pub struct TypeScriptConfig {
    /// The Buck2 cell of external deps, e.g. "jsdeps"
    pub external_cell: String,
    /// The names of the npm packages in js-deps.toml
    pub external_deps: HashSet<String>,
}

/// The TypeScript and JavaScript rule kinds. Their deps go in npm_deps: the
/// rules take the jsdeps cell's packages there, and other TypeScript
/// targets in deps, which sync doesn't resolve (it skips relative imports).
fn typescript_rule(rule: &str) -> Option<Rule> {
    let kind = match rule {
        "typescript_library" | "js_library" => TargetKind::Library,
        "typescript_binary" | "js_binary" => TargetKind::Binary,
        "typescript_test" | "js_test" => TargetKind::Test,
        _ => return None,
    };
    Some(Rule {
        kind,
        deps_attribute: "npm_deps",
        ..Rule::default()
    })
}

/// Resolves a TypeScript or JavaScript package's deps from the imports
/// deps-extract finds in its sources
pub struct TypeScriptLanguage {
    project_root: String,
    cfg: Option<TypeScriptConfig>,
    launcher: Launcher,
}

impl TypeScriptLanguage {
    pub fn new(mcfg: &super::Config, lang: &config::Language) -> Self {
        TypeScriptLanguage {
            project_root: mcfg.project_root.clone(),
            cfg: detect_typescript_config(&mcfg.project_root, lang),
            launcher: mcfg.launcher.clone(),
        }
    }

    /// `deps` plus the @types package of each external one that js-deps.toml
    /// has, deduplicated and sorted
    pub(super) fn with_types(&self, deps: Vec<MappedDep>) -> Vec<MappedDep> {
        let Some(cfg) = &self.cfg else {
            return deps;
        };
        let mut seen: HashSet<String> = deps.iter().map(|d| d.target.clone()).collect();
        let mut result = deps.clone();
        for dep in &deps {
            if dep.kind != DependencyType::External {
                continue;
            }
            let types = types_package(&npm_package_name(&dep.import_path));
            if !cfg.external_deps.contains(&types) {
                continue;
            }
            let types_dep = MappedDep {
                target: label(cfg, &types),
                kind: DependencyType::External,
                import_path: types,
            };
            if seen.insert(types_dep.target.clone()) {
                result.push(types_dep);
            }
        }
        sort_deps(&mut result);
        result
    }
}

impl Language for TypeScriptLanguage {
    fn name(&self) -> &str {
        "typescript"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        typescript_rule(rule)
    }

    fn source_patterns(&self) -> &'static [&'static str] {
        &["*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs"]
    }

    fn dimensions(&self, _: &str) -> Result<Dimensions> {
        Ok(Dimensions::default())
    }

    /// Maps the package's imports, and adds for each npm package its
    /// DefinitelyTyped package (@types/...) when js-deps.toml has one: code
    /// never imports those, but TypeScript needs them to type-check the
    /// import.
    fn resolve_deps(&self, pkg_dir: &str, _: &Request) -> Result<PackageMapping> {
        let mut mapping = resolve_with_deps_extract(
            self,
            &self.launcher,
            self.name(),
            &self.project_root,
            pkg_dir,
        )?;
        mapping.deps = self.with_types(mapping.deps);
        mapping.test_deps = self.with_types(mapping.test_deps);
        Ok(mapping)
    }
}

impl ImportLanguage for TypeScriptLanguage {
    fn map_import(&self, imp: &Import) -> MappedDep {
        let Some(cfg) = &self.cfg else {
            return unmapped_dep(&imp.path);
        };
        match imp.kind {
            // Node.js builtins
            ImportKind::Stdlib => skipped_dep(&imp.path),
            // Relative imports like ./foo or ../bar are files of the same
            // target, not deps
            ImportKind::Internal => skipped_dep(&imp.path),
            ImportKind::External => {
                let pkg = npm_package_name(&imp.path);
                if !cfg.external_deps.contains(&pkg) {
                    return unmapped_dep(&imp.path);
                }
                MappedDep {
                    target: label(cfg, &pkg),
                    kind: DependencyType::External,
                    import_path: imp.path.clone(),
                }
            }
        }
    }
}

/// The DefinitelyTyped package of an npm package: "lodash" is
/// "@types/lodash", "@org/pkg" "@types/org__pkg". A package under @types is
/// its own types.
pub(super) fn types_package(pkg: &str) -> String {
    if pkg.starts_with("@types/") {
        return pkg.to_string();
    }
    match pkg.strip_prefix('@') {
        Some(scoped) => format!("@types/{}", scoped.replacen('/', "__", 1)),
        None => format!("@types/{pkg}"),
    }
}

/// The project's TypeScript configuration, with the language's cell and
/// deps file: none without a package.json or a tsconfig.json at its root
fn detect_typescript_config(
    project_root: &str,
    lang: &config::Language,
) -> Option<TypeScriptConfig> {
    if !exists(&gostd::path::join(&[project_root, "package.json"]))
        && !exists(&gostd::path::join(&[project_root, "tsconfig.json"]))
    {
        return None;
    }
    Some(TypeScriptConfig {
        external_cell: lang.cell.clone(),
        external_deps: load_package_names(&gostd::path::join(&[project_root, &lang.deps_file]))
            .unwrap_or_default(),
    })
}

/// A deps file of `[[package]]` tables, as jsdeps-gen and soldeps-gen write
/// them
#[derive(Deserialize, Default)]
#[serde(default)]
struct PackagesFile {
    package: Vec<NamedPackage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NamedPackage {
    name: String,
}

/// The package names of a deps file of `[[package]]` tables
pub(super) fn load_package_names(path: &str) -> Result<HashSet<String>> {
    let file: PackagesFile = read_toml(path)?;
    Ok(file.package.into_iter().map(|p| p.name).collect())
}

/// The jsdeps cell's target for an npm package: the alias at the cell root,
/// named with "@" dropped and "/" replaced by "_" (as
/// nix/lib/deps-cell/adapters/javascript.nix names it), e.g. "lodash" is
/// "jsdeps//:lodash", "@types/node" "jsdeps//:types_node"
fn label(cfg: &TypeScriptConfig, pkg: &str) -> String {
    let name = pkg.replace('@', "").replace('/', "_");
    format!("{}//:{name}", cfg.external_cell)
}

/// The package an import path names, scoped packages included:
/// "@org/pkg/subpath" is "@org/pkg", "pkg/subpath" "pkg"
pub(super) fn npm_package_name(import_path: &str) -> String {
    if import_path.starts_with('@') {
        let parts: Vec<&str> = import_path.splitn(3, '/').collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[0], parts[1]);
        }
        return import_path.to_string();
    }
    import_path
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string()
}
