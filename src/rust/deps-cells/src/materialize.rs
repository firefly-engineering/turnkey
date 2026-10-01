//! Keeping a deps cell's directory in line with its cell index (ADR 0004)
//!
//! A deps cell (`.turnkey/<cell>`) is a real directory. It holds a store
//! link per package's store path, `_store/<store basename>`, and an alias
//! package per package path (`vendor/anyhow@1.0.100`) and per unversioned
//! name (`vendor/anyhow`): a real `rules.star` whose `alias()` targets
//! forward to a store link's package. A store path may hold several
//! packages, each in its own subdirectory (a Go module's packages, ADR
//! 0008), and alias packages nest (`vendor/cloud.google.com/go` and
//! `vendor/cloud.google.com/go/storage`). A forwarding alias package
//! forwards to a label outside the cell instead (a go.work member's
//! package). A store link is named after its target, so it is only ever
//! created or deleted, never retargeted: everything that changes on a
//! dependency bump is a real file, which buck2's file watcher sees.
//!
//! The shell builds the index with Nix; the materializer never runs Nix to
//! build anything and never reads Starlark: target names come from the
//! index.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::fsutil::{mkdir_all, write_if_changed};
use crate::index::{Package, read_index};
use crate::{BUILD_FILE, Error, GCROOTS_DIR, MARKER_NAME, STORE_DIR, cell_dir};
use gostd::path;
use gostd::strconv::quote;

/// Registers a link as a GC root for a store path. The materializer never
/// runs Nix itself; tk passes one that does.
pub type AddRoot<'a> = &'a mut dyn FnMut(&Path, &str) -> Result<(), String>;

/// Which cell to materialize, from which index
pub struct Options<'a> {
    /// The project root; the cell is `.turnkey/<index's cell>` under it
    pub root: PathBuf,
    /// The cell index's store path
    pub index_path: String,
    /// Registers `link` as a GC root for the index; `None` skips rooting
    pub add_root: Option<AddRoot<'a>>,
}

/// What a materialization changed. A no-op changes nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The cell was a symlink to one store path, now a real directory
    pub switched_over: bool,
    /// Store links created
    pub store_links_added: usize,
    /// Store links removed
    pub store_links_gone: usize,
    /// Alias packages' rules.star written
    pub packages_written: usize,
    /// Alias packages' rules.star removed
    pub packages_gone: usize,
    /// Writes of the cell's .buckconfig
    pub buckconfig_writes: usize,
    /// The index was rooted
    pub rooted: bool,
}

impl Outcome {
    /// Whether the materialization changed anything
    pub fn changed(&self) -> bool {
        self.switched_over
            || self.store_links_added
                + self.store_links_gone
                + self.packages_written
                + self.packages_gone
                + self.buckconfig_writes
                > 0
            || self.rooted
    }
}

