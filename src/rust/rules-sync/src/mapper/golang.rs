//! The Go plug-in: deps from the imports `go list` reports, on each
//! platform, and for a library with each combination of the allowed build
//! tags its build constraints use

use super::{
    DependencyType, Dimensions, ImportLanguage, Language, MappedDep, PackageMapping, Request, Rule,
    TargetKind, platform, read_toml, resolve_imports, skipped_dep, unmapped_dep, variant_labels,
};
use anyhow::{Context, Result};
use deps_extract::extraction::{self, Import, ImportKind};
use project_sync::config;
use project_sync::launch::Launcher;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::path::Path;

/// The project's Go configuration
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoConfig {
    /// The workspace's modules (ADR 0007), the only first-party Go code:
    /// go-deps.toml's `[members]`, or the root go.mod alone when it records
    /// none
    pub members: Vec<GoMember>,
    /// The Buck2 cell of external deps, e.g. "godeps"
    pub external_cell: String,
    /// The import paths of the modules in go-deps.toml
    pub external_deps: HashSet<String>,
}

/// A module of the Go workspace
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoMember {
    /// Its module path
    pub path: String,
    /// Its directory, relative to the project root ("." for the root), with
    /// forward slashes
    pub dir: String,
}

/// The Go rule kinds. A binary or a test is built with its build_tags,
/// literally; a library gets its tags from the configuration.
fn go_rule(rule: &str) -> Option<Rule> {
    let (kind, variant): (TargetKind, &'static [&'static str]) = match rule {
        "go_library" | "go_exported_library" => (TargetKind::Library, &[]),
        "go_binary" => (TargetKind::Binary, &["build_tags"]),
        "go_test" => (TargetKind::Test, &["build_tags"]),
        _ => return None,
    };
    Some(Rule {
        kind,
        deps_attribute: "deps",
        variant,
        canonical: None,
    })
}

/// Resolves a Go package's deps from the imports go list reports
pub struct GoLanguage {
    project_root: String,
    cfg: GoConfig,
    /// The build tags a library's deps may vary with (sync.toml's
    /// `[conditions] go_tags`)
    allowed_tags: Vec<String>,
    launcher: Launcher,
    /// The working directory relative paths are against
    cwd: String,
}

impl GoLanguage {
    pub fn new(mcfg: &super::Config, lang: &config::Language) -> Self {
        GoLanguage {
            project_root: mcfg.project_root.clone(),
            cfg: detect_go_config(&mcfg.project_root, lang),
            allowed_tags: mcfg.conditions.go_tags.clone(),
            launcher: mcfg.launcher.clone(),
            cwd: mcfg.cwd.clone(),
        }
    }

    /// A plug-in with the given configuration, for tests
    #[cfg(test)]
    pub(super) fn with_config(project_root: &str, cfg: GoConfig) -> Self {
        GoLanguage {
            project_root: project_root.to_string(),
            cfg,
            allowed_tags: Vec::new(),
            launcher: Launcher::default(),
            cwd: "/".to_string(),
        }
    }

    /// The build tags a request is resolved with: a binary's or test's
    /// build_tags, literally, or the tags a library's configuration sets
    /// (`config_tags`)
    fn build_tags(req: &Request, config_tags: Vec<String>) -> Vec<String> {
        match req.kind {
            TargetKind::Binary | TargetKind::Test => variant_labels(&req.variant, "build_tags"),
            _ => config_tags,
        }
    }

