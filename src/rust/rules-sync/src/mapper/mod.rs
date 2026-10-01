//! The mapper: a package's dependencies resolved to Buck2 labels, through
//! one plug-in per language ([`Language`])
//!
//! A plug-in owns everything rules sync knows about its language: its rule
//! kinds, the files whose changes make its targets stale, and how a
//! package's deps are resolved. It says what a package wants; [`Package`]
//! composes that into what each target wants, and the syncer writes it.
//!
//! The Rust port of src/go/pkg/mapper.

mod cargo;
mod cargo_cfg;
mod cargo_features;
mod golang;
mod pymarkers;
mod python;
mod rust;
mod solidity;
mod target;
mod typescript;
mod uv;
mod variant;

pub use golang::{GoConfig, GoMember};
pub use target::{Package, Target, Want};
pub use variant::{Variant, read_variant};

use anyhow::{Context, Result, anyhow, bail};
use conditions::{CPU, Configuration, OS, OnOff};
use deps_extract::extraction;
use project_sync::config;
use project_sync::launch::Launcher;
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::path::Path;

/// How a dependency is classified
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DependencyType {
    /// The language's standard library, or a module of the same package:
    /// not a dep
    #[default]
    StdLib,
    /// A first-party dep
    Internal,
    /// A third-party dep, in the language's cell
    External,
    /// A dep nothing maps
    Unmapped,
}

/// A dependency resolved to its Buck2 target
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MappedDep {
    /// The Buck2 target, e.g. `//src/go/pkg/foo:foo`
    pub target: String,
    /// How it is classified
    pub kind: DependencyType,
    /// The import (or package name) it was resolved from
    pub import_path: String,
}

/// A dependency a manifest declares that sync doesn't manage: it is
/// neither added to nor removed from a target
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnsyncedDep {
    /// The target the dependency maps to
    pub dep: MappedDep,
    /// Why it isn't synced, e.g. "build dependency"
    pub reason: String,
}

/// The mapper's configuration
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The project's root directory
    pub project_root: String,
    /// The working directory relative paths are against: the process's,
    /// read once by its `main`
    pub cwd: String,
    /// The languages to resolve deps for, in order, each with the cell and
    /// deps file of its external deps
    pub languages: Vec<config::Language>,
    /// The build configurations sync evaluates: Go's allowed build tags,
    /// and the platforms the Rust plug-in reads a member's variants in
    pub conditions: config::ConditionsConfig,
    /// Starts the tools the plug-ins run (go, deps-extract, python3)
    pub launcher: Launcher,
}

/// Resolves packages' deps to Buck2 targets through language plug-ins
pub struct Mapper {
    languages: Vec<Box<dyn Language>>,
}

impl Mapper {
    /// A mapper with the plug-in of each configured language. A language
    /// without a plug-in is an error.
    pub fn new(cfg: &Config) -> Result<Mapper> {
        let mut languages: Vec<Box<dyn Language>> = Vec::new();
        for lang in &cfg.languages {
            languages.push(match lang.name.as_str() {
                "go" => Box::new(golang::GoLanguage::new(cfg, lang)),
                "rust" => Box::new(rust::RustLanguage::new(cfg, lang)),
                "python" => Box::new(python::PythonLanguage::new(cfg, lang)),
                "javascript" => Box::new(typescript::TypeScriptLanguage::new(cfg, lang)),
                "solidity" => Box::new(solidity::SolidityLanguage::new(cfg, lang)),
                name => bail!(
                    "no rules sync plug-in for language {}",
                    gostd::strconv::quote(name)
                ),
            });
        }
        Ok(Mapper { languages })
    }

    /// A mapper with the given plug-ins instead of the configured ones
    pub fn with(languages: Vec<Box<dyn Language>>) -> Mapper {
        Mapper { languages }
    }

    /// The plug-ins, in order
    pub fn languages(&self) -> &[Box<dyn Language>] {
        &self.languages
    }

    /// The plug-in named `name`
    pub fn language(&self, name: &str) -> Option<&dyn Language> {
        self.languages
            .iter()
            .find(|l| l.name() == name)
            .map(|l| l.as_ref())
    }

    /// The plug-in a Buck2 rule kind belongs to, and what it knows of the
    /// rule
    pub fn rule_language(&self, rule: &str) -> Option<(&dyn Language, Rule)> {
        self.languages
            .iter()
            .find_map(|l| l.rule(rule).map(|r| (l.as_ref(), r)))
    }
}

/// The dependencies of a package
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageMapping {
    /// The package's path
    pub path: String,
    /// The deps of its library and binary targets
    pub deps: Vec<MappedDep>,
    /// What its test targets need beyond those
    pub test_deps: Vec<MappedDep>,
    /// Non-test imports that couldn't be mapped: they leave the deps of
    /// every target built from the package incomplete
    pub unmapped_imports: Vec<String>,
    /// Test-only imports that couldn't be mapped: they leave only the test
    /// targets' deps incomplete
    pub unmapped_test_imports: Vec<String>,
    /// Declared deps sync doesn't manage (e.g. a Rust crate's build
    /// dependencies): neither added nor removed
    pub unsynced_deps: Vec<UnsyncedDep>,
    /// Other attributes of the package's targets that sync owns, with their
    /// values: e.g. a Rust target's "features". An attribute here is set to
    /// exactly its value; one with no values isn't added.
    pub attrs: BTreeMap<String, Vec<String>>,
}

