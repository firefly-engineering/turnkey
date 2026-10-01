//! A package synced through its language, and what each of its targets
//! wants

use super::variant::{VariantReading, read_variant};
use super::{Language, MappedDep, PackageMapping, Request, Rule, TargetKind, deps_to_targets};
use anyhow::Result;
use conditions::{Configuration, Space};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// One package being synced through its language: the configurations its
/// deps are resolved for, and what each of its targets wants in them. It
/// resolves each distinct request once, and collects what the resolutions
/// report.
pub struct Package<'a> {
    lang: &'a dyn Language,
    dir: String,
    dims: Vec<String>,
    buck_package: String,
    space: Space,
    cache: RefCell<HashMap<String, PackageMapping>>,
    /// The reports of every resolution, each once, in order
    messages: RefCell<Vec<String>>,
    reported: RefCell<HashSet<String>>,
}

impl<'a> Package<'a> {
    /// The package in `dir`, synced through `lang` in every configuration
    /// of `space` crossed with the on/off dimensions its deps depend on
    /// (e.g. Go build tags). `project_root` locates the package in the
    /// Buck2 project.
    pub fn open(
        lang: &'a dyn Language,
        project_root: &str,
        dir: &str,
        space: &Space,
    ) -> Result<Package<'a>> {
        let dims = lang.dimensions(dir)?;
        Ok(Package {
            lang,
            dir: dir.to_string(),
            dims: dims.names(),
            buck_package: buck_package(dir, project_root),
            space: space.with_dimensions(&dims.on_off),
            cache: RefCell::new(HashMap::new()),
            messages: RefCell::new(Vec::new()),
            reported: RefCell::new(HashSet::new()),
        })
    }

    /// The configurations the package's deps are resolved for
    pub fn space(&self) -> &Space {
        &self.space
    }

    /// What the resolutions so far reported (imports that couldn't be
    /// mapped, deps sync doesn't own), each once, in order
    pub fn messages(&self) -> Vec<String> {
        self.messages.borrow().clone()
    }

    /// Resolves the package in every configuration, as a library's deps,
    /// so that what can't be mapped is reported even if no target is
    /// synced.
    pub fn resolve_all(&self) -> Result<()> {
        for config in &self.space.configurations {
            self.resolve(config, TargetKind::Library, &Default::default())?;
        }
        Ok(())
    }

    /// What `target`, of `rule`, wants. It is an error, naming the
    /// attribute, if one of its variant attributes can't be read.
    pub fn target(
        &self,
        target: &rules_star::Target,
        rule: Rule,
    ) -> std::result::Result<Target<'_, 'a>, String> {
        let variant = read_variant(target, rule.variant, &self.space)?;
        Ok(Target {
            pkg: self,
            rule,
            variant,
            under_test: !target.get_string_attr("target_under_test").is_empty(),
        })
    }

    /// The package's deps in `config`, for a target of `kind` and its
    /// variant. Deps on the package's own targets are dropped (e.g. when
    /// syncing src/python/cargo, //src/python/cargo:cargo): its targets
    /// depend on each other as ":name", which sync keeps.
    fn resolve(
        &self,
        config: &Configuration,
        kind: TargetKind,
        variant: &super::Variant,
    ) -> Result<PackageMapping> {
        let req = Request {
            config: config.project(&self.dims),
            kind,
            variant: variant.clone(),
        };
        let key = format!("{}|{}|{}", req.config, kind as i32, variant_key(variant));
        if let Some(m) = self.cache.borrow().get(&key) {
            return Ok(m.clone());
        }
        let mut m = self.lang.resolve_deps(&self.dir, &req)?;
        m.deps = self.without_own_targets(m.deps);
        m.test_deps = self.without_own_targets(m.test_deps);
        self.cache.borrow_mut().insert(key, m.clone());

        for unmapped in &m.unmapped_imports {
            self.report(format!("unmapped import: {unmapped}"));
        }
        for unmapped in &m.unmapped_test_imports {
            self.report(format!("unmapped test import: {unmapped}"));
        }
        for u in &m.unsynced_deps {
            self.report(format!(
                "{} dependency {} not synced",
                u.reason, u.dep.import_path
            ));
        }
        Ok(m)
    }

    /// Records a message unless it already was.
    fn report(&self, msg: String) {
        if self.reported.borrow_mut().insert(msg.clone()) {
            self.messages.borrow_mut().push(msg);
        }
    }

    /// `deps` without those on a target of the package
    fn without_own_targets(&self, deps: Vec<MappedDep>) -> Vec<MappedDep> {
        if self.buck_package.is_empty() {
            return deps;
        }
        deps.into_iter()
            .filter(|dep| {
                let pkg = dep.target.split_once(':').map_or(&*dep.target, |p| p.0);
                pkg != self.buck_package
            })
            .collect()
    }
}

