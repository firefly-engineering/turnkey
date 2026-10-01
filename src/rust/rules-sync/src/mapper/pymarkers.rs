//! A uv workspace member's declared dependencies: their markers and its
//! extras applied to what its imports map to

use super::python::PythonLanguage;
use super::{MappedDep, PackageMapping, Request, read_toml, sort_deps};
use anyhow::{Result, anyhow};
use conditions::Configuration;
use pep508::{Marker, Requirement};
use rules_star::Value;
use serde::Deserialize;
use std::collections::BTreeMap;

/// A uv workspace member: where it is, and the dependencies its
/// pyproject.toml declares
#[derive(Debug, Clone)]
pub struct PyMember {
    /// The member's directory, relative to the project root
    pub dir: String,
    /// Its distribution name, normalized
    pub name: String,
    /// Its `[project]` dependencies, by normalized name
    pub requires: BTreeMap<String, Requirement>,
    /// Its `[project.optional-dependencies]`, by normalized extra
    pub extras: BTreeMap<String, Vec<Requirement>>,
}

/// The root pyproject.toml, as far as the workspace is concerned
#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct RootPyproject {
    pub tool: Tool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct Tool {
    pub uv: Uv,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct Uv {
    pub workspace: Workspace,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct Workspace {
    pub members: Vec<String>,
}

/// A member's pyproject.toml, as far as its dependencies are concerned
#[derive(Deserialize, Default)]
#[serde(default)]
struct MemberPyproject {
    project: Project,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Project {
    name: String,
    dependencies: Vec<String>,
    #[serde(rename = "optional-dependencies")]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

/// Reads the uv workspace members' pyproject.toml.
pub fn load_py_members(project_root: &str) -> Result<Vec<PyMember>> {
    let root: RootPyproject = read_toml(&gostd::path::join(&[project_root, "pyproject.toml"]))?;
    let mut members = Vec::new();
    for pattern in &root.tool.uv.workspace.members {
        let dirs = super::glob(&gostd::path::join(&[project_root, pattern]))
            .map_err(|err| anyhow!("workspace member {}: {err}", gostd::strconv::quote(pattern)))?;
        for dir in dirs {
            if let Some(member) = load_py_member(project_root, &dir)? {
                members.push(member);
            }
        }
    }
    Ok(members)
}

/// Reads one member's pyproject.toml, or `None` if the directory has none.
fn load_py_member(project_root: &str, dir: &str) -> Result<Option<PyMember>> {
    let path = gostd::path::join(&[dir, "pyproject.toml"]);
    if std::fs::metadata(&path).is_err() {
        return Ok(None);
    }
    let pyproject: MemberPyproject = read_toml(&path)?;
    let rel = gostd::path::rel(project_root, dir).map_err(|err| anyhow!(err))?;
    let mut member = PyMember {
        dir: rel,
        name: pep508::normalize_name(&pyproject.project.name),
        requires: BTreeMap::new(),
        extras: BTreeMap::new(),
    };
    for spec in &pyproject.project.dependencies {
        let req = pep508::parse_requirement(spec).map_err(|err| anyhow!("{path}: {err}"))?;
        member.requires.insert(req.name.clone(), req);
    }
    for (extra, specs) in &pyproject.project.optional_dependencies {
        let extra = pep508::normalize_name(extra);
        for spec in specs {
            let req = pep508::parse_requirement(spec).map_err(|err| anyhow!("{path}: {err}"))?;
            member.extras.entry(extra.clone()).or_default().push(req);
        }
    }
    Ok(Some(member))
}

/// A member's target: the package at its directory, named after it
fn member_target(m: &PyMember) -> String {
    format!("//{}:{}", m.dir, gostd::path::base(&m.dir))
}

impl PythonLanguage {
    /// The member whose directory holds `rel` (relative to the project
    /// root), the innermost one
    pub(super) fn member_of(&self, rel: &str) -> Option<&PyMember> {
        let mut best: Option<&PyMember> = None;
        for m in &self.members {
            let holds = rel == m.dir
                || rel
                    .strip_prefix(&m.dir)
                    .is_some_and(|rest| rest.starts_with('/'));
            if holds && best.is_none_or(|b| m.dir.len() > b.dir.len()) {
                best = Some(m);
            }
        }
        best
    }

    /// The member whose distribution is `name`
    fn member_named(&self, name: &str) -> Option<&PyMember> {
        self.members.iter().find(|m| m.name == name)
    }

    /// The distribution a dep's target builds, normalized: a member's name,
    /// or a pydeps package's (whose key uses _ for -)
    fn dist_key(&self, target: &str) -> String {
        if let Some(m) = self.members.iter().find(|m| target == member_target(m)) {
            return m.name.clone();
        }
        let cell = self.cfg.as_ref().map_or("", |c| c.external_cell.as_str());
        if let Some(pkg) = target.strip_prefix(&format!("{cell}//vendor/")) {
            let key = pkg.split_once(':').map_or(pkg, |p| p.0);
            return pep508::normalize_name(key);
        }
        String::new()
    }

    /// A requirement's target: a member's, or the pydeps cell's for a
    /// vendored package
    fn requirement_target(&self, req: &Requirement) -> Option<MappedDep> {
        if let Some(m) = self.member_named(&req.name) {
            return Some(MappedDep {
                target: member_target(m),
                kind: super::DependencyType::Internal,
                import_path: req.name.clone(),
            });
        }
        let cfg = self.cfg.as_ref()?;
        let key = req.name.replace('-', "_");
        if !cfg.external_deps.contains(&key) {
            return None;
        }
        Some(MappedDep {
            target: format!("{}//vendor/{key}:{key}", cfg.external_cell),
            kind: super::DependencyType::External,
            import_path: req.name.clone(),
        })
    }

    /// The Python toolchain's full version, from the python3 on the PATH
    /// (the toolchain's, in a turnkey shell), or "" if there is none
    fn python_version(&self) -> String {
        if let Some(version) = self.version.borrow().as_ref() {
            return version.clone();
        }
        let version = self
            .launcher
            .command(
                std::ffi::OsStr::new("python3"),
                &["-c", "import platform; print(platform.python_version())"],
                None,
            )
            .ok()
            .and_then(|mut cmd| cmd.stderr(std::process::Stdio::piped()).output().ok())
            .filter(|out| out.status.success())
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .trim_matches(gostd::unicode::is_space)
                    .to_string()
            })
            .unwrap_or_default();
        *self.version.borrow_mut() = Some(version.clone());
        version
    }

    /// Whether a requirement's marker holds in `config` (a platform, or
    /// none) for the Python toolchain, with `extra`. A marker whose
    /// variables aren't known (a platform one without a platform, a Python
    /// one without python3) is taken to hold: sync can't tell, so it keeps
    /// the dep.
    fn marker_holds(&self, marker: Option<&Marker>, config: &Configuration, extra: &str) -> bool {
        let version = self.python_version();
        // For a platform it doesn't know, the environment without one
        let env = pep508::env_for(config, &version, extra)
            .or_else(|| pep508::env_for(&Configuration::default(), &version, extra))
            .expect("the environment of no platform");
        !env.decides(marker) || pep508::holds(marker, &env)
    }

    /// Keeps the deps of a member's package that its pyproject.toml declares
    /// where their markers hold, and adds the dependencies of the target's
    /// extras:
    /// - a dependency in `[project] dependencies` is kept where its marker
    ///   holds
    /// - one only in `[project.optional-dependencies]` is kept if one of the
    ///   target's extras declares it, where that marker holds
    /// - one the member doesn't declare (an import sync maps on its own) is
    ///   kept
    ///
    /// Each extra's dependencies are added where their markers hold,
    /// whether the sources import them or not.
    pub(super) fn apply_markers(
        &self,
        mut m: PackageMapping,
        member: &PyMember,
        req: &Request,
    ) -> PackageMapping {
        let extras = target_extras(req.variant.get("extras"));
        let keep = |deps: Vec<MappedDep>| -> Vec<MappedDep> {
            deps.into_iter()
                .filter(|dep| {
                    let name = self.dist_key(&dep.target);
                    if let Some(r) = member.requires.get(&name) {
                        return self.marker_holds(r.marker.as_ref(), &req.config, "");
                    }
                    let (mut declared, mut enabled) = (false, false);
                    for (extra, reqs) in &member.extras {
                        for r in reqs.iter().filter(|r| r.name == name) {
                            declared = true;
                            if extras.contains(extra)
                                && self.marker_holds(r.marker.as_ref(), &req.config, extra)
                            {
                                enabled = true;
                            }
                        }
                    }
                    !declared || enabled
                })
                .collect()
        };
        m.deps = keep(m.deps);
        m.test_deps = keep(m.test_deps);

        for extra in &extras {
            for r in member.extras.get(extra).into_iter().flatten() {
                if !self.marker_holds(r.marker.as_ref(), &req.config, extra) {
                    continue;
                }
                let Some(dep) = self.requirement_target(r) else {
                    m.unmapped_imports
                        .push(format!("{} (extra {extra})", r.name));
                    continue;
                };
                if dep.target == member_target(member)
                    || m.deps.iter().any(|d| d.target == dep.target)
                {
                    continue;
                }
                m.deps.push(dep);
            }
        }
        sort_deps(&mut m.deps);
        m
    }
}

/// The extras a target's variant builds with, normalized and sorted
fn target_extras(extras: Option<&Value>) -> Vec<String> {
    let mut normalized: Vec<String> = rules_star::labels(extras)
        .unwrap_or_default()
        .iter()
        .map(|e| pep508::normalize_name(e))
        .collect();
    normalized.sort();
    normalized
}
