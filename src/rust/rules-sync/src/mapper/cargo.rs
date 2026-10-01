//! A crate's dependencies read from its Cargo.toml: workspace inheritance,
//! features, and the target-specific tables that hold on a platform

use super::cargo_cfg::Spec;
use super::cargo_features::{self, Crate};
use super::rust::{CARGO_VARIANT_ATTRIBUTES, RustLanguage};
use super::variant::{Variant, read_variant};
use super::{
    DependencyType, MappedDep, PackageMapping, Request, UnsyncedDep, path_error, sort_deps,
};
use anyhow::{Result, anyhow, bail};
use conditions::{CPU, Configuration, OS};
use rules_star::Value;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use toml::Value as Toml;

/// A dependency table: each entry's key and value (a version string or a
/// table)
type DepTable = BTreeMap<String, Toml>;

/// The part of a Cargo.toml that sync reads
#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct CargoManifest {
    package: Option<PackageSection>,
    workspace: Option<WorkspaceSection>,
    features: BTreeMap<String, Vec<String>>,
    dependencies: DepTable,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: DepTable,
    #[serde(rename = "build-dependencies")]
    build_dependencies: DepTable,
    target: BTreeMap<String, TargetTables>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PackageSection {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct WorkspaceSection {
    members: Vec<String>,
    dependencies: DepTable,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TargetTables {
    dependencies: DepTable,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: DepTable,
    #[serde(rename = "build-dependencies")]
    build_dependencies: DepTable,
}

/// Parses the Cargo.toml in `dir`.
pub(super) fn read_cargo_manifest(dir: &str) -> Result<CargoManifest> {
    let path = gostd::path::join(&[dir, "Cargo.toml"]);
    let content =
        std::fs::read_to_string(&path).map_err(|err| anyhow!(path_error("open", &path, &err)))?;
    toml::from_str(&content).map_err(|err| anyhow!("parsing {path}: {}", toml_error(&err)))
}

/// A TOML error's message, on one line
fn toml_error(err: &toml::de::Error) -> String {
    err.message().to_string()
}

/// One dependency entry of a manifest, with a workspace = true entry
/// resolved against the workspace's `[workspace.dependencies]`
#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoDep {
    /// The entry's key in the manifest
    key: String,
    /// The Cargo package name: the entry's package key if it renames the
    /// dependency, else the entry's key
    package: String,
    /// The directory of a path dependency, or ""
    path: String,
    /// optional = true
    optional: bool,
    /// The features the entry asks for: its own and, for a workspace =
    /// true entry, the workspace entry's
    features: Vec<String>,
    /// False for default-features = false: the dependency's default
    /// features aren't asked for
    default_features: bool,
}

/// Reads one dependency entry, whose value is a version string or a
/// table. `base_dir` resolves a relative path.
fn parse_cargo_dep(key: &str, value: &Toml, base_dir: &str) -> CargoDep {
    let mut dep = CargoDep {
        key: key.to_string(),
        package: key.to_string(),
        path: String::new(),
        optional: false,
        features: Vec::new(),
        default_features: true,
    };
    let Some(table) = value.as_table() else {
        return dep;
    };
    if let Some(pkg) = table.get("package").and_then(Toml::as_str)
        && !pkg.is_empty()
    {
        dep.package = pkg.to_string();
    }
    if let Some(path) = table.get("path").and_then(Toml::as_str)
        && !path.is_empty()
    {
        dep.path = gostd::path::join(&[base_dir, path]);
    }
    if let Some(optional) = table.get("optional").and_then(Toml::as_bool) {
        dep.optional = optional;
    }
    for key in ["default-features", "default_features"] {
        if let Some(defaults) = table.get(key).and_then(Toml::as_bool) {
            dep.default_features = defaults;
        }
    }
    if let Some(features) = table.get("features").and_then(Toml::as_array) {
        dep.features = features
            .iter()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect();
    }
    dep
}

/// Whether a dependency entry is workspace = true
fn is_workspace_dep(value: &Toml) -> bool {
    value
        .as_table()
        .and_then(|t| t.get("workspace"))
        .and_then(Toml::as_bool)
        .unwrap_or(false)
}

/// A Cargo workspace root: its `[workspace.dependencies]`, which workspace
/// = true entries inherit from
#[derive(Debug, Clone, Default)]
pub(super) struct CargoWorkspace {
    dir: String,
    deps: DepTable,
}

/// Reads a dependency table, resolving workspace = true entries against
/// `ws`, sorted by key
fn resolve_cargo_deps(
    table: &DepTable,
    crate_dir: &str,
    ws: Option<&CargoWorkspace>,
) -> Result<Vec<CargoDep>> {
    let mut deps = Vec::with_capacity(table.len());
    for (key, value) in table {
        if !is_workspace_dep(value) {
            deps.push(parse_cargo_dep(key, value, crate_dir));
            continue;
        }
        let Some(ws) = ws else {
            bail!("dependency {key}: workspace = true, but no workspace root found");
        };
        let Some(inherited) = ws.deps.get(key) else {
            bail!("dependency {key}: workspace = true, but [workspace.dependencies] has no {key}");
        };
        let mut dep = parse_cargo_dep(key, inherited, &ws.dir);
        // optional is set on the member's entry, never the workspace's;
        // features add up, and default-features is the workspace entry's
        let own = parse_cargo_dep(key, value, crate_dir);
        dep.optional = own.optional;
        for f in own.features {
            if !dep.features.contains(&f) {
                dep.features.push(f);
            }
        }
        deps.push(dep);
    }
    Ok(deps)
}

/// Reads the workspace root's manifest in `dir`: its
/// `[workspace.dependencies]`, and each member's package name, keyed to the
/// member's directory relative to `project_root`.
pub(super) fn load_cargo_workspace(
    dir: &str,
    project_root: &str,
) -> Result<(CargoWorkspace, HashMap<String, String>)> {
    let manifest = read_cargo_manifest(dir)?;
    let Some(workspace) = manifest.workspace else {
        bail!(
            "{} has no [workspace]",
            gostd::path::join(&[dir, "Cargo.toml"])
        );
    };
    let ws = CargoWorkspace {
        dir: dir.to_string(),
        deps: workspace.dependencies,
    };
    let mut members = HashMap::new();
    for pattern in &workspace.members {
        let dirs = super::glob(&gostd::path::join(&[dir, pattern]))
            .map_err(|err| anyhow!("workspace member {}: {err}", gostd::strconv::quote(pattern)))?;
        for member_dir in dirs {
            let Ok(member) = read_cargo_manifest(&member_dir) else {
                continue;
            };
            let Some(package) = member.package else {
                continue;
            };
            let Ok(rel) = gostd::path::rel(project_root, &member_dir) else {
                continue;
            };
            members.insert(package.name, rel);
        }
    }
    Ok((ws, members))
}

/// The features a target's variant asks for: its cargo_features, and
/// "default" unless default_features = False
fn feature_request(variant: &Variant) -> Vec<String> {
    let mut request = rules_star::labels(variant.get("cargo_features")).unwrap_or_default();
    match variant.get("default_features") {
        Some(Value::Bool(false)) => {}
        _ => request.push("default".to_string()),
    }
    request
}

/// What feature activation reads from a manifest: its `[features]`, and
/// which dependency keys are optional or required, in `[dependencies]` and
/// every `[target.*.dependencies]`
fn feature_crate(manifest: &CargoManifest) -> Crate {
    let mut krate = Crate {
        features: manifest.features.clone(),
        ..Crate::default()
    };
    let tables = std::iter::once(&manifest.dependencies)
        .chain(manifest.target.values().map(|t| &t.dependencies));
    for table in tables {
        for (key, value) in table {
            let optional = value
                .as_table()
                .and_then(|t| t.get("optional"))
                .is_some_and(|o| o.as_bool() == Some(true));
            if optional {
                krate.optional.insert(key.clone());
            } else {
                krate.required.insert(key.clone());
            }
        }
    }
    krate
}

/// The Rust target triple of a configuration's platform, in Buck2's
/// names; `None` if the configuration has no platform, or one turnkey
/// doesn't build Rust for
fn rust_triple(config: &Configuration) -> Option<&'static str> {
    Some(match (config.get(OS), config.get(CPU)) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "arm64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "arm64") => "aarch64-apple-darwin",
        _ => return None,
    })
}

