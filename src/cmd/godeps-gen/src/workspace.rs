//! The Go workspace of a project: the modules it resolves together (ADR
//! 0007), and the dependencies they resolve to
//!
//! A workspace is the members of the project's go.work, or, without one,
//! its go.mod alone. Paths are relative to the project root, with forward
//! slashes, and are handled as Go's `path` and `path/filepath` handle them
//! (see `gostd::path`).

use crate::deps::{Dependency, ParseOptions, parse_go_sum};
use anyhow::{Context, Result, anyhow, bail};
use gomod::{ModFile, Replace, WorkFile, is_directory_path};
use gostd::path::{clean, dir as path_dir, is_abs, join, rel};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::process::Command;

/// A Go workspace
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The go.work, or empty for a workspace of one go.mod
    pub work_file: String,
    /// The workspace's modules, in go.work's order
    pub members: Vec<Member>,
    /// The files the workspace was read from, existing or not: go.work and
    /// go.work.sum, then each member's go.mod and go.sum
    pub sources: Vec<String>,
    /// Each member's parsed go.mod, by member index
    mod_files: Vec<ModFile>,
    /// The parsed go.work, without one `None`
    work: Option<WorkFile>,
}

/// A module of the workspace: first-party code, never fetched
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The module path its go.mod declares
    pub path: String,
    /// Its directory, relative to the project root ("." for the root)
    pub dir: String,
    /// Its go.sum (or the one named on the command line)
    pub sum_file: String,
}

/// The content of `root`/`file`, `None` when it doesn't exist
fn read_file(root: &str, file: &str) -> Result<Option<Vec<u8>>> {
    let path = join(&[root, file]);
    match std::fs::read(&path) {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("open {path}: {e}")),
    }
}

/// A file's content as text. x/mod reads bytes: an invalid UTF-8 sequence
/// (in a comment, say) reads as U+FFFD here, as Go's lexer decodes it.
fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

impl Workspace {
    /// The workspace of the project at `root`. With `work_file` present it
    /// is a go.work workspace, and `mod_file` and `sum_file` are unused;
    /// otherwise `mod_file` (with `sum_file`) is its only member. File
    /// arguments are relative to `root`.
    pub fn load(root: &str, work_file: &str, mod_file: &str, sum_file: &str) -> Result<Workspace> {
        let Some(work_data) = read_file(root, work_file)? else {
            // go.work is a source though absent: adding one changes the
            // workspace
            let (work_file, mod_file, sum_file) =
                (clean(work_file), clean(mod_file), clean(sum_file));
            let mut ws = Workspace {
                work_file: String::new(),
                members: Vec::new(),
                sources: vec![
                    work_file.clone(),
                    format!("{work_file}.sum"),
                    mod_file.clone(),
                    sum_file.clone(),
                ],
                mod_files: Vec::new(),
                work: None,
            };
            ws.add_member(root, &path_dir(&mod_file), &mod_file, &sum_file)?;
            return Ok(ws);
        };

        let work = gomod::parse_work(work_file, &text(&work_data))?;
        let cleaned = clean(work_file);
        let mut ws = Workspace {
            work_file: cleaned.clone(),
            members: Vec::new(),
            sources: vec![cleaned.clone(), format!("{cleaned}.sum")],
            mod_files: Vec::new(),
            work: None,
        };
        for use_dir in &work.use_dirs {
            let dir = project_path(root, &path_dir(&cleaned), use_dir)
                .map_err(|e| anyhow!("{work_file}: use {use_dir}: {e}"))?;
            let (m, s) = (join(&[&dir, "go.mod"]), join(&[&dir, "go.sum"]));
            ws.sources.push(m.clone());
            ws.sources.push(s.clone());
            ws.add_member(root, &dir, &m, &s)?;
        }
        ws.work = Some(work);
        Ok(ws)
    }

    /// Read the go.mod of the member in `dir`
    fn add_member(&mut self, root: &str, dir: &str, mod_file: &str, sum_file: &str) -> Result<()> {
        let data = read_file(root, mod_file)?.ok_or_else(|| {
            anyhow!(
                "open {}: no such file or directory",
                join(&[root, mod_file])
            )
        })?;
        let f = gomod::parse_mod(mod_file, &text(&data))?;
        let Some(path) = f.module.clone() else {
            bail!("{mod_file}: no module directive");
        };
        self.members.push(Member {
            path,
            dir: dir.to_string(),
            sum_file: sum_file.to_string(),
        });
        self.mod_files.push(f);
        Ok(())
    }