/// The targets of deps
pub fn deps_to_targets(deps: &[MappedDep]) -> Vec<String> {
    deps.iter().map(|d| d.target.clone()).collect()
}

/// What a Buck2 rule builds, as far as sync is concerned
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TargetKind {
    /// A rule of the language whose deps sync leaves alone
    #[default]
    NotSynced = 0,
    /// Library targets depend on what their sources need
    Library = 1,
    /// Binary targets depend on what their sources need, like libraries
    Binary = 2,
    /// Test targets depend on what their sources and tests need
    Test = 3,
}

/// One language's rules sync plug-in
pub trait Language {
    /// Identifies the language, e.g. "go"
    fn name(&self) -> &str;

    /// What sync knows of a Buck2 rule kind, if it belongs to the language
    fn rule(&self, rule: &str) -> Option<Rule>;

    /// The file name patterns (`filepath.Match`) whose changes make the
    /// language's targets stale
    fn source_patterns(&self) -> &'static [&'static str];

    /// The configuration dimensions the deps of the package in `pkg_dir`
    /// depend on. Sync resolves the package once per combination of their
    /// values, and writes deps that differ as a `select()`.
    fn dimensions(&self, pkg_dir: &str) -> Result<Dimensions>;

    /// The deps of the package in `pkg_dir`, for one configuration and
    /// variant: deps for its library and binary targets, test deps for what
    /// its test targets need beyond those, and what couldn't be mapped or
    /// isn't synced
    fn resolve_deps(&self, pkg_dir: &str, req: &Request) -> Result<PackageMapping>;

    /// Whether sync manages the language's rules in the package in
    /// `pkg_dir`: a language may manage those of only some packages (Go,
    /// those of the workspace's modules), and sync leaves the others alone.
    fn manages(&self, _pkg_dir: &str) -> bool {
        true
    }
}

/// The configuration dimensions a package's deps depend on
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dimensions {
    /// The platform's dimensions they depend on ([`OS`], [`CPU`]), or none
    pub platform: Vec<String>,
    /// The on/off dimensions they depend on too, e.g. Go build tags: sync
    /// crosses the platforms with them
    pub on_off: Vec<OnOff>,
}

impl Dimensions {
    /// The dimensions' names
    fn names(&self) -> Vec<String> {
        let mut names = self.platform.clone();
        names.extend(self.on_off.iter().map(|o| o.name.clone()));
        names
    }
}

/// The dimensions of a package whose deps depend on the platform alone
fn platform() -> Dimensions {
    Dimensions {
        platform: vec![OS.to_string(), CPU.to_string()],
        on_off: Vec::new(),
    }
}

/// What sync knows of one of a language's Buck2 rule kinds
#[derive(Debug, Clone, Default)]
pub struct Rule {
    /// What its targets build
    pub kind: TargetKind,
    /// The attribute of its targets the resolved deps go in, e.g. "deps"
    pub deps_attribute: &'static str,
    /// The attributes of its targets that select which variant of the
    /// package they build (e.g. Rust features), or none. Sync reads them
    /// from the target, evaluating a `select()` for each configuration,
    /// and passes them to [`Language::resolve_deps`].
    pub variant: &'static [&'static str],
    /// The label an existing dep of its targets stands for, when that
    /// isn't the label itself: sync keeps an existing dep in place of a
    /// wanted label it stands for. `None` when every label stands for
    /// itself.
    pub canonical: Option<fn(&str) -> String>,
}

/// What one resolution of a package's deps is for
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    /// A value for each of the language's dimensions for the package;
    /// empty when the deps don't depend on the configuration
    pub config: Configuration,
    /// The kind of target the deps are for
    pub kind: TargetKind,
    /// The target's variant attributes ([`Rule::variant`]) that it sets,
    /// as they are in the configuration resolved
    pub variant: Variant,
}

/// A language whose deps come from the imports its sources make, mapped
/// one at a time
trait ImportLanguage {
    /// Maps one import. A [`DependencyType::StdLib`] result is skipped (the
    /// standard library, or a module of the same package); a
    /// [`DependencyType::Unmapped`] one is reported as unmapped.
    fn map_import(&self, imp: &extraction::Import) -> MappedDep;
}

/// Maps imports, dropping skipped ones and duplicate targets: the deps
/// sorted by target, and the imports that couldn't be mapped
fn map_imports(
    lang: &dyn ImportLanguage,
    imports: &[extraction::Import],
) -> (Vec<MappedDep>, Vec<String>) {
    let mut seen = HashSet::new();
    let (mut deps, mut unmapped) = (Vec::new(), Vec::new());
    for imp in imports {
        let dep = lang.map_import(imp);
        match dep.kind {
            DependencyType::StdLib => continue,
            DependencyType::Unmapped => {
                unmapped.push(imp.path.clone());
                continue;
            }
            _ => {}
        }
        if seen.insert(dep.target.clone()) {
            deps.push(dep);
        }
    }
    sort_deps(&mut deps);
    (deps, unmapped)
}