impl RustLanguage {
    /// A Rust crate's deps and features from its Cargo.toml, for req's
    /// variant in req's configuration.
    ///
    /// The variant is a Cargo-style feature request (cargo_features, and
    /// the crate's defaults unless default_features = False): with neither,
    /// what `cargo build -p <crate>` builds. The request is expanded into
    /// the crate's features (attrs["features"]) and the optional
    /// dependencies they activate. `[dependencies]` then become deps and
    /// `[dev-dependencies]` test deps, and so do the
    /// `[target.'<spec>'.*]` tables whose spec (a cfg() expression or a
    /// target triple) holds on the configuration's platform; an optional
    /// dependency only when activated.
    ///
    /// Workspace members map to their Buck2 target (see `map_cargo_dep`),
    /// other crates to the external cell. A crate neither, or a member no
    /// target of which builds the features asked for, is reported unmapped.
    /// Build dependencies are reported unsynced, and so are target-specific
    /// ones when there's no platform to evaluate them for.
    pub(super) fn resolve_crate(&self, crate_dir: &str, req: &Request) -> Result<PackageMapping> {
        let rel = gostd::path::rel(&self.project_root, crate_dir).unwrap_or(crate_dir.to_string());
        let mut mapping = PackageMapping {
            path: rel,
            ..PackageMapping::default()
        };
        let Some(cfg) = &self.cfg else {
            bail!("no Rust configuration: the project has no Cargo.toml");
        };
        let manifest = read_cargo_manifest(crate_dir)?;

        let activation = cargo_features::activate(
            &feature_crate(&manifest),
            &feature_request(&req.variant),
            &[],
        );
        mapping
            .attrs
            .insert("features".to_string(), activation.features.clone());

        // A crate with its own [workspace] is its own root; any other
        // inherits from the project's.
        let own_ws = manifest.workspace.as_ref().map(|w| CargoWorkspace {
            dir: crate_dir.to_string(),
            deps: w.dependencies.clone(),
        });
        let ws = own_ws.as_ref().or(cfg.workspace.as_ref());
        let resolve = |table: &DepTable| resolve_cargo_deps(table, crate_dir, ws);

        // Sync doesn't manage these, but reports each so none is dropped
        // silently.
        let unsynced =
            |mapping: &mut PackageMapping, table: &DepTable, reason: &str| -> Result<()> {
                for dep in resolve(table)? {
                    let mapped = self
                        .map_cargo_dep(&dep, &[], req)
                        .unwrap_or_else(|_| MappedDep {
                            kind: DependencyType::Unmapped,
                            import_path: dep.package.clone(),
                            ..MappedDep::default()
                        });
                    mapping.unsynced_deps.push(UnsyncedDep {
                        dep: mapped,
                        reason: reason.to_string(),
                    });
                }
                Ok(())
            };

        // The tables that apply: the unconditional ones, and each
        // target-specific one whose spec holds
        let mut applicable: Vec<(&DepTable, &DepTable, &DepTable)> = vec![(
            &manifest.dependencies,
            &manifest.dev_dependencies,
            &manifest.build_dependencies,
        )];
        // Target-specific tables there's no platform to evaluate for
        let mut unevaluated: Vec<(&DepTable, String)> = Vec::new();
        let triple = rust_triple(&req.config);
        for (spec, t) in &manifest.target {
            let mut reason = format!("target-specific ({spec})");
            match Spec::parse(spec) {
                Err(err) => reason = format!("target-specific ({spec}: {err})"),
                Ok(parsed) => {
                    if let Some(triple) = triple {
                        if parsed.matches(triple) {
                            applicable.push((
                                &t.dependencies,
                                &t.dev_dependencies,
                                &t.build_dependencies,
                            ));
                        }
                        continue;
                    }
                }
            }
            for table in [&t.dependencies, &t.dev_dependencies, &t.build_dependencies] {
                unevaluated.push((table, reason.clone()));
            }
        }

        for (deps, dev_deps, build_deps) in applicable {
            for dep in resolve(deps)? {
                if dep.optional && !activation.optional_deps.contains(&dep.key) {
                    // No enabled feature activates it
                    continue;
                }
                let forwarded = activation
                    .dep_features
                    .get(&dep.key)
                    .cloned()
                    .unwrap_or_default();
                match self.map_cargo_dep(&dep, &forwarded, req) {
                    Ok(mapped) => mapping.deps.push(mapped),
                    Err(unmapped) => mapping.unmapped_imports.push(unmapped),
                }
            }
            for dep in resolve(dev_deps)? {
                let forwarded = activation
                    .dep_features
                    .get(&dep.key)
                    .cloned()
                    .unwrap_or_default();
                match self.map_cargo_dep(&dep, &forwarded, req) {
                    Ok(mapped) => mapping.test_deps.push(mapped),
                    Err(unmapped) => mapping.unmapped_test_imports.push(unmapped),
                }
            }
            unsynced(&mut mapping, build_deps, "build dependency")?;
        }
        for (table, reason) in unevaluated {
            unsynced(&mut mapping, table, &reason)?;
        }

        sort_deps(&mut mapping.deps);
        sort_deps(&mut mapping.test_deps);
        Ok(mapping)
    }