    /// Fail on a local-path replace, in go.work or a member's go.mod, that
    /// points anywhere but a workspace member: a local module is
    /// first-party code, and the members are all of it
    pub fn check_local_replaces(&self, root: &str) -> Result<()> {
        let members: HashSet<&str> = self.members.iter().map(|m| m.dir.as_str()).collect();
        let check = |file: &str, dir: &str, replaces: &[Replace]| -> Result<()> {
            for r in replaces {
                if !is_directory_path(&r.new.path) {
                    continue;
                }
                match project_path(root, dir, &r.new.path) {
                    Ok(target) if members.contains(target.as_str()) => {}
                    _ => bail!(
                        "{file}: replace {} => {}: not a workspace member; add it to go.work",
                        r.old.path,
                        r.new.path
                    ),
                }
            }
            Ok(())
        };
        if let Some(work) = &self.work {
            check(&self.work_file, &path_dir(&self.work_file), &work.replace)?;
        }
        for (m, f) in self.members.iter().zip(&self.mod_files) {
            check(&join(&[&m.dir, "go.mod"]), &m.dir, &f.replace)?;
        }
        Ok(())
    }

    /// The modules the members require, minus the members themselves, at
    /// the version the first member requiring it names, sorted by path. A
    /// module is indirect when every member requiring it says so.
    pub fn requires(&self, opts: ParseOptions) -> Vec<Dependency> {
        let members: HashSet<&str> = self.members.iter().map(|m| m.path.as_str()).collect();
        let mut by_path: BTreeMap<&str, Dependency> = BTreeMap::new();
        for f in &self.mod_files {
            for req in &f.require {
                let path = req.module.path.as_str();
                if members.contains(path) || (req.indirect && !opts.include_indirect) {
                    continue;
                }
                by_path
                    .entry(path)
                    .and_modify(|dep| dep.indirect = dep.indirect && req.indirect)
                    .or_insert_with(|| Dependency {
                        import_path: path.to_string(),
                        version: req.module.version.clone(),
                        indirect: req.indirect,
                        ..Default::default()
                    });
            }
        }
        by_path.into_values().collect()
    }

    /// The go.sum hashes of every member and of go.work.sum, keyed as
    /// [`parse_go_sum`] keys them. A missing go.sum has none.
    pub fn sum_hashes(&self, root: &str) -> Result<HashMap<String, String>> {
        let mut files: Vec<String> = self.members.iter().map(|m| m.sum_file.clone()).collect();
        if !self.work_file.is_empty() {
            files.push(format!("{}.sum", self.work_file));
        }
        let mut hashes = HashMap::new();
        for file in files {
            let Some(data) = read_file(root, &file)? else {
                continue;
            };
            hashes.extend(parse_go_sum(&data).with_context(|| file.clone())?);
        }
        Ok(hashes)
    }

    /// The workspace's dependencies: the modules its members require, at
    /// the version Go selects for the whole workspace (which can be above
    /// any version a member names), fetched from the replacement a
    /// non-local replace names, with their go.sum hashes
    pub fn resolve(
        &self,
        root: &str,
        lister: &impl ModuleLister,
        opts: ParseOptions,
    ) -> Result<Vec<Dependency>> {
        self.check_local_replaces(root)?;
        let listed = lister.list_modules(root, self)?;
        let selected: HashMap<&str, &ListedModule> =
            listed.iter().map(|m| (m.path.as_str(), m)).collect();
        let hashes = self.sum_hashes(root)?;

        let mut deps = Vec::new();
        for mut dep in self.requires(opts) {
            let Some(m) = selected.get(dep.import_path.as_str()) else {
                bail!(
                    "go list -m all does not list {}, which a workspace member requires",
                    dep.import_path
                );
            };
            if m.main {
                continue;
            }
            match &m.replace {
                Some(r) if r.version.is_empty() => {
                    // check_local_replaces let it through, so Go would have
                    // made the replacement a member
                    bail!(
                        "{} is replaced by local directory {}, which is not a workspace member; add it to go.work",
                        m.path,
                        r.path
                    );
                }
                Some(r) => {
                    dep.version = r.version.clone();
                    if r.path != dep.import_path {
                        dep.fetch_path = r.path.clone();
                    }
                }
                None => dep.version = m.version.clone(),
            }
            dep.go_sum_hash = hashes
                .get(&format!("{} {}", dep.effective_fetch_path(), dep.version))
                .cloned()
                .unwrap_or_default();
            deps.push(dep);
        }
        Ok(deps)
    }
}