/// Maps one extracted package's imports and test imports
fn map_package(lang: &dyn ImportLanguage, pkg: &extraction::Package) -> PackageMapping {
    let (deps, unmapped_imports) = map_imports(lang, &pkg.imports);
    let (test_deps, unmapped_test_imports) = map_imports(lang, &pkg.test_imports);
    PackageMapping {
        path: pkg.path.clone(),
        deps,
        test_deps,
        unmapped_imports,
        unmapped_test_imports,
        ..PackageMapping::default()
    }
}

/// Maps an extraction result and merges the packages it found: a package
/// dir may hold several (e.g. Solidity's src/ and test/), all built into
/// the same targets.
fn resolve_imports(lang: &dyn ImportLanguage, result: &extraction::Result) -> PackageMapping {
    let mut merged = PackageMapping::default();
    let (mut seen_deps, mut seen_test_deps) = (HashSet::new(), HashSet::new());
    for pkg in &result.packages {
        let m = map_package(lang, pkg);
        for dep in m.deps {
            if seen_deps.insert(dep.target.clone()) {
                merged.deps.push(dep);
            }
        }
        for dep in m.test_deps {
            if seen_test_deps.insert(dep.target.clone()) {
                merged.test_deps.push(dep);
            }
        }
        merged.unmapped_imports.extend(m.unmapped_imports);
        merged.unmapped_test_imports.extend(m.unmapped_test_imports);
    }
    merged
}

/// Runs deps-extract on `pkg_dir` for a language
fn run_deps_extract(
    launcher: &Launcher,
    project_root: &str,
    lang: &str,
    pkg_dir: &str,
) -> Result<extraction::Result> {
    let mut cmd = launcher
        .command(
            OsStr::new("deps-extract"),
            &["--lang", lang, pkg_dir],
            Some(Path::new(project_root)),
        )
        .map_err(|_| {
            anyhow!(
                "deps-extract not found in PATH (install with: cargo install --path src/rust/deps-extract)"
            )
        })?;
    let output = cmd
        .stderr(std::process::Stdio::piped())
        .output()
        .context("running deps-extract")?;
    if !output.status.success() {
        bail!(
            "deps-extract failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing deps-extract output")
}

/// A package's deps, from the imports deps-extract finds in it
fn resolve_with_deps_extract(
    lang: &dyn ImportLanguage,
    launcher: &Launcher,
    name: &str,
    project_root: &str,
    pkg_dir: &str,
) -> Result<PackageMapping> {
    let result =
        run_deps_extract(launcher, project_root, name, pkg_dir).context("extractor failed")?;
    Ok(resolve_imports(lang, &result))
}

/// Sorts deps by target, for stable output.
fn sort_deps(deps: &mut [MappedDep]) {
    deps.sort_by(|a, b| a.target.cmp(&b.target));
}

/// The result of mapping an import nothing maps
fn unmapped_dep(path: &str) -> MappedDep {
    MappedDep {
        import_path: path.to_string(),
        kind: DependencyType::Unmapped,
        ..MappedDep::default()
    }
}

/// The result of mapping an import that isn't a dep
fn skipped_dep(path: &str) -> MappedDep {
    MappedDep {
        import_path: path.to_string(),
        kind: DependencyType::StdLib,
        ..MappedDep::default()
    }
}

/// A variant attribute's labels, if it is a list of labels
fn variant_labels(variant: &Variant, name: &str) -> Vec<String> {
    rules_star::labels(variant.get(name)).unwrap_or_default()
}

/// Reads a TOML file as `T`: an error reading it is worded as Go's, and
/// one parsing it names the file
fn read_toml<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    let content =
        std::fs::read_to_string(path).map_err(|err| anyhow!(path_error("open", path, &err)))?;
    toml::from_str(&content).map_err(|err| anyhow!("parsing {path}: {}", err.message()))
}

/// An I/O error as Go's `*fs.PathError` prints it: `<op> <path>: <error>`,
/// with the error as Go names it ("no such file or directory").
pub(crate) fn path_error(op: &str, path: &str, err: &std::io::Error) -> String {
    let text = err.to_string();
    let text = text
        .find(" (os error ")
        .map_or(text.as_str(), |i| &text[..i]);
    let mut chars = text.chars();
    let text: String = match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    };
    format!("{op} {path}: {text}")
}

/// `filepath.Glob` on a string pattern: the matching paths
fn glob(pattern: &str) -> std::result::Result<Vec<String>, gostd::filepath::BadPattern> {
    Ok(gostd::filepath::glob(Path::new(pattern))?
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Whether the file at `path` exists (`os.Stat` succeeds)
fn exists(path: &str) -> bool {
    std::fs::metadata(path).is_ok()
}

#[cfg(test)]
mod tests;