    /// Lists the imports of the Go packages under `pkg_dir` with go list, in
    /// the environment `env` (added to the process's) and with `tags`.
    fn extract(
        &self,
        pkg_dir: &str,
        env: &[(String, String)],
        tags: &[String],
    ) -> Result<extraction::Result> {
        let mut result = extraction::Result::new("go");
        let joined = tags.join(",");
        let mut args = vec!["list", "-e", "-json"];
        if !tags.is_empty() {
            args.extend(["-tags", joined.as_str()]);
        }
        args.push("./...");
        let mut cmd = self
            .launcher
            .command(OsStr::new("go"), &args, Some(Path::new(pkg_dir)))
            .context("running go list")?;
        cmd.envs(env.iter().map(|(k, v)| (k, v)));
        let output = cmd
            .stderr(std::process::Stdio::piped())
            .output()
            .context("running go list")?;
        if !output.status.success() {
            result.add_error(format!(
                "go list warning: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // A stream of JSON objects, one per package; one that doesn't
        // decode is skipped
        for pkg in serde_json::Deserializer::from_slice(&output.stdout).into_iter::<GoListPackage>()
        {
            let Ok(pkg) = pkg else {
                break;
            };
            let rel = gostd::path::rel(&self.project_root, &pkg.dir).unwrap_or(pkg.dir.clone());
            let classify = |imp: &String| Import {
                path: imp.clone(),
                kind: classify_go_import(imp, &self.cfg),
            };
            result.add_package(extraction::Package {
                path: rel,
                files: pkg.go_files,
                imports: pkg.imports.iter().map(classify).collect(),
                test_imports: pkg
                    .test_imports
                    .iter()
                    .chain(&pkg.x_test_imports)
                    .map(classify)
                    .collect(),
            });
        }
        Ok(result)
    }

    /// Maps an internal Go import to the target of its package in the
    /// member that owns it, named after the import path's last component, as
    /// buckgen names every Go package (and the godeps cell's forwarding
    /// aliases name a member's packages).
    fn map_internal(&self, import_path: &str) -> MappedDep {
        let Some(m) = self.cfg.member(import_path) else {
            return unmapped_dep(import_path);
        };
        // e.g. "src/go/pkg/foo" -> "//src/go/pkg/foo:foo", or a member's
        // root package github.com/x/foo in third_party/fork ->
        // "//third_party/fork:foo"
        let rest = &import_path[m.path.len()..];
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let mut dir = gostd::path::join(&[&m.dir, rest]);
        if dir == "." {
            dir = String::new();
        }
        MappedDep {
            target: format!("//{dir}:{}", path_base(import_path)),
            kind: DependencyType::Internal,
            import_path: import_path.to_string(),
        }
    }

    /// Maps an external Go import to its target in the external cell, e.g.
    /// "golang.org/x/sys/cpu" -> "godeps//vendor/golang.org/x/sys/cpu:cpu"
    fn map_external(&self, import_path: &str) -> MappedDep {
        if self.cfg.external_module(import_path).is_empty() {
            return unmapped_dep(import_path);
        }
        MappedDep {
            target: format!(
                "{}//vendor/{import_path}:{}",
                self.cfg.external_cell,
                gostd::path::base(import_path)
            ),
            kind: DependencyType::External,
            import_path: import_path.to_string(),
        }
    }
}

/// A package as `go list -json` reports it, as far as sync reads it
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct GoListPackage {
    dir: String,
    go_files: Vec<String>,
    imports: Vec<String>,
    test_imports: Vec<String>,
    #[serde(rename = "XTestImports")]
    x_test_imports: Vec<String>,
}

impl Language for GoLanguage {
    fn name(&self) -> &str {
        "go"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        go_rule(rule)
    }

    fn source_patterns(&self) -> &'static [&'static str] {
        &["*.go"]
    }

    /// The platform, and each allowed build tag the package's build
    /// constraints use.
    fn dimensions(&self, pkg_dir: &str) -> Result<Dimensions> {
        let mut dims = platform();
        for tag in goparse::tree_constraint_tags(pkg_dir)? {
            if self.allowed_tags.contains(&tag) {
                dims.on_off.push(goparse::tag_dimension(&tag));
            }
        }
        Ok(dims)
    }

    fn resolve_deps(&self, pkg_dir: &str, req: &Request) -> Result<PackageMapping> {
        let (ctx, _) = goparse::config_context(&req.config);
        let env: Vec<(String, String)> = ctx
            .environ()
            .iter()
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let tags = Self::build_tags(req, ctx.tags);
        let result = self
            .extract(pkg_dir, &env, &tags)
            .context("extractor failed")?;
        Ok(resolve_imports(self, &result))
    }

    /// Whether rules sync manages the Go rules of the package in `pkg_dir`:
    /// whether the module it is in, the nearest go.mod above it, is a
    /// workspace member. Any other go.mod is outside the Go build, as it is
    /// for go ./... in the workspace (ADR 0007), e.g. a test fixture's.
    fn manages(&self, pkg_dir: &str) -> bool {
        let (root, abs) = (
            absolute(&self.project_root, &self.cwd),
            absolute(pkg_dir, &self.cwd),
        );
        let Ok(rel) = gostd::path::rel(&root, &abs) else {
            return false;
        };
        let mut dir = rel;
        while dir != ".." && !dir.starts_with("../") {
            if std::fs::metadata(gostd::path::join(&[&root, &dir, "go.mod"])).is_ok() {
                return self
                    .cfg
                    .members
                    .iter()
                    .any(|m| gostd::path::clean(&m.dir) == dir);
            }
            if dir == "." {
                break;
            }
            dir = gostd::path::dir(&dir);
        }
        false
    }
}

impl ImportLanguage for GoLanguage {
    fn map_import(&self, imp: &Import) -> MappedDep {
        match imp.kind {
            ImportKind::Stdlib => skipped_dep(&imp.path),
            ImportKind::Internal => self.map_internal(&imp.path),
            ImportKind::External => self.map_external(&imp.path),
        }
    }
}

/// `filepath.Abs`: `path` cleaned, against the working directory `cwd`
/// when it is relative
fn absolute(path: &str, cwd: &str) -> String {
    if gostd::path::is_abs(path) {
        return gostd::path::clean(path);
    }
    gostd::path::join(&[cwd, path])
}

/// `path.Base`
fn path_base(p: &str) -> String {
    gostd::path::base(p)
}

/// Whether an import is of the standard library, external or internal:
/// internal when the module that owns it is a workspace member, the longest
/// module path prefixing it on a / boundary among the members and the
/// modules of go-deps.toml (a member's module path can prefix a third-party
/// one's).
pub fn classify_go_import(imp: &str, cfg: &GoConfig) -> ImportKind {
    let first = match imp.find('/') {
        Some(i) if i > 0 => &imp[..i],
        _ => imp,
    };
    if !first.contains('.') {
        return ImportKind::Stdlib;
    }
    if let Some(m) = cfg.member(imp)
        && m.path.len() >= cfg.external_module(imp).len()
    {
        return ImportKind::Internal;
    }
    ImportKind::External
}

/// Whether `import_path` is in the module `module_path`: the module path
/// itself, or below it on a / boundary
fn within(import_path: &str, module_path: &str) -> bool {
    import_path == module_path
        || import_path
            .strip_prefix(module_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl GoConfig {
    /// The workspace member that owns `import_path`: the one whose module
    /// path is its longest prefix, on a / boundary
    pub fn member(&self, import_path: &str) -> Option<&GoMember> {
        let mut owner: Option<&GoMember> = None;
        for m in &self.members {
            if within(import_path, &m.path) && owner.is_none_or(|o| m.path.len() > o.path.len()) {
                owner = Some(m);
            }
        }
        owner
    }

    /// The longest module path of go-deps.toml that `import_path` is in,
    /// or ""
    pub fn external_module(&self, import_path: &str) -> &str {
        let mut owner = "";
        for dep in &self.external_deps {
            if within(import_path, dep) && dep.len() > owner.len() {
                owner = dep;
            }
        }
        owner
    }
}

/// The project's Go configuration, with the language's cell and deps file
pub fn detect_go_config(project_root: &str, lang: &config::Language) -> GoConfig {
    let mut cfg = GoConfig {
        external_cell: lang.cell.clone(),
        ..GoConfig::default()
    };
    if let Ok((deps, members)) = load_go_deps(&gostd::path::join(&[project_root, &lang.deps_file]))
    {
        cfg.external_deps = deps;
        cfg.members = members;
    }
    // Without recorded members, the workspace is the root go.mod alone
    if cfg.members.is_empty()
        && let Ok(content) = std::fs::read_to_string(gostd::path::join(&[project_root, "go.mod"]))
    {
        let module_path = gomod::modfile::module_path(&content);
        if !module_path.is_empty() {
            cfg.members = vec![GoMember {
                path: module_path,
                dir: ".".to_string(),
            }];
        }
    }
    cfg
}

/// go-deps.toml, as far as sync reads it
#[derive(Deserialize, Default)]
#[serde(default)]
struct GoDepsFile {
    deps: BTreeMap<String, GoDep>,
    members: BTreeMap<String, GoDepsMember>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GoDep {
    import_path: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GoDepsMember {
    dir: String,
}

/// The import paths of the modules in go-deps.toml, and the workspace
/// members it records, sorted by directory.
///
/// Entries are keyed "path@version" (schema 2), so the import path comes
/// from each entry's import_path; a key without one is taken as the path
/// itself.
fn load_go_deps(path: &str) -> Result<(HashSet<String>, Vec<GoMember>)> {
    let file: GoDepsFile = read_toml(path)?;
    let deps = file
        .deps
        .into_iter()
        .map(|(key, dep)| {
            if dep.import_path.is_empty() {
                key
            } else {
                dep.import_path
            }
        })
        .collect();
    let mut members: Vec<GoMember> = file
        .members
        .into_iter()
        .map(|(path, m)| GoMember { path, dir: m.dir })
        .collect();
    members.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok((deps, members))
}