/// `p`, written in the file in `dir` (both relative to `root`), as a clean
/// path relative to `root`. A path outside the project has no Buck2
/// target, so it is an error.
fn project_path(root: &str, dir: &str, p: &str) -> Result<String> {
    let abs = if is_abs(p) {
        p.to_string()
    } else {
        join(&[root, dir, p])
    };
    let rel = rel(root, &abs).map_err(|e| anyhow!(e))?;
    if rel == ".." || rel.starts_with("../") {
        bail!("{p} is outside the project");
    }
    Ok(rel)
}

/// A module of the build list Go selects, as `go list -m -json all`
/// reports it
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListedModule {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub version: String,
    /// A workspace member
    #[serde(default)]
    pub main: bool,
    #[serde(default)]
    pub replace: Option<Box<ListedModule>>,
}

/// Reports the build list Go selects for a workspace
pub trait ModuleLister {
    fn list_modules(&self, root: &str, ws: &Workspace) -> Result<Vec<ListedModule>>;
}

/// Lists modules with `go list -m -json all`, in workspace mode for a
/// go.work workspace and in module mode otherwise
pub struct GoLister {
    /// The go command to run
    pub go: OsString,
    /// The environment go runs in, minus the GOWORK and GOFLAGS the lister
    /// sets itself
    pub env: Vec<(OsString, OsString)>,
}