/// What a target wants in its deps in one configuration
#[derive(Debug, Clone, Default)]
pub struct Want {
    /// The deps its sources or manifest need
    pub labels: Vec<String>,
    /// The imports that couldn't be mapped: with any, the labels are
    /// incomplete, so no existing dep should be removed
    pub unmapped: Vec<String>,
    /// The targets of deps sync doesn't own: an existing dep in the Buck2
    /// package of one should never be removed
    pub unsynced: Vec<String>,
    /// The rule's [`Rule::canonical`]
    pub canonical: Option<fn(&str) -> String>,
}

impl Want {
    /// The label an existing dep stands for: the label itself, unless the
    /// rule says otherwise ([`Rule::canonical`])
    pub fn stands_for(&self, label: &str) -> String {
        match self.canonical {
            Some(canonical) => canonical(label),
            None => label.to_string(),
        }
    }
}

/// What sync wants for one target of a package
pub struct Target<'p, 'a> {
    pkg: &'p Package<'a>,
    rule: Rule,
    variant: VariantReading,
    /// Set for a test with a target_under_test
    under_test: bool,
}

impl Target<'_, '_> {
    /// What the target wants in its deps in `config`, where it has `old`
    pub fn deps(&self, config: &Configuration, old: &[String]) -> Result<Want> {
        let m = self
            .pkg
            .resolve(config, self.rule.kind, &self.variant.get(config))?;
        // A test with a target_under_test or a same-package dep (":foo")
        // gets its library's deps through it.
        let with_library = !self.under_test && !has_local_dep(old);
        let mut w = compose_deps(&m, self.rule.kind, with_library);
        w.canonical = self.rule.canonical;
        Ok(w)
    }

    /// The attributes other than the deps that the language sets for the
    /// target, in any configuration, sorted
    pub fn owned(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = Vec::new();
        for config in &self.pkg.space.configurations {
            let m = self
                .pkg
                .resolve(config, self.rule.kind, &self.variant.get(config))?;
            for name in m.attrs.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// The value the target wants for `name`, an attribute
    /// [`Target::owned`] returns, in `config`
    pub fn attr(&self, config: &Configuration, name: &str) -> Result<Vec<String>> {
        let m = self
            .pkg
            .resolve(config, self.rule.kind, &self.variant.get(config))?;
        Ok(m.attrs.get(name).cloned().unwrap_or_default())
    }
}

/// What a target of `kind` wants, from its package's mapping `m` in one
/// configuration. A library or a binary wants exactly what its sources
/// need. A test wants its test-only deps and, with its library, its
/// package's library deps too; test-only imports that couldn't be mapped
/// leave only a test's deps incomplete.
fn compose_deps(m: &PackageMapping, kind: TargetKind, with_library: bool) -> Want {
    let mut w = Want {
        unsynced: unsynced_targets(m),
        ..Want::default()
    };
    match kind {
        TargetKind::Library | TargetKind::Binary => {
            w.labels = deps_to_targets(&m.deps);
            w.unmapped = m.unmapped_imports.clone();
        }
        TargetKind::Test => {
            let mut seen = HashSet::new();
            let mut add = |deps: &[MappedDep], labels: &mut Vec<String>| {
                for d in deps_to_targets(deps) {
                    if seen.insert(d.clone()) {
                        labels.push(d);
                    }
                }
            };
            if with_library {
                add(&m.deps, &mut w.labels);
            }
            add(&m.test_deps, &mut w.labels);
            w.unmapped = m.unmapped_imports.clone();
            w.unmapped.extend(m.unmapped_test_imports.iter().cloned());
        }
        TargetKind::NotSynced => {}
    }
    w
}

/// Identifies a variant
fn variant_key(variant: &super::Variant) -> String {
    variant
        .iter()
        .map(|(name, value)| format!("{name}={};", rules_star::render(value)))
        .collect()
}

/// The targets of a mapping's unsynced deps
fn unsynced_targets(m: &PackageMapping) -> Vec<String> {
    m.unsynced_deps
        .iter()
        .filter(|u| !u.dep.target.is_empty())
        .map(|u| u.dep.target.clone())
        .collect()
}

/// Whether deps hold a local target dep (":foo")
fn has_local_dep(deps: &[String]) -> bool {
    deps.iter().any(|d| d.starts_with(':'))
}

/// The Buck2 package of the directory `pkg_dir`, e.g.
/// "/path/to/src/python/cargo" with project root "/path/to" is
/// "//src/python/cargo", or "" if it isn't under the project root.
fn buck_package(pkg_dir: &str, project_root: &str) -> String {
    match gostd::path::rel(project_root, pkg_dir) {
        Ok(rel) if rel == ".." || rel.starts_with("../") => String::new(),
        Ok(rel) if rel == "." => "//".to_string(),
        Ok(rel) => format!("//{rel}"),
        Err(_) => String::new(),
    }
}