    /// A resolved dependency's Buck2 target: a workspace member's (see
    /// `member_target`), or the external cell's for the package.
    /// `forwarded` are features the depending crate's own features ask for
    /// on it (x/feat). It is an error, describing the dependency for the
    /// report, if the dependency is neither.
    fn map_cargo_dep(
        &self,
        dep: &CargoDep,
        forwarded: &[String],
        req: &Request,
    ) -> Result<MappedDep, String> {
        let cfg = self.cfg.as_ref().expect("a Rust configuration");
        let mut member_dir = cfg.workspace_packages.get(&dep.package).cloned();
        if !dep.path.is_empty() {
            match gostd::path::rel(&self.project_root, &dep.path) {
                Ok(rel) if rel != ".." && !rel.starts_with("../") => member_dir = Some(rel),
                _ => return Err(dep.package.clone()),
            }
        }
        if let Some(member_dir) = member_dir {
            let mut features = dep.features.clone();
            for f in forwarded {
                if !features.contains(f) {
                    features.push(f.clone());
                }
            }
            let mut name = gostd::path::base(&member_dir);
            if !features.is_empty() || !dep.default_features {
                name = self
                    .member_target(&member_dir, &features, dep.default_features, req)
                    .map_err(|why| format!("{} ({why})", dep.package))?;
            }
            // e.g. "src/rust/nix-eval" -> "//src/rust/nix-eval:nix-eval"
            return Ok(MappedDep {
                target: format!("//{member_dir}:{name}"),
                kind: DependencyType::Internal,
                import_path: dep.package.clone(),
            });
        }
        if !cfg.external_deps.contains(&dep.package) {
            return Err(dep.package.clone());
        }
        // e.g. "tree-sitter" -> "rustdeps//vendor/tree-sitter:tree-sitter"
        Ok(MappedDep {
            target: format!(
                "{}//vendor/{}:{}",
                cfg.external_cell, dep.package, dep.package
            ),
            kind: DependencyType::External,
            import_path: dep.package.clone(),
        })
    }