/// Brings the cell's directory in line with the index, under a lock. It is
/// idempotent: re-running after a crash repairs a partial materialization.
pub fn materialize(mut opts: Options) -> Result<Outcome, Error> {
    let mut outcome = Outcome::default();
    let index = read_index(Path::new(&opts.index_path))?;
    let turnkey = opts.root.join(".turnkey");
    mkdir_all(&turnkey)?;
    let _lock = lock(&turnkey.join(format!("{}.lock", index.cell)))?;

    let cell = cell_dir(&opts.root, &index.cell);

    // Switching over: the cell was a symlink to one store path
    if std::fs::symlink_metadata(&cell).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(&cell)
            .map_err(|e| Error::new(format!("replacing the {} cell symlink: {e}", index.cell)))?;
        outcome.switched_over = true;
    }
    for dir in [cell.clone(), cell.join(STORE_DIR), cell.join("vendor")] {
        mkdir_all(dir)?;
    }

    if write_if_changed(&cell.join(".buckconfig"), index.buckconfig.as_bytes())? {
        outcome.buckconfig_writes += 1;
    }

    // 1. Store links: added, never retargeted
    let links: BTreeMap<String, &str> = index
        .packages
        .values()
        .map(|pkg| (path::base(&pkg.store), pkg.store.as_str()))
        .collect();
    for (name, store) in &links {
        if ensure_store_link(&cell.join(STORE_DIR).join(name), store)? {
            outcome.store_links_added += 1;
        }
    }

    // 2. Alias packages: rewritten atomically when they change
    let mut packages: BTreeMap<String, Vec<u8>> = index
        .packages
        .iter()
        .map(|(path, pkg)| (path.clone(), alias_file(pkg)))
        .collect();
    for (path, target) in &index.aliases {
        let Some(pkg) = index.packages.get(target) else {
            return Err(Error::new(format!(
                "cell index: alias {path} forwards to {target}, which it doesn't list"
            )));
        };
        packages.insert(path.clone(), alias_file(pkg));
    }
    for (path, label) in &index.forwards {
        if packages.contains_key(path) {
            return Err(Error::new(format!(
                "cell index: {path} is both a package and a forwarding alias"
            )));
        }
        packages.insert(path.clone(), forward_file(label));
    }
    let cell_str = cell.to_string_lossy().into_owned();
    for (path, content) in &packages {
        let dir = PathBuf::from(path::join(&[&cell_str, path]));
        replace_symlink_with_dir(&dir)?;
        if write_if_changed(&dir.join(BUILD_FILE), content)? {
            outcome.packages_written += 1;
        }
    }

    // 3. What the index no longer names
    outcome.store_links_gone = remove_stale_store_links(&cell.join(STORE_DIR), &links)?;
    outcome.packages_gone = remove_stale_packages(&cell_str, &packages)?;

    // 4. The GC root, for the current index
    if let Some(add_root) = opts.add_root.as_mut() {
        let link = turnkey.join(GCROOTS_DIR).join(&index.cell);
        let current = std::fs::read_link(&link);
        if !current.is_ok_and(|current| current.as_os_str() == opts.index_path.as_str()) {
            mkdir_all(turnkey.join(GCROOTS_DIR))?;
            add_root(&link, &opts.index_path)
                .map_err(|e| Error::new(format!("rooting the {} cell index: {e}", index.cell)))?;
            outcome.rooted = true;
        }
    }

    // 5. The deps file's hash, which tk compares with the file on disk
    write_if_changed(
        &cell.join(MARKER_NAME),
        format!("{}\n", index.deps_file_sha256).as_bytes(),
    )?;
    Ok(outcome)
}

/// Whether a materialized cell was built from another version of
/// `deps_file` (relative to `root`) than the one on disk. A cell that isn't
/// materialized (no marker) is not stale: there is nothing to compare.
pub fn stale(root: &Path, cell: &str, deps_file: &str) -> io::Result<bool> {
    let marker = match std::fs::read(cell_dir(root, cell).join(MARKER_NAME)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        marker => marker?,
    };
    let root = root.to_string_lossy();
    let content = match std::fs::read(path::join(&[&root, deps_file])) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        content => content?,
    };
    let sum: String = Sha256::digest(&content)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(String::from_utf8_lossy(&marker).trim() != sum)
}

const GENERATED_HEADER: &str = "# Generated by tk materialize from the cell index. Do not edit.\n";

/// An alias package's rules.star: each target forwards to the same-named
/// target of the package, in its store link
fn alias_file(pkg: &Package) -> Vec<u8> {
    let mut out = String::from(GENERATED_HEADER);
    let dir = path::join(&[STORE_DIR, &path::base(&pkg.store), &pkg.subdir]);
    let mut sorted: Vec<&String> = pkg.targets.iter().collect();
    sorted.sort();
    for t in sorted {
        out.push_str(&format!(
            "alias(name = {}, actual = \"//{dir}:{t}\", visibility = [\"PUBLIC\"])\n",
            quote(t)
        ));
    }
    out.into_bytes()
}

/// A forwarding alias package's rules.star: one target, named as the
/// label's, forwarding to it
fn forward_file(label: &str) -> Vec<u8> {
    let name = &label[label.rfind(':').map_or(0, |i| i + 1)..];
    format!(
        "{GENERATED_HEADER}alias(name = {}, actual = {}, visibility = [\"PUBLIC\"])\n",
        quote(name),
        quote(label)
    )
    .into_bytes()
}