impl ModuleLister for GoLister {
    fn list_modules(&self, root: &str, ws: &Workspace) -> Result<Vec<ListedModule>> {
        let mut cmd = Command::new(&self.go);
        cmd.args(["list", "-m", "-json", "all"]);
        let gowork = if ws.work_file.is_empty() {
            cmd.current_dir(join(&[root, &ws.members[0].dir]));
            "off".to_string()
        } else {
            // root is absolute, so the joined path is the absolute one
            let abs = join(&[root, &ws.work_file]);
            cmd.current_dir(path_dir(&abs));
            abs
        };
        cmd.env_clear();
        cmd.envs(
            self.env
                .iter()
                .filter(|(k, _)| k != "GOWORK" && k != "GOFLAGS")
                .map(|(k, v)| (k, v)),
        );
        // -mod=readonly: an untidy go.mod is an error to fix, not a file
        // for godeps-gen to rewrite
        cmd.env("GOWORK", gowork).env("GOFLAGS", "-mod=readonly");
        let out = cmd
            .output()
            .map_err(|e| anyhow!("go list -m -json all: {e}"))?;
        if !out.status.success() {
            bail!(
                "go list -m -json all: {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        serde_json::Deserializer::from_slice(&out.stdout)
            .into_iter::<ListedModule>()
            .collect::<Result<_, _>>()
            .map_err(|e| anyhow!("go list -m -json all: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Write files (path relative to root -> content) under root
    fn write_files(root: &Path, files: &[(&str, &str)]) {
        for (name, content) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    fn root_of(dir: &tempfile::TempDir) -> String {
        dir.path().to_str().unwrap().to_string()
    }

    /// Reports a fixed build list
    struct FakeLister(Vec<ListedModule>);

    impl ModuleLister for FakeLister {
        fn list_modules(&self, _: &str, _: &Workspace) -> Result<Vec<ListedModule>> {
            Ok(self.0.clone())
        }
    }

    fn listed(path: &str, version: &str) -> ListedModule {
        ListedModule {
            path: path.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    fn main_module(path: &str) -> ListedModule {
        ListedModule {
            path: path.into(),
            main: true,
            ..Default::default()
        }
    }

    fn dep(import_path: &str, version: &str) -> Dependency {
        Dependency {
            import_path: import_path.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    /// A go.work workspace where member a requires member b
    const TWO_MEMBERS: [(&str, &str); 4] = [
        ("go.work", "go 1.22\n\nuse (\n\t./a\n\t./b\n)\n"),
        (
            "a/go.mod",
            "module example.com/a\n\ngo 1.22\n\nrequire (\n\texample.com/b v0.0.0\n\tgithub.com/google/uuid v1.3.0\n)\n\nreplace example.com/b => ../b\n",
        ),
        (
            "b/go.mod",
            "module example.com/b\n\ngo 1.22\n\nrequire golang.org/x/mod v0.31.0 // indirect\n",
        ),
        (
            "b/go.sum",
            "golang.org/x/mod v0.31.0 h1:mod=\ngolang.org/x/mod v0.31.0/go.mod h1:gomod=\n",
        ),
    ];

    #[test]
    fn members_are_recorded_and_never_deps() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(tmp.path(), &TWO_MEMBERS);
        let root = root_of(&tmp);
        let ws = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap();
        let members: Vec<String> = ws
            .members
            .iter()
            .map(|m| format!("{}={}", m.path, m.dir))
            .collect();
        assert_eq!(members, vec!["example.com/a=a", "example.com/b=b"]);
        assert_eq!(
            ws.sources,
            vec![
                "go.work",
                "go.work.sum",
                "a/go.mod",
                "a/go.sum",
                "b/go.mod",
                "b/go.sum"
            ]
        );

        let deps = ws
            .resolve(
                &root,
                &FakeLister(vec![
                    main_module("example.com/a"),
                    main_module("example.com/b"),
                    listed("github.com/google/uuid", "v1.3.0"),
                    listed("golang.org/x/mod", "v0.31.0"),
                ]),
                ParseOptions::default(),
            )
            .unwrap();
        let mut x_mod = dep("golang.org/x/mod", "v0.31.0");
        x_mod.indirect = true;
        x_mod.go_sum_hash = "h1:mod=".into();
        assert_eq!(deps, vec![dep("github.com/google/uuid", "v1.3.0"), x_mod]);
    }

    #[test]
    fn records_the_version_go_selects() {
        // Workspace MVS can select a version above what any member names,
        // when a dependency's go.mod asks for it
        let tmp = tempfile::tempdir().unwrap();
        write_files(tmp.path(), &TWO_MEMBERS);
        let root = root_of(&tmp);
        let ws = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap();
        let deps = ws
            .resolve(
                &root,
                &FakeLister(vec![
                    main_module("example.com/a"),
                    main_module("example.com/b"),
                    listed("github.com/google/uuid", "v1.6.0"),
                    listed("golang.org/x/mod", "v0.31.0"),
                ]),
                ParseOptions::default(),
            )
            .unwrap();
        assert_eq!(deps[0].version, "v1.6.0");
    }

    #[test]
    fn fetches_an_external_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[(
                "go.mod",
                "module example.com/m\n\nrequire github.com/up/lib v1.0.0\n\nreplace github.com/up/lib => github.com/fork/lib v1.0.1\n",
            )],
        );
        let root = root_of(&tmp);
        let ws = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap();
        let mut up = listed("github.com/up/lib", "v1.0.0");
        up.replace = Some(Box::new(listed("github.com/fork/lib", "v1.0.1")));
        let deps = ws
            .resolve(
                &root,
                &FakeLister(vec![main_module("example.com/m"), up]),
                ParseOptions::default(),
            )
            .unwrap();
        let mut want = dep("github.com/up/lib", "v1.0.1");
        want.fetch_path = "github.com/fork/lib".into();
        assert_eq!(deps, vec![want]);
    }

    #[test]
    fn a_local_replace_must_be_a_member() {
        let cases: [(&str, &[(&str, &str)]); 3] = [
            (
                "in a go.mod without go.work",
                &[
                    (
                        "go.mod",
                        "module example.com/m\n\nrequire example.com/lib v0.0.0\n\nreplace example.com/lib => ./lib\n",
                    ),
                    ("lib/go.mod", "module example.com/lib\n"),
                ],
            ),
            (
                "outside the project",
                &[(
                    "go.mod",
                    "module example.com/m\n\nrequire example.com/lib v0.0.0\n\nreplace example.com/lib => ../lib\n",
                )],
            ),
            (
                "in go.work",
                &[
                    (
                        "go.work",
                        "go 1.22\n\nuse ./m\n\nreplace example.com/lib => ./lib\n",
                    ),
                    ("m/go.mod", "module example.com/m\n"),
                    ("lib/go.mod", "module example.com/lib\n"),
                ],
            ),
        ];
        for (name, files) in cases {
            let tmp = tempfile::tempdir().unwrap();
            write_files(tmp.path(), files);
            let root = root_of(&tmp);
            let ws = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap();
            let err = ws
                .resolve(&root, &FakeLister(vec![]), ParseOptions::default())
                .unwrap_err();
            assert!(
                err.to_string().contains("add it to go.work"),
                "{name}: {err}"
            );
        }
    }

    #[test]
    fn a_single_module_is_a_workspace_of_one() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                (
                    "go.mod",
                    "module example.com/m\n\nrequire github.com/google/uuid v1.6.0\n",
                ),
                ("go.sum", "github.com/google/uuid v1.6.0 h1:uuid=\n"),
            ],
        );
        let root = root_of(&tmp);
        let ws = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap();
        // go.work is a source though absent: adding one makes go-deps.toml
        // stale
        assert_eq!(ws.work_file, "");
        assert_eq!(
            ws.sources,
            vec!["go.work", "go.work.sum", "go.mod", "go.sum"]
        );
        assert_eq!(
            ws.members,
            vec![Member {
                path: "example.com/m".into(),
                dir: ".".into(),
                sum_file: "go.sum".into(),
            }]
        );
        let deps = ws
            .resolve(
                &root,
                &FakeLister(vec![
                    main_module("example.com/m"),
                    listed("github.com/google/uuid", "v1.6.0"),
                ]),
                ParseOptions::default(),
            )
            .unwrap();
        let mut want = dep("github.com/google/uuid", "v1.6.0");
        want.go_sum_hash = "h1:uuid=".into();
        assert_eq!(deps, vec![want]);
    }

    #[test]
    fn the_go_lister_resolves_a_workspace() {
        // The go on PATH, found as the go command finds it
        let Some(go) = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("go"))
                .find(|p| p.is_file())
        }) else {
            eprintln!("no go on PATH");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        // Members only, so go needs neither the network nor a module
        // cache. Go fetches a member's go.mod at the required version
        // unless a replace points the require at the member.
        write_files(
            tmp.path(),
            &[
                ("go.work", "go 1.22\n\nuse (\n\t./a\n\t./b\n)\n"),
                (
                    "a/go.mod",
                    "module example.com/a\n\ngo 1.22\n\nrequire example.com/b v0.0.0\n\nreplace example.com/b => ../b\n",
                ),
                ("b/go.mod", "module example.com/b\n\ngo 1.22\n"),
            ],
        );
        let root = root_of(&tmp);
        let ws = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap();
        // An inherited GOWORK=off must not turn workspace mode off
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        env.push(("GOWORK".into(), "off".into()));
        env.push(("GOTOOLCHAIN".into(), "local".into()));
        let lister = GoLister {
            go: go.into_os_string(),
            env,
        };
        let main: Vec<String> = lister
            .list_modules(&root, &ws)
            .unwrap()
            .into_iter()
            .filter(|m| m.main)
            .map(|m| m.path)
            .collect();
        assert_eq!(main, vec!["example.com/a", "example.com/b"]);
    }

    #[test]
    fn unreadable_workspaces_are_errors() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                ("go.mod", "module example.com/m\n"),
                ("bad/go.mod", "require x\n"),
            ],
        );
        let root = root_of(&tmp);
        let err = Workspace::load(&root, "go.work", "absent.mod", "go.sum").unwrap_err();
        assert!(err.to_string().contains("absent.mod"), "{err}");
        let err = Workspace::load(&root, "go.work", "bad/go.mod", "go.sum").unwrap_err();
        assert!(
            err.to_string()
                .contains("bad/go.mod:1: usage: require module/path v1.2.3"),
            "{err}"
        );

        write_files(tmp.path(), &[("go.work", "use ../elsewhere\n")]);
        let err = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap_err();
        assert_eq!(
            err.to_string(),
            "go.work: use ../elsewhere: ../elsewhere is outside the project"
        );

        write_files(
            tmp.path(),
            &[("go.work", "use ./nomod\n"), ("nomod/go.mod", "go 1.22\n")],
        );
        let err = Workspace::load(&root, "go.work", "go.mod", "go.sum").unwrap_err();
        assert_eq!(err.to_string(), "nomod/go.mod: no module directive");
    }
}