    /// The name of the rust_library of the member in `member_dir` that
    /// builds what a dependency asking for `features` (and its defaults,
    /// with `defaults`) gets: the target whose request, expanded in req's
    /// configuration, enables exactly the same features. If no target, or
    /// several, match, it is an error saying why.
    fn member_target(
        &self,
        member_dir: &str,
        features: &[String],
        defaults: bool,
        req: &Request,
    ) -> Result<String, String> {
        let manifest = read_cargo_manifest(&gostd::path::join(&[&self.project_root, member_dir]))
            .map_err(|err| format!("reading its Cargo.toml: {err:#}"))?;
        let krate = feature_crate(&manifest);
        let mut request = features.to_vec();
        if defaults {
            request.push("default".to_string());
        }
        let want = cargo_features::activate(&krate, &request, &[]).features;
        let needs = if want.is_empty() {
            "no features".to_string()
        } else {
            format!("features {}", want.join(", "))
        };

        let matches = self
            .member_rules(member_dir, |rules| {
                rules
                    .targets
                    .iter()
                    .filter(|t| t.rule == "rust_library")
                    .filter(|t| {
                        let Ok(variant) = read_variant(t, CARGO_VARIANT_ATTRIBUTES, &self.space)
                        else {
                            return false;
                        };
                        let have = cargo_features::activate(
                            &krate,
                            &feature_request(&variant.get(&req.config)),
                            &[],
                        )
                        .features;
                        have == want
                    })
                    .map(|t| t.name.clone())
                    .collect::<Vec<_>>()
            })
            .map_err(|err| format!("needs {needs}: {err}"))?;
        match matches.len() {
            1 => Ok(matches[0].clone()),
            0 => Err(format!(
                "needs {needs}: no rust_library of //{member_dir} builds exactly them"
            )),
            _ => Err(format!(
                "needs {needs}: several rust_library targets of //{member_dir} build them: {}",
                matches.join(", ")
            )),
        }
    }
}