/// Creates the store link if it is missing. One that exists must already
/// point at its name: nothing may retarget a store link. Whether it was
/// created.
fn ensure_store_link(link: &Path, store: &str) -> Result<bool, Error> {
    let info = match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(store, link)?;
            return Ok(true);
        }
        info => info?,
    };
    if !info.file_type().is_symlink() {
        return Err(Error::new(format!(
            "store link {} is not a symlink: remove it and re-run",
            link.display()
        )));
    }
    let target = std::fs::read_link(link)?;
    if target.as_os_str() != store {
        return Err(Error::new(format!(
            "store link {} points at {}, not {store}: store links are never retargeted, remove it and re-run",
            link.display(),
            target.display()
        )));
    }
    Ok(false)
}

/// Makes `dir` a real directory: the old cell layout had symlinks where
/// packages now are
fn replace_symlink_with_dir(dir: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(dir)?;
    }
    mkdir_all(dir)
}

/// Removes the store links the index no longer names; how many
fn remove_stale_store_links(dir: &Path, wanted: &BTreeMap<String, &str>) -> Result<usize, Error> {
    let mut gone = 0;
    for name in read_dir_names(dir)? {
        if wanted.contains_key(name.to_string_lossy().as_ref()) {
            continue;
        }
        remove_all(&dir.join(&name))?;
        gone += 1;
    }
    Ok(gone)
}

/// Removes, under `vendor/`, everything but the wanted packages'
/// rules.star files, and the directories left empty; how many rules.star
/// were removed
fn remove_stale_packages(cell: &str, wanted: &BTreeMap<String, Vec<u8>>) -> Result<usize, Error> {
    let keep: BTreeSet<String> = wanted
        .keys()
        .map(|path| path::join(&[cell, path, BUILD_FILE]))
        .collect();
    let vendor = path::join(&[cell, "vendor"]);
    let mut gone = 0;
    let mut dirs = Vec::new();
    // filepath.WalkDir: in name order, symlinks not followed
    for entry in walkdir::WalkDir::new(&vendor).sort_by_file_name() {
        let entry = entry.map_err(|e| Error::new(e.to_string()))?;
        let path = entry.path().to_string_lossy().into_owned();
        if entry.file_type().is_dir() {
            dirs.push(path);
            continue;
        }
        if keep.contains(&path) {
            continue;
        }
        if entry.file_name() == BUILD_FILE {
            gone += 1;
        }
        std::fs::remove_file(entry.path())?;
    }
    // Deepest first, so a parent is empty once its children are gone
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.len()));
    for dir in dirs {
        if dir == vendor {
            continue;
        }
        if read_dir_names(Path::new(&dir))?.is_empty() {
            std::fs::remove_dir(&dir)?;
        }
    }
    Ok(gone)
}

/// The names in a directory, sorted as `os.ReadDir` sorts them
fn read_dir_names(dir: &Path) -> io::Result<Vec<std::ffi::OsString>> {
    let mut names = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

/// `os.RemoveAll`: the path, and everything under a directory, symlinks
/// removed rather than followed; a missing path is no error
pub(crate) fn remove_all(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// Takes an exclusive lock on `path`, waiting for another materialization
/// of the same cell to finish. Dropping the file releases it.
fn lock(path: &Path) -> Result<File, Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(path)?;
    file.lock()
        .map_err(|e| Error::new(format!("locking {}: {e}", path.display())))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Index, ResolveError, Resolved, current_index};
    use std::time::{Duration, SystemTime};

    /// A project root and a fake store, where each package's "store path"
    /// is a directory
    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        store: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("project");
            let store = dir.path().join("store");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&store).unwrap();
            Fixture {
                _dir: dir,
                root,
                store,
            }
        }

        fn store_path(&self, name: &str) -> String {
            let p = self.store.join(name);
            std::fs::create_dir_all(&p).unwrap();
            p.to_string_lossy().into_owned()
        }

        fn write_index(&self, name: &str, index: &Index) -> String {
            let path = self.store.join(name);
            std::fs::write(&path, serde_json::to_vec(index).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        }

        /// A cell index for anyhow at `version`, and prost-derive
        fn index(&self, version: &str) -> String {
            let anyhow = format!("vendor/anyhow@{version}");
            let index = Index {
                cell: "rustdeps".into(),
                deps_file_sha256: format!("sha-{version}"),
                buckconfig: "[cells]\n    rustdeps = .\n".into(),
                packages: [
                    (
                        anyhow.clone(),
                        package(
                            &self.store_path(&format!("aaa-dep-rust-anyhow-{version}")),
                            "",
                            &["anyhow"],
                        ),
                    ),
                    (
                        "vendor/prost-derive@0.14.4".into(),
                        package(
                            &self
                                .store_path(&format!("bbb-dep-rust-prost-derive-0.14.4-{version}")),
                            "",
                            &["prost-derive"],
                        ),
                    ),
                ]
                .into(),
                aliases: [("vendor/anyhow".to_string(), anyhow)].into(),
                forwards: BTreeMap::new(),
            };
            self.write_index(&format!("index-{version}.json"), &index)
        }

        /// A Go cell index (ADR 0008): cloud.google.com/go at `version`,
        /// with packages at its root and, if `with_civil`, in civil/, and
        /// its nested module cloud.google.com/go/storage; and a forwarding
        /// alias package for example.com/fork/pkg, a go.work member's package
        fn go_index(&self, version: &str, with_civil: bool) -> String {
            let gocloud = self.store_path(&format!("ccc-dep-go-cloud-google-com-go-{version}"));
            let mut packages: BTreeMap<String, Package> = [
                (
                    "vendor/cloud.google.com/go".to_string(),
                    package(&gocloud, "", &["go"]),
                ),
                (
                    "vendor/cloud.google.com/go/storage".to_string(),
                    package(
                        &self.store_path("ddd-dep-go-cloud-google-com-go-storage"),
                        "",
                        &["storage"],
                    ),
                ),
            ]
            .into();
            if with_civil {
                packages.insert(
                    "vendor/cloud.google.com/go/civil".into(),
                    package(&gocloud, "civil", &["civil"]),
                );
            }
            let index = Index {
                cell: "godeps".into(),
                deps_file_sha256: format!("sha-{version}"),
                buckconfig: "[cells]\n    godeps = .\n".into(),
                packages,
                aliases: BTreeMap::new(),
                forwards: [(
                    "vendor/example.com/fork/pkg".to_string(),
                    "root//fork/pkg:pkg".to_string(),
                )]
                .into(),
            };
            self.write_index(&format!("go-index-{version}.json"), &index)
        }

        fn materialize(&self, index: &str) -> Outcome {
            materialize(Options {
                root: self.root.clone(),
                index_path: index.to_string(),
                add_root: None,
            })
            .unwrap()
        }

        fn cell(&self, cell: &str, path: &str) -> PathBuf {
            cell_dir(&self.root, cell).join(path)
        }

        fn read(&self, cell: &str, path: &str) -> String {
            std::fs::read_to_string(self.cell(cell, path)).unwrap()
        }
    }

    fn package(store: &str, subdir: &str, targets: &[&str]) -> Package {
        Package {
            store: store.into(),
            subdir: subdir.into(),
            targets: targets.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn materializes_the_index() {
        let f = Fixture::new();
        let outcome = f.materialize(&f.index("1.0.100"));
        assert_eq!(
            (outcome.store_links_added, outcome.packages_written),
            (2, 3),
            "{outcome:?}"
        );
        let link = f.cell("rustdeps", "_store/aaa-dep-rust-anyhow-1.0.100");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            f.store.join("aaa-dep-rust-anyhow-1.0.100")
        );
        let want = r#"alias(name = "anyhow", actual = "//_store/aaa-dep-rust-anyhow-1.0.100:anyhow", visibility = ["PUBLIC"])"#;
        for pkg in ["vendor/anyhow@1.0.100", "vendor/anyhow"] {
            let got = f.read("rustdeps", &format!("{pkg}/rules.star"));
            assert_eq!(got, format!("{GENERATED_HEADER}{want}\n"), "{pkg}");
        }
        assert_eq!(
            f.read("rustdeps", ".buckconfig"),
            "[cells]\n    rustdeps = .\n"
        );
        assert_eq!(f.read("rustdeps", ".deps-file-sha256"), "sha-1.0.100\n");
    }

    #[test]
    fn a_no_op_materialization_touches_nothing() {
        let f = Fixture::new();
        let index = f.index("1.0.100");
        f.materialize(&index);
        let past = SystemTime::now() - Duration::from_secs(3600);
        let alias = f.cell("rustdeps", "vendor/anyhow@1.0.100/rules.star");
        std::fs::File::options()
            .write(true)
            .open(&alias)
            .unwrap()
            .set_modified(past)
            .unwrap();

        let outcome = f.materialize(&index);
        assert!(
            !outcome.changed(),
            "second materialization changed {outcome:?}"
        );
        assert_eq!(std::fs::metadata(&alias).unwrap().modified().unwrap(), past);
    }

    #[test]
    fn a_bump_swaps_one_store_link() {
        let f = Fixture::new();
        f.materialize(&f.index("1.0.100"));
        let outcome = f.materialize(&f.index("1.0.101"));
        // anyhow's and prost-derive's store paths changed
        assert_eq!(
            (outcome.store_links_added, outcome.store_links_gone),
            (2, 2),
            "{outcome:?}"
        );
        assert!(std::fs::symlink_metadata(f.cell("rustdeps", "vendor/anyhow@1.0.100")).is_err());
        assert!(
            f.read("rustdeps", "vendor/anyhow/rules.star")
                .contains("anyhow-1.0.101:anyhow")
        );
    }

    #[test]
    fn a_store_link_is_never_retargeted() {
        let f = Fixture::new();
        let index = f.index("1.0.100");
        f.materialize(&index);
        let link = f.cell("rustdeps", "_store/aaa-dep-rust-anyhow-1.0.100");
        std::fs::remove_file(&link).unwrap();
        let elsewhere = f.store_path("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();

        let err = materialize(Options {
            root: f.root.clone(),
            index_path: index,
            add_root: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("never retargeted"), "{err}");
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new(&elsewhere));
    }

    #[test]
    fn re_running_repairs_a_partial_materialization() {
        let f = Fixture::new();
        let index = f.index("1.0.100");
        f.materialize(&index);
        // As a crash would leave it: a store link and an alias file
        // missing, and a temporary file behind
        for p in [
            "_store/bbb-dep-rust-prost-derive-0.14.4-1.0.100",
            "vendor/anyhow/rules.star",
        ] {
            std::fs::remove_file(f.cell("rustdeps", p)).unwrap();
        }
        let tmp = f.cell("rustdeps", "vendor/anyhow/.tmp-rules.star-123");
        std::fs::write(&tmp, "").unwrap();

        let outcome = f.materialize(&index);
        assert_eq!(
            (outcome.store_links_added, outcome.packages_written),
            (1, 1),
            "{outcome:?}"
        );
        assert!(!tmp.exists());
        let outcome = f.materialize(&index);
        assert!(
            !outcome.changed(),
            "after the repair, a re-run changed {outcome:?}"
        );
    }

    #[test]
    fn the_first_run_replaces_the_cell_symlink() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.root.join(".turnkey")).unwrap();
        let old = f.store_path("old-rustdeps-cell");
        std::os::unix::fs::symlink(&old, cell_dir(&f.root, "rustdeps")).unwrap();

        let outcome = f.materialize(&f.index("1.0.100"));
        assert!(outcome.switched_over, "{outcome:?}");
        assert!(
            std::fs::symlink_metadata(cell_dir(&f.root, "rustdeps"))
                .unwrap()
                .is_dir()
        );
        assert!(Path::new(&old).exists());
    }

    #[test]
    fn roots_the_index_only_when_it_changes() {
        let f = Fixture::new();
        let mut rooted = Vec::new();
        let mut add_root = |link: &Path, store: &str| -> Result<(), String> {
            rooted.push(store.to_string());
            let _ = std::fs::remove_file(link);
            std::os::unix::fs::symlink(store, link).map_err(|e| e.to_string())
        };
        let index = f.index("1.0.100");
        for _ in 0..2 {
            materialize(Options {
                root: f.root.clone(),
                index_path: index.clone(),
                add_root: Some(&mut add_root),
            })
            .unwrap();
        }
        assert_eq!(rooted.len(), 1);
        assert_eq!(
            std::fs::read_link(f.root.join(".turnkey/gcroots/rustdeps")).unwrap(),
            Path::new(&index)
        );
    }

    #[test]
    fn stale_compares_the_deps_file_with_the_marker() {
        let f = Fixture::new();
        let content = b"schema_version = 2\n";
        std::fs::write(f.root.join("rust-deps.toml"), content).unwrap();
        assert!(
            !stale(&f.root, "rustdeps", "rust-deps.toml").unwrap(),
            "never materialized"
        );

        let index_path = f.index("1.0.100");
        let mut index = read_index(Path::new(&index_path)).unwrap();
        index.deps_file_sha256 = Sha256::digest(content)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        std::fs::write(&index_path, serde_json::to_vec(&index).unwrap()).unwrap();
        f.materialize(&index_path);

        assert!(
            !stale(&f.root, "rustdeps", "rust-deps.toml").unwrap(),
            "matching deps file"
        );
        std::fs::write(f.root.join("rust-deps.toml"), "schema_version = 3\n").unwrap();
        assert!(
            stale(&f.root, "rustdeps", "rust-deps.toml").unwrap(),
            "changed deps file"
        );
        std::fs::remove_file(f.root.join("rust-deps.toml")).unwrap();
        assert!(
            stale(&f.root, "rustdeps", "rust-deps.toml").unwrap(),
            "missing deps file"
        );
    }

    #[test]
    fn resolves_paths_through_the_current_index() {
        let f = Fixture::new();
        let index = f.index("1.0.100");
        let mut add_root = |link: &Path, store: &str| -> Result<(), String> {
            std::os::unix::fs::symlink(store, link).map_err(|e| e.to_string())
        };
        materialize(Options {
            root: f.root.clone(),
            index_path: index,
            add_root: Some(&mut add_root),
        })
        .unwrap();
        let current = current_index(&f.root, "rustdeps").unwrap();
        let store = f
            .store
            .join("aaa-dep-rust-anyhow-1.0.100")
            .to_string_lossy()
            .into_owned();
        for path in [
            "vendor/anyhow@1.0.100/src/lib.rs",
            "vendor/anyhow/src/lib.rs",
        ] {
            assert_eq!(
                current.resolve(path),
                Ok(Resolved {
                    root: "vendor/anyhow@1.0.100".into(),
                    store: store.clone(),
                    rest: "src/lib.rs".into(),
                }),
                "{path}"
            );
        }
        assert!(current.resolve("vendor/serde/src/lib.rs").is_err());
        assert!(current_index(&f.root, "godeps").is_err());
    }

    #[test]
    fn materializes_nested_and_forwarding_alias_packages() {
        let f = Fixture::new();
        let outcome = f.materialize(&f.go_index("v0.1.0", true));
        assert_eq!(
            (outcome.store_links_added, outcome.packages_written),
            (2, 4),
            "{outcome:?}"
        );
        for (path, want) in [
            (
                "vendor/cloud.google.com/go/rules.star",
                r#"alias(name = "go", actual = "//_store/ccc-dep-go-cloud-google-com-go-v0.1.0:go", visibility = ["PUBLIC"])"#,
            ),
            (
                "vendor/cloud.google.com/go/civil/rules.star",
                r#"alias(name = "civil", actual = "//_store/ccc-dep-go-cloud-google-com-go-v0.1.0/civil:civil", visibility = ["PUBLIC"])"#,
            ),
            (
                "vendor/cloud.google.com/go/storage/rules.star",
                r#"alias(name = "storage", actual = "//_store/ddd-dep-go-cloud-google-com-go-storage:storage", visibility = ["PUBLIC"])"#,
            ),
            (
                "vendor/example.com/fork/pkg/rules.star",
                r#"alias(name = "pkg", actual = "root//fork/pkg:pkg", visibility = ["PUBLIC"])"#,
            ),
        ] {
            assert_eq!(
                f.read("godeps", path),
                format!("{GENERATED_HEADER}{want}\n"),
                "{path}"
            );
        }

        // A bump that drops civil/ removes its alias package, and keeps the
        // packages it nests in and beside
        let outcome = f.materialize(&f.go_index("v0.2.0", false));
        assert_eq!(
            (
                outcome.packages_gone,
                outcome.store_links_added,
                outcome.store_links_gone
            ),
            (1, 1, 1),
            "{outcome:?}"
        );
        assert!(
            !f.cell("godeps", "vendor/cloud.google.com/go/civil")
                .exists()
        );
        for kept in [
            "vendor/cloud.google.com/go/rules.star",
            "vendor/cloud.google.com/go/storage/rules.star",
        ] {
            assert!(f.cell("godeps", kept).exists(), "{kept}");
        }
    }

    #[test]
    fn resolves_a_modules_files_and_refuses_first_party_ones() {
        let f = Fixture::new();
        let index = read_index(Path::new(&f.go_index("v0.1.0", true))).unwrap();
        let gocloud = f
            .store
            .join("ccc-dep-go-cloud-google-com-go-v0.1.0")
            .to_string_lossy()
            .into_owned();
        let storage = f
            .store
            .join("ddd-dep-go-cloud-google-com-go-storage")
            .to_string_lossy()
            .into_owned();
        for (path, root, store, rest) in [
            (
                "vendor/cloud.google.com/go/civil/civil.go",
                "vendor/cloud.google.com/go",
                &gocloud,
                "civil/civil.go",
            ),
            (
                "vendor/cloud.google.com/go/internal/doc.md",
                "vendor/cloud.google.com/go",
                &gocloud,
                "internal/doc.md",
            ),
            (
                "vendor/cloud.google.com/go/storage/storage.go",
                "vendor/cloud.google.com/go/storage",
                &storage,
                "storage.go",
            ),
            // Cleaned first
            (
                "./vendor//cloud.google.com/go/x/../storage/storage.go",
                "vendor/cloud.google.com/go/storage",
                &storage,
                "storage.go",
            ),
        ] {
            assert_eq!(
                index.resolve(path),
                Ok(Resolved {
                    root: root.into(),
                    store: store.clone(),
                    rest: rest.into()
                }),
                "{path}"
            );
        }
        assert_eq!(
            index.resolve("vendor/example.com/fork/pkg/pkg.go"),
            Err(ResolveError::FirstParty {
                package: "vendor/example.com/fork/pkg".into(),
                label: "root//fork/pkg:pkg".into(),
            })
        );
    }

    #[test]
    fn index_errors() {
        let f = Fixture::new();
        for (name, content, want) in [
            ("no-cell.json", r#"{"packages": {}}"#, "names no cell"),
            ("bad.json", "not json", "parsing cell index"),
        ] {
            let path = f.store.join(name);
            std::fs::write(&path, content).unwrap();
            let err = read_index(&path).unwrap_err();
            assert!(err.to_string().contains(want), "{name}: {err}");
        }
        // null reads as nothing, as encoding/json reads it
        let path = f.store.join("nulls.json");
        std::fs::write(
            &path,
            r#"{"cell": "c", "packages": null, "aliases": {"a": "b"}}"#,
        )
        .unwrap();
        let index = read_index(&path).unwrap();
        assert!(index.packages.is_empty());

        let err = materialize(Options {
            root: f.root.clone(),
            index_path: path.to_string_lossy().into_owned(),
            add_root: None,
        })
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("alias a forwards to b, which it doesn't list"),
            "{err}"
        );
    }
}
