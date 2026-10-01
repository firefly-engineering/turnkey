//! The Go rulessync's tests, ported: those that need neither Go's build
//! constraints (goparse) nor reading sync.toml (syncconfig)

use super::*;
use crate::mapper::{Dimensions, MappedDep, PackageMapping, Request, Rule, TargetKind};
use conditions::{OS, Platform};
use project_sync::config::{ConditionsConfig, Language as LanguageConfig};
use std::cell::RefCell;
use std::rc::Rc;

/// turnkey's default platforms
fn default_platforms() -> Vec<Platform> {
    [
        ("linux", "x86_64"),
        ("linux", "arm64"),
        ("macos", "x86_64"),
        ("macos", "arm64"),
    ]
    .into_iter()
    .map(|(os, cpu)| Platform {
        os: os.into(),
        cpu: cpu.into(),
    })
    .collect()
}

/// The sync configuration of a project with every language, as turnkey's
/// shell lists them (testdata/sync.toml), built for
/// `platforms`
fn test_sync(platforms: Vec<Platform>) -> sync_config::Config {
    sync_config::Config {
        languages: [
            ("go", "godeps", "go-deps.toml"),
            ("rust", "rustdeps", "rust-deps.toml"),
            ("python", "pydeps", "python-deps.toml"),
            ("javascript", "jsdeps", "js-deps.toml"),
            ("solidity", "soldeps", "solidity-deps.toml"),
        ]
        .into_iter()
        .map(|(name, cell, deps_file)| LanguageConfig {
            name: name.into(),
            cell: cell.into(),
            deps_file: deps_file.into(),
        })
        .collect(),
        conditions: ConditionsConfig {
            platforms,
            ..ConditionsConfig::default()
        },
        ..sync_config::Config::default()
    }
}

/// A temporary directory, and its path as a string
fn temp_dir() -> (tempfile::TempDir, String) {
    // Not hidden: a walk skips a directory whose name starts with "."
    let dir = tempfile::Builder::new()
        .prefix("rules-sync-test")
        .tempdir()
        .unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    (dir, path)
}

/// Writes files (relative path, content) under `root`.
fn write_files(root: &str, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = std::path::Path::new(root).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// A syncer of the project at `root`, forced, with the configured plug-ins
fn syncer(root: &str, platforms: Vec<Platform>, dry_run: bool) -> Syncer {
    Syncer::new(Config {
        project_root: root.to_string(),
        force: true,
        dry_run,
        sync: Some(test_sync(platforms)),
        cwd: "/".to_string(),
        ..Config::default()
    })
    .unwrap()
}

/// Without languages, there is nothing to sync with: that is an error, not
/// a sync that passes every check.
#[test]
fn new_syncer_without_languages() {
    let (_dir, root) = temp_dir();
    let err = Syncer::new(Config {
        project_root: root,
        ..Config::default()
    })
    .err()
    .expect("an error");
    assert_eq!(
        err.to_string(),
        ".turnkey/sync.toml lists no [[languages]]: re-enter the turnkey shell to regenerate it"
    );
}

/// Applies one set of mapped deps to a target, with no configurations.
fn apply_deps(
    result: &mut SyncResult,
    target: &mut Target,
    attr: &str,
    mapped: &[&str],
    unmapped: &[&str],
    unsynced: &[&str],
) -> bool {
    let space = Space::new(&[], "");
    let old = conditional::read_labels(target, attr, &space).unwrap();
    apply_conditional(result, target, attr, &space, &old, |_| {
        Ok(Want {
            labels: strings(mapped),
            unmapped: strings(unmapped),
            unsynced: strings(unsynced),
            canonical: None,
        })
    })
    .unwrap()
}

/// The only target of a rules.star source
fn parse_target(src: &str) -> Target {
    let mut f = rules_star::parse("rules.star", src.into()).unwrap();
    assert_eq!(f.targets.len(), 1);
    f.targets.remove(0)
}

const LIB_WITH_X: &str = r#"go_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//pkg/x:x",
        # turnkey:auto-end
    ],
)
"#;

/// With unmapped imports the mapped deps are incomplete: an existing dep
/// the mapper didn't return is kept, and a newly mapped dep is still added.
#[test]
fn apply_deps_keeps_deps_when_unmapped() {
    let mut target = parse_target(LIB_WITH_X);
    let mut result = SyncResult::default();
    assert!(apply_deps(
        &mut result,
        &mut target,
        "deps",
        &["//pkg/y:y"],
        &["example.com/unknown"],
        &[]
    ));
    assert_eq!(target.get_deps(), ["//pkg/x:x", "//pkg/y:y"]);
    assert_eq!(
        result.changes,
        [TargetChange {
            target: "lib".into(),
            added: strings(&["//pkg/y:y"]),
            kept: strings(&["//pkg/x:x"]),
            unmapped: strings(&["example.com/unknown"]),
            ..TargetChange::default()
        }]
    );
}

/// A kept dep is reported even when nothing else about the target changes.
#[test]
fn apply_deps_reports_kept_without_change() {
    let mut target = parse_target(LIB_WITH_X);
    let mut result = SyncResult::default();
    assert!(!apply_deps(
        &mut result,
        &mut target,
        "deps",
        &[],
        &["example.com/unknown"],
        &[]
    ));
    assert_eq!(target.get_deps(), ["//pkg/x:x"]);
    assert_eq!(result.changes.len(), 1);
    assert_eq!(result.changes[0].kept, ["//pkg/x:x"]);
}

/// Without unmapped imports the mapped deps are complete, so a dep the
/// mapper didn't return is removed.
#[test]
fn apply_deps_removes_when_all_mapped() {
    let mut target = parse_target(LIB_WITH_X);
    let mut result = SyncResult::default();
    assert!(apply_deps(
        &mut result,
        &mut target,
        "deps",
        &["//pkg/y:y"],
        &[],
        &[]
    ));
    assert_eq!(target.get_deps(), ["//pkg/y:y"]);
    assert_eq!(
        result.changes,
        [TargetChange {
            target: "lib".into(),
            added: strings(&["//pkg/y:y"]),
            removed: strings(&["//pkg/x:x"]),
            ..TargetChange::default()
        }]
    );
}

/// A target already holding exactly the mapped deps is left as written,
/// whatever their order.
#[test]
fn apply_deps_ignores_order() {
    let mut target = parse_target(
        r#"rust_library(
    name = "lib",
    deps = [
        "rustdeps//vendor/tree-sitter:tree-sitter",
        "rustdeps//vendor/tree-sitter-starlark:tree-sitter-starlark",
    ],
)
"#,
    );
    let mut result = SyncResult::default();
    let mapped = [
        "rustdeps//vendor/tree-sitter-starlark:tree-sitter-starlark",
        "rustdeps//vendor/tree-sitter:tree-sitter",
    ];
    assert!(!apply_deps(
        &mut result,
        &mut target,
        "deps",
        &mapped,
        &[],
        &[]
    ));
    assert!(result.changes.is_empty(), "{:?}", result.changes);
}

/// An existing dep in the package of an unsynced dep is neither removed
/// nor reported as kept, and an unsynced dep is never added.
#[test]
fn apply_deps_leaves_unsynced_deps() {
    let mut target = parse_target(
        r#"rust_binary(
    name = "bin",
    deps = [
        "//src/rust/composition:composition-full",
        "rustdeps//vendor/log:log",
    ],
)
"#,
    );
    let mut result = SyncResult::default();
    assert!(!apply_deps(
        &mut result,
        &mut target,
        "deps",
        &["rustdeps//vendor/log:log"],
        &[],
        &["//src/rust/composition:composition"]
    ));
    assert!(result.changes.is_empty(), "{:?}", result.changes);

    let mut target = parse_target("rust_binary(\n    name = \"bin\",\n    deps = [],\n)\n");
    let mut result = SyncResult::default();
    apply_deps(
        &mut result,
        &mut target,
        "deps",
        &[],
        &[],
        &["//src/rust/composition:composition"],
    );
    assert!(target.get_deps().is_empty(), "unsynced dep added");
}

/// A dep in a turnkey:preserve section is never removed, reported or
/// copied into the auto-managed section.
#[test]
fn apply_deps_honours_preserve_section() {
    let src = r#"rust_binary(
    name = "bin",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
        # turnkey:preserve-start
        "//native:lib",
        # turnkey:preserve-end
    ],
)
"#;
    let mut target = parse_target(src);
    let mut result = SyncResult::default();
    assert!(apply_deps(
        &mut result,
        &mut target,
        "deps",
        &["//b:b"],
        &[],
        &[]
    ));
    assert_eq!(target.get_auto_deps(), ["//b:b"]);
    assert_eq!(target.get_preserved_deps(), ["//native:lib"]);
    assert_eq!(
        result.changes,
        [TargetChange {
            target: "bin".into(),
            added: strings(&["//b:b"]),
            removed: strings(&["//a:a"]),
            ..TargetChange::default()
        }]
    );

    // Mapped deps that match: nothing to do.
    let mut target = parse_target(src);
    let mut result = SyncResult::default();
    assert!(!apply_deps(
        &mut result,
        &mut target,
        "deps",
        &["//a:a"],
        &[],
        &[]
    ));
    assert!(result.changes.is_empty(), "{:?}", result.changes);
}

/// A language whose deps go in another attribute (TypeScript's npm_deps)
/// syncs that attribute and leaves deps alone.
#[test]
fn apply_deps_to_other_attribute() {
    let mut target = parse_target(
        r#"typescript_binary(
    name = "app",
    deps = ["//lib:lib"],
    npm_deps = ["jsdeps//:left-pad"],
)
"#,
    );
    let mut result = SyncResult::default();
    assert!(apply_deps(
        &mut result,
        &mut target,
        "npm_deps",
        &["jsdeps//:lodash"],
        &[],
        &[]
    ));
    assert_eq!(target.get_labels("npm_deps"), ["jsdeps//:lodash"]);
    assert_eq!(target.get_deps(), ["//lib:lib"]);
}

/// In one file, an ordinary target is synced, a target opted out with
/// "# turnkey:no-sync" is left alone and reported as opted out, and one
/// whose deps sync can't read is reported as unreadable.
#[test]
fn sync_file_opt_out_and_unreadable() {
    let (_dir, root) = temp_dir();
    write_files(
        &root,
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nanyhow = \"1\"\n",
            ),
            (
                "rust-deps.toml",
                "[deps.\"anyhow@1.0.0\"]\nname = \"anyhow\"\n",
            ),
            (
                "crates/lib/Cargo.toml",
                "[package]\nname = \"lib\"\n\n[dependencies]\nanyhow.workspace = true\n",
            ),
            (
                "crates/lib/rules.star",
                r#"load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "lib",
    deps = [],
)

# turnkey:no-sync
rust_library(
    name = "lib-custom",
    deps = ["//somewhere:else"],
)

_DEPS = ["rustdeps//vendor/anyhow:anyhow"]

rust_test(
    name = "lib-test",
    deps = _DEPS,
)
"#,
            ),
        ],
    );
    let s = syncer(&root, vec![], true);
    let result = s
        .sync_file(&format!("{root}/crates/lib/rules.star"))
        .unwrap();
    assert_eq!(
        result.changes,
        [TargetChange {
            target: "lib".into(),
            added: strings(&["rustdeps//vendor/anyhow:anyhow"]),
            ..TargetChange::default()
        }]
    );
    assert_eq!(result.opted_out, ["lib-custom"]);
    assert_eq!(
        result.unreadable,
        [UnreadableTarget {
            target: "lib-test".into(),
            attribute: "deps".into(),
        }]
    );
}

// --- conditional_test.go

/// A plug-in whose deps depend on the OS: every platform needs //unix:unix
/// and Linux also //linux:only. A target's "features" variant adds
/// //feature/<name>:<name> per feature.
#[derive(Default)]
struct FakeLanguage {
    /// What was resolved
    requests: RefCell<Vec<Request>>,
    /// Its rule's canonical
    canonical: Option<fn(&str) -> String>,
}

impl Language for FakeLanguage {
    fn name(&self) -> &str {
        "fake"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        (rule == "fake_library").then_some(Rule {
            kind: TargetKind::Library,
            deps_attribute: "deps",
            variant: &["features"],
            canonical: self.canonical,
        })
    }

    fn source_patterns(&self) -> &'static [&'static str] {
        &["*.fake"]
    }

    fn dimensions(&self, _: &str) -> Result<Dimensions> {
        Ok(Dimensions {
            platform: vec![OS.to_string()],
            on_off: vec![],
        })
    }

    fn resolve_deps(&self, _: &str, req: &Request) -> Result<PackageMapping> {
        self.requests.borrow_mut().push(req.clone());
        let dep = |target: String| MappedDep {
            target,
            ..MappedDep::default()
        };
        let mut deps = vec![dep("//unix:unix".into())];
        if req.config.get(OS) == "linux" {
            deps.push(dep("//linux:only".into()));
        }
        let features = rules_star::labels(req.variant.get("features")).unwrap_or_default();
        for f in features {
            deps.push(dep(format!("//feature/{f}:{f}")));
        }
        Ok(PackageMapping {
            deps,
            ..PackageMapping::default()
        })
    }
}

/// A plug-in shared with the mapper, to look at what it resolved
struct Shared(Rc<FakeLanguage>);

impl Language for Shared {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn rule(&self, rule: &str) -> Option<Rule> {
        self.0.rule(rule)
    }
    fn source_patterns(&self) -> &'static [&'static str] {
        self.0.source_patterns()
    }
    fn dimensions(&self, dir: &str) -> Result<Dimensions> {
        self.0.dimensions(dir)
    }
    fn resolve_deps(&self, dir: &str, req: &Request) -> Result<PackageMapping> {
        self.0.resolve_deps(dir, req)
    }
}

/// Syncs rules.star content with the fake language: the result, and what
/// sync wrote.
fn sync_fake(
    lang: &Rc<FakeLanguage>,
    platforms: Vec<Platform>,
    content: &str,
) -> (SyncResult, String) {
    let (_dir, root) = temp_dir();
    write_files(&root, &[("pkg/rules.star", content)]);
    let s = Syncer::with_mapper(
        Config {
            project_root: root.clone(),
            force: true,
            sync: Some(test_sync(platforms)),
            cwd: "/".to_string(),
            ..Config::default()
        },
        Mapper::with(vec![Box::new(Shared(lang.clone()))]),
    )
    .unwrap();
    let rules_path = format!("{root}/pkg/rules.star");
    let result = s.sync_file(&rules_path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    let out = std::fs::read_to_string(&rules_path).unwrap();
    (result, out)
}

const FAKE_LIB: &str = r#"fake_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//old:dep",
        # turnkey:auto-end
    ],
)
"#;

/// Deps that differ by OS are written as [<common>] + select({...}) keyed
/// on the OS, with a branch for every OS and no DEFAULT; syncing again
/// changes nothing, and the package is resolved once per OS.
#[test]
fn sync_file_writes_platform_conditional_deps() {
    let lang = Rc::new(FakeLanguage::default());
    let (result, out) = sync_fake(&lang, default_platforms(), FAKE_LIB);
    assert_eq!(
        out,
        r#"fake_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//unix:unix",
        # turnkey:auto-end
    ] + select({
        "config//os:linux": ["//linux:only"],
        "config//os:macos": [],
    }),
)
"#
    );
    assert!(result.updated);
    assert_eq!(lang.requests.borrow().len(), 2, "resolved once per OS");

    let (result, again) = sync_fake(&Rc::default(), default_platforms(), &out);
    assert!(!result.updated && again == out && result.changes.is_empty());
}

/// What sync writes doesn't depend on the host: nothing it does looks the
/// host up, so syncing the same file again and again gives the same file.
#[test]
fn sync_file_is_deterministic() {
    let outputs: Vec<String> = (0..4)
        .map(|_| sync_fake(&Rc::default(), default_platforms(), FAKE_LIB).1)
        .collect();
    for out in &outputs[1..] {
        assert_eq!(out, &outputs[0]);
    }
}

/// Without platforms there's a single configuration: a language's
/// conditions can't show, and the deps are a plain list.
#[test]
fn sync_file_without_platforms() {
    let (_, out) = sync_fake(&Rc::default(), vec![], FAKE_LIB);
    assert!(
        out.contains("\"//unix:unix\",\n        # turnkey:auto-end\n    ],\n)"),
        "{out}"
    );
}

/// A variant attribute is read in each configuration, a select() included,
/// and passed to the language.
#[test]
fn sync_file_passes_variants() {
    let lang = Rc::new(FakeLanguage::default());
    let (_, out) = sync_fake(
        &lang,
        default_platforms(),
        r#"fake_library(
    name = "lib",
    features = ["a"] + select({
        "config//os:linux": [],
        "config//os:macos": ["b"],
    }),
    deps = [],
)
"#,
    );
    assert_eq!(
        out,
        r#"fake_library(
    name = "lib",
    features = ["a"] + select({
        "config//os:linux": [],
        "config//os:macos": ["b"],
    }),
    deps = [
        "//unix:unix",
        "//feature/a:a",
    ] + select({
        "config//os:linux": ["//linux:only"],
        "config//os:macos": ["//feature/b:b"],
    }),
)
"#
    );
    let mut variants: Vec<String> = lang
        .requests
        .borrow()
        .iter()
        .filter_map(|req| {
            req.variant
                .get("features")
                .map(|v| format!("{}:{}", req.config.get(OS), rules_star::render(v)))
        })
        .collect();
    variants.sort();
    assert_eq!(
        variants,
        ["linux:[\"a\"]", "macos:[\n    \"a\",\n    \"b\",\n]"]
    );
}

/// An existing dep the rule says stands for a wanted label (as a Rust dep
/// pinning a crate's version stands for the crate's target) is kept in its
/// place, and one that stands for no wanted label is removed.
#[test]
fn sync_file_keeps_equivalent_label() {
    let lang = Rc::new(FakeLanguage {
        canonical: Some(|label| {
            let (pkg, name) = label.split_once(':').unwrap_or((label, ""));
            let pkg = pkg.split_once('@').map_or(pkg, |p| p.0);
            format!("{pkg}:{name}")
        }),
        ..FakeLanguage::default()
    });
    let (result, out) = sync_fake(
        &lang,
        vec![],
        r#"fake_library(
    name = "lib",
    deps = [
        "//unix@2:unix",
        "//gone@1:gone",
    ],
)
"#,
    );
    assert_eq!(
        out,
        "fake_library(\n    name = \"lib\",\n    deps = [\"//unix@2:unix\"],\n)\n"
    );
    assert_eq!(
        result.changes,
        [TargetChange {
            target: "lib".into(),
            removed: strings(&["//gone@1:gone"]),
            ..TargetChange::default()
        }]
    );

    // Without the rule's say, the pinned dep is replaced
    let (_, out) = sync_fake(
        &Rc::default(),
        vec![],
        "fake_library(\n    name = \"lib\",\n    deps = [\"//unix@2:unix\"],\n)\n",
    );
    assert!(
        out.contains("\"//unix:unix\"") && !out.contains("@2"),
        "{out}"
    );
}

/// A variant attribute whose select() has a key sync doesn't know makes
/// the target unreadable.
#[test]
fn sync_file_unreadable_variant() {
    let (result, _) = sync_fake(
        &Rc::default(),
        default_platforms(),
        r#"fake_library(
    name = "lib",
    features = select({"//my:setting": ["a"]}),
    deps = [],
)
"#,
    );
    assert_eq!(
        result.unreadable,
        [UnreadableTarget {
            target: "lib".into(),
            attribute: "features".into(),
        }]
    );
}

/// A crate's cfg(target_os = "linux") and cfg(unix) deps: the unix one is
/// common, the Linux one is keyed on the OS, and there is no DEFAULT.
#[test]
fn sync_file_rust_target_specific_deps() {
    let (_dir, root) = temp_dir();
    write_files(
        &root,
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "rust-deps.toml",
                "[deps.\"libc@0.2.0\"]\nname = \"libc\"\n[deps.\"inotify@0.11.0\"]\nname = \"inotify\"\n",
            ),
            (
                "crates/watch/Cargo.toml",
                r#"[package]
name = "watch"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(target_os = "linux")'.dependencies]
inotify = "0.11"
"#,
            ),
            (
                "crates/watch/rules.star",
                "rust_library(\n    name = \"watch\",\n    deps = [],\n)\n",
            ),
        ],
    );
    let s = syncer(&root, default_platforms(), false);
    let rules_path = format!("{root}/crates/watch/rules.star");
    let result = s.sync_file(&rules_path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    assert_eq!(
        std::fs::read_to_string(&rules_path).unwrap(),
        r#"rust_library(
    name = "watch",
    deps = ["rustdeps//vendor/libc:libc"] + select({
        "config//os:linux": ["rustdeps//vendor/inotify:inotify"],
        "config//os:macos": [],
    }),
)
"#
    );
}

/// A Rust variant target with a select()'d cargo_features gets the features
/// and deps its request expands to, per OS; the primary target gets the
/// defaults' features.
#[test]
fn sync_file_rust_variant_target() {
    let (_dir, root) = temp_dir();
    write_files(
        &root,
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "rust-deps.toml",
                r#"[deps."libc@0.2.0"]
name = "libc"
[deps."fuser@0.15.0"]
name = "fuser"
[deps."notify@8.0.0"]
name = "notify"
[deps."log@0.4.0"]
name = "log"
"#,
            ),
            (
                "crates/comp/Cargo.toml",
                r#"[package]
name = "comp"

[features]
default = ["std"]
std = []
fuse = ["dep:fuser", "dep:libc"]
fuse-t = ["dep:libc"]
watcher = ["dep:notify"]

[dependencies]
log = "0.4"
fuser = { version = "0.15", optional = true }
libc = { version = "0.2", optional = true }
notify = { version = "8", optional = true }
"#,
            ),
            (
                "crates/comp/rules.star",
                r#"rust_library(
    name = "comp",
    deps = ["rustdeps//vendor/log:log"],
)

rust_library(
    name = "comp-full",
    cargo_features = ["watcher"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
    deps = [],
)
"#,
            ),
        ],
    );
    let s = syncer(&root, default_platforms(), false);
    let rules_path = format!("{root}/crates/comp/rules.star");
    let result = s.sync_file(&rules_path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    assert_eq!(
        std::fs::read_to_string(&rules_path).unwrap(),
        r#"rust_library(
    name = "comp",
    deps = ["rustdeps//vendor/log:log"],
    features = ["std"],
)

rust_library(
    name = "comp-full",
    cargo_features = ["watcher"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
    deps = [
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/notify:notify",
    ] + select({
        "config//os:linux": ["rustdeps//vendor/fuser:fuser"],
        "config//os:macos": [],
    }),
    features = [
        "std",
        "watcher",
    ] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
)
"#
    );
}

// --- python_conditional_test.go

/// The PATH the test runs with: python3 and deps-extract are found on it
fn path_launcher() -> Launcher {
    Launcher::new(std::env::var_os("PATH"))
}

/// Whether a tool is on the PATH
fn on_path(tool: &str) -> bool {
    path_launcher().look_path(OsStr::new(tool)).is_ok()
}

/// A uv workspace member's platform marker is a select() on the OS, a
/// marker the Python toolchain doesn't satisfy drops the dep, and a target
/// built with an extra gets the extra's deps.
#[test]
fn sync_file_python_markers_and_extras() {
    for tool in ["deps-extract", "python3"] {
        if !on_path(tool) {
            eprintln!("skipped: {tool} not in PATH");
            return;
        }
    }
    let new_enough = path_launcher()
        .command(
            OsStr::new("python3"),
            &["-c", "import sys; print(sys.version_info >= (3, 10))"],
            None,
        )
        .unwrap()
        .output()
        .is_ok_and(|out| out.stdout == b"True\n");
    if !new_enough {
        eprintln!("skipped: needs a Python >= 3.10 toolchain");
        return;
    }

    let (_dir, root) = temp_dir();
    write_files(
        &root,
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"root\"\n\n[tool.uv.workspace]\nmembers = [\"src/app\"]\n",
            ),
            (
                "python-deps.toml",
                "schema_version = 2\n\n[deps.requests]\n[deps.oldlib]\n[deps.extradep]\n",
            ),
            (
                "src/app/pyproject.toml",
                r#"[project]
name = "app"
dependencies = [
    "requests ; sys_platform == \"linux\"",
    "oldlib ; python_version < \"3.10\"",
]

[project.optional-dependencies]
x = ["extradep"]
"#,
            ),
            (
                "src/app/app/__init__.py",
                "import requests\nimport oldlib\n",
            ),
            (
                "src/app/rules.star",
                r#"python_library(
    name = "app",
    deps = [],
)

python_library(
    name = "app-x",
    extras = ["x"],
    deps = [],
)
"#,
            ),
        ],
    );
    let s = Syncer::new(Config {
        project_root: root.clone(),
        force: true,
        sync: Some(test_sync(default_platforms())),
        launcher: path_launcher(),
        cwd: "/".to_string(),
        ..Config::default()
    })
    .unwrap();
    let rules_path = format!("{root}/src/app/rules.star");
    let result = s.sync_file(&rules_path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    assert_eq!(
        std::fs::read_to_string(&rules_path).unwrap(),
        r#"python_library(
    name = "app",
    deps = select({
        "config//os:linux": ["pydeps//vendor/requests:requests"],
        "config//os:macos": [],
    }),
)

python_library(
    name = "app-x",
    extras = ["x"],
    deps = ["pydeps//vendor/extradep:extradep"] + select({
        "config//os:linux": ["pydeps//vendor/requests:requests"],
        "config//os:macos": [],
    }),
)
"#
    );
}

/// The walk visits rules.star files in lexical order, skipping vendor,
/// testdata and hidden directories
#[test]
fn sync_directory_walks_in_order() {
    let (_dir, root) = temp_dir();
    let rules = "fake_library(name = \"lib\")\n";
    write_files(
        &root,
        &[
            ("b/rules.star", rules),
            ("a/rules.star", rules),
            ("a/z/rules.star", rules),
            ("vendor/x/rules.star", rules),
            ("testdata/rules.star", rules),
            (".hidden/rules.star", rules),
            ("c/notes.txt", ""),
        ],
    );
    let lang = Rc::new(FakeLanguage::default());
    let s = Syncer::with_mapper(
        Config {
            project_root: root.clone(),
            force: true,
            dry_run: true,
            cwd: "/".to_string(),
            ..Config::default()
        },
        Mapper::with(vec![Box::new(Shared(lang))]),
    )
    .unwrap();
    let paths: Vec<String> = s
        .sync_directory(&root)
        .unwrap()
        .into_iter()
        .map(|r| r.path[root.len()..].to_string())
        .collect();
    assert_eq!(paths, ["/a/rules.star", "/a/z/rules.star", "/b/rules.star"]);
}

// --- syncer_test.go and golang_conditional_test.go: the sync.toml turnkey
// writes, and the Go plug-in

/// .turnkey/sync.toml as turnkey's shell writes it for a project with
/// every language (testdata/sync.toml, which checks.sync-config-contract
/// checks against nix/buck2/sync-config.nix)
const CONTRACT_SYNC_TOML: &str = include_str!("../../testdata/sync.toml");

/// Rules sync reads the sync.toml turnkey writes: every language gets its
/// plug-in, with the cell and deps file its record gives, and the Go
/// plug-in the allowed build tags.
#[test]
fn syncer_reads_what_the_shell_writes() {
    let cfg = sync_config::Config::parse(CONTRACT_SYNC_TOML.as_bytes()).unwrap();
    cfg.validate()
        .expect("the sync.toml turnkey writes validates");
    let (_dir, root) = temp_dir();
    let s = Syncer::new(Config {
        project_root: root,
        sync: Some(cfg.clone()),
        cwd: "/".to_string(),
        ..Config::default()
    })
    .unwrap();
    assert_eq!(s.languages().len(), cfg.languages.len());
    assert!(
        !cfg.conditions.go_tags.is_empty() && !s.space().configurations.is_empty(),
        "the contract has no Go tags or platforms to check: {:?}",
        cfg.conditions
    );
}

/// A project's .turnkey/sync.toml is read when no settings are given, and
/// one that doesn't validate is an error
#[test]
fn syncer_reads_the_project_sync_toml() {
    let (_dir, root) = temp_dir();
    write_files(&root, &[(".turnkey/sync.toml", CONTRACT_SYNC_TOML)]);
    let s = Syncer::new(Config {
        project_root: root.clone(),
        cwd: "/".to_string(),
        ..Config::default()
    })
    .unwrap();
    assert_eq!(s.languages().len(), 5);

    write_files(
        &root,
        &[(
            ".turnkey/sync.toml",
            "[[deps]]\nname = \"go\"\ntarget = \"go-deps.toml\"\n",
        )],
    );
    let err = Syncer::new(Config {
        project_root: root,
        cwd: "/".to_string(),
        ..Config::default()
    })
    .err()
    .expect("an error");
    assert!(
        err.to_string().starts_with("invalid sync config: "),
        "{err}"
    );
}

/// The PATH the test runs with, for go and git; `None` when `tool` isn't
/// on it, and the test is skipped
fn launcher_with(tool: &str) -> Option<Launcher> {
    let launcher = path_launcher();
    if launcher.look_path(OsStr::new(tool)).is_err() {
        eprintln!("skipped: {tool} not in PATH");
        return None;
    }
    Some(launcher)
}

/// A syncer of the project at `root` with every language, forced, running
/// the tools `launcher` finds
fn go_syncer(root: &str, launcher: Launcher, dry_run: bool) -> Syncer {
    Syncer::new(Config {
        project_root: root.to_string(),
        force: true,
        dry_run,
        sync: Some(test_sync(vec![])),
        launcher,
        cwd: "/".to_string(),
        ..Config::default()
    })
    .unwrap()
}

/// A go_binary's deps follow its imports, as a go_library's do: the
/// internal package it imports is added and one it no longer imports is
/// removed.
#[test]
fn sync_file_go_binary() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let (_dir, root) = temp_dir();
    write_files(
        &root,
        &[
            ("go.mod", "module example.com/project\n\ngo 1.22\n"),
            (
                "pkg/greet/greet.go",
                "package greet\n\nfunc Hello() string { return \"hello\" }\n",
            ),
            ("pkg/old/old.go", "package old\n"),
            (
                "cmd/hello/main.go",
                r#"package main

import (
	"fmt"

	"example.com/project/pkg/greet"
)

func main() { fmt.Println(greet.Hello()) }
"#,
            ),
            (
                "cmd/hello/rules.star",
                r#"load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "hello",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        "//pkg/old:old",
        # turnkey:auto-end
    ],
)
"#,
            ),
        ],
    );
    let s = go_syncer(&root, launcher, false);
    let rules_path = format!("{root}/cmd/hello/rules.star");
    let result = s.sync_file(&rules_path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    assert!(result.updated, "rules.star not updated");
    let f = rules_star::parse_file(Path::new(&rules_path)).unwrap();
    assert_eq!(f.targets[0].get_deps(), ["//pkg/greet:greet"]);
}

/// Each target whose deps change gets its own entry in the result, test
/// targets included, instead of the last one overwriting the others.
#[test]
fn sync_file_reports_each_target() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let (_dir, root) = temp_dir();
    write_files(
        &root,
        &[
            ("go.mod", "module example.com/project\n\ngo 1.22\n"),
            (
                "pkg/greet/greet.go",
                "package greet\n\nfunc Hello() string { return \"hello\" }\n",
            ),
            (
                "pkg/check/check.go",
                "package check\n\nfunc OK() bool { return true }\n",
            ),
            ("pkg/old/old.go", "package old\n"),
            (
                "pkg/lib/lib.go",
                "package lib\n\nimport \"example.com/project/pkg/greet\"\n\nfunc Hi() string { return greet.Hello() }\n",
            ),
            (
                "pkg/lib/lib_test.go",
                r#"package lib

import (
	"testing"

	"example.com/project/pkg/check"
)

func TestHi(t *testing.T) { _ = check.OK() }
"#,
            ),
            (
                "pkg/lib/rules.star",
                r#"load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "lib",
    srcs = ["lib.go"],
    deps = [
        # turnkey:auto-start
        "//pkg/old:old",
        # turnkey:auto-end
    ],
)

go_test(
    name = "lib_test",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        # turnkey:auto-end
    ],
)
"#,
            ),
        ],
    );
    let s = go_syncer(&root, launcher, true);
    let result = s.sync_file(&format!("{root}/pkg/lib/rules.star")).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    assert_eq!(
        result.changes,
        [
            TargetChange {
                target: "lib".into(),
                added: strings(&["//pkg/greet:greet"]),
                removed: strings(&["//pkg/old:old"]),
                ..TargetChange::default()
            },
            TargetChange {
                target: "lib_test".into(),
                added: strings(&["//pkg/greet:greet", "//pkg/check:check"]),
                ..TargetChange::default()
            },
        ]
    );
}

/// In a go.work workspace, an import of a member maps to the member's
/// package, including a member that is a local fork of a third-party
/// module; a rules.star under a go.mod that isn't a member is left alone.
#[test]
fn sync_file_go_workspace() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let (_dir, root) = temp_dir();
    let stale = r#"load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "hello",
    srcs = glob(["*.go"]),
    deps = [
        # turnkey:auto-start
        "//old:old",
        # turnkey:auto-end
    ],
)
"#;
    write_files(
        &root,
        &[
            (
                "go.work",
                "go 1.22\n\nuse (\n\t./app\n\t./lib\n\t./third_party/x-sys\n)\n",
            ),
            (
                "go-deps.toml",
                "schema_version = 2\n\n[members.\"example.com/app\"]\ndir = \"app\"\n\n[members.\"example.com/lib\"]\ndir = \"lib\"\n\n[members.\"golang.org/x/sys\"]\ndir = \"third_party/x-sys\"\n",
            ),
            ("app/go.mod", "module example.com/app\n\ngo 1.22\n"),
            ("lib/go.mod", "module example.com/lib\n\ngo 1.22\n"),
            (
                "lib/lib.go",
                "package lib\n\nfunc Hello() string { return \"hello\" }\n",
            ),
            (
                "third_party/x-sys/go.mod",
                "module golang.org/x/sys\n\ngo 1.22\n",
            ),
            (
                "third_party/x-sys/cpu/cpu.go",
                "package cpu\n\nconst X86 = false\n",
            ),
            (
                "app/cmd/hello/main.go",
                r#"package main

import (
	"fmt"

	"example.com/lib"
	"golang.org/x/sys/cpu"
)

func main() { fmt.Println(lib.Hello(), cpu.X86) }
"#,
            ),
            ("app/cmd/hello/rules.star", stale),
            (
                "app/testdata/fixture/go.mod",
                "module example.com/fixture\n\ngo 1.22\n",
            ),
            (
                "app/testdata/fixture/main.go",
                "package main\n\nfunc main() {}\n",
            ),
            ("app/testdata/fixture/rules.star", stale),
        ],
    );
    let s = go_syncer(&root, launcher, false);
    let rules_path = format!("{root}/app/cmd/hello/rules.star");
    let result = s.sync_file(&rules_path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    let f = rules_star::parse_file(Path::new(&rules_path)).unwrap();
    assert_eq!(
        f.targets[0].get_deps(),
        ["//lib:lib", "//third_party/x-sys/cpu:cpu"]
    );

    let fixture = format!("{root}/app/testdata/fixture/rules.star");
    let result = s.sync_file(&fixture).unwrap();
    assert!(
        !result.updated && result.errors.is_empty(),
        "fixture outside the workspace: {result:?}"
    );
    assert_eq!(std::fs::read_to_string(&fixture).unwrap(), stale);
}

/// A Go module whose packages depend on the platform and on build tags,
/// with sync.toml allowing the integration tag
fn go_conditional_fixture() -> (tempfile::TempDir, String) {
    let (dir, root) = temp_dir();
    let mut platforms = String::new();
    for p in default_platforms() {
        platforms += &format!(
            "\n[[conditions.platforms]]\nos = \"{}\"\ncpu = \"{}\"\n",
            p.os, p.cpu
        );
    }
    let sync_toml = format!(
        "[[languages]]\nname = \"go\"\ncell = \"godeps\"\ndeps_file = \"go-deps.toml\"\n\n[conditions]\nsettings = \"toolchains//conditions\"\ngo_tags = [\"integration\"]\n{platforms}"
    );
    write_files(
        &root,
        &[
            (".turnkey/sync.toml", &sync_toml),
            ("go.mod", "module example.com/project\n\ngo 1.22\n"),
            ("pkg/common/common.go", "package common\n"),
            ("pkg/inotify/inotify.go", "package inotify\n"),
            ("pkg/harness/harness.go", "package harness\n"),
            (
                "pkg/watch/watch.go",
                "package watch\n\nimport _ \"example.com/project/pkg/common\"\n",
            ),
            (
                "pkg/watch/watch_linux.go",
                "package watch\n\nimport _ \"example.com/project/pkg/inotify\"\n",
            ),
            (
                "pkg/watch/it.go",
                "//go:build integration\n\npackage watch\n\nimport _ \"example.com/project/pkg/harness\"\n",
            ),
            (
                "pkg/watch/rules.star",
                "go_library(\n    name = \"watch\",\n    deps = [],\n)\n",
            ),
            ("pkg/fixtures/fixtures.go", "package fixtures\n"),
            (
                "pkg/fixtures/it.go",
                "//go:build integration\n\npackage fixtures\n\nimport _ \"example.com/project/pkg/harness\"\n",
            ),
            (
                "pkg/fixtures/rules.star",
                "go_library(\n    name = \"fixtures\",\n    deps = [],\n)\n",
            ),
            (
                "cmd/it/main.go",
                "//go:build integration\n\npackage main\n\nimport _ \"example.com/project/pkg/harness\"\n\nfunc main() {}\n",
            ),
            (
                "cmd/it/rules.star",
                "go_binary(\n    name = \"it\",\n    build_tags = [\"integration\"],\n    deps = [],\n)\n",
            ),
        ],
    );
    (dir, root)
}

/// Syncs one rules.star of a fixture, with the project's sync.toml, and
/// returns what sync wrote.
fn sync_go(root: &str, rel: &str, launcher: Launcher) -> String {
    let s = Syncer::new(Config {
        project_root: root.to_string(),
        force: true,
        launcher,
        cwd: "/".to_string(),
        ..Config::default()
    })
    .unwrap();
    let path = format!("{root}/{rel}");
    let result = s.sync_file(&path).unwrap();
    assert!(result.errors.is_empty(), "sync errors: {:?}", result.errors);
    std::fs::read_to_string(&path).unwrap()
}

const WATCH_SYNCED: &str = r#"go_library(
    name = "watch",
    deps = ["//pkg/common:common"] + select({
        "toolchains//conditions:linux-integration": [
            "//pkg/harness:harness",
            "//pkg/inotify:inotify",
        ],
        "toolchains//conditions:linux-no_integration": ["//pkg/inotify:inotify"],
        "toolchains//conditions:macos-integration": ["//pkg/harness:harness"],
        "toolchains//conditions:macos-no_integration": [],
    }),
)
"#;

/// A _linux.go file's import is a config//os:linux branch, whatever the
/// host, and a //go:build integration file's import in a library is a
/// branch on the tag's constraint: the OS and the tag combine on the
/// toolchains cell's settings only when both matter.
#[test]
fn sync_file_go_platform_and_tag_deps() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let (_dir, root) = go_conditional_fixture();
    assert_eq!(
        sync_go(&root, "pkg/watch/rules.star", launcher),
        WATCH_SYNCED
    );
}

/// A library's tagged import alone is a branch on the tag's constraint.
#[test]
fn sync_file_go_tag_deps() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let (_dir, root) = go_conditional_fixture();
    assert_eq!(
        sync_go(&root, "pkg/fixtures/rules.star", launcher),
        r#"go_library(
    name = "fixtures",
    deps = select({
        "prelude//go/tags/constraints:integration[set]": ["//pkg/harness:harness"],
        "prelude//go/tags/constraints:integration[unset]": [],
    }),
)
"#
    );
}

/// A binary is built with its build_tags, literally: the tagged import is
/// a plain dep.
#[test]
fn sync_file_go_binary_build_tags() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let (_dir, root) = go_conditional_fixture();
    assert_eq!(
        sync_go(&root, "cmd/it/rules.star", launcher),
        "go_binary(\n    name = \"it\",\n    build_tags = [\"integration\"],\n    deps = [\"//pkg/harness:harness\"],\n)\n"
    );
}

/// Go sync doesn't depend on the host: with the go command seeing each
/// host's GOOS and GOARCH (a `go` on the PATH that sets them unless sync
/// does), the file is the same.
#[test]
fn sync_file_go_is_host_independent() {
    let Some(launcher) = launcher_with("go") else {
        return;
    };
    let go = launcher.look_path(OsStr::new("go")).unwrap();
    for (goos, goarch) in [
        ("linux", "amd64"),
        ("linux", "arm64"),
        ("darwin", "amd64"),
        ("darwin", "arm64"),
    ] {
        let (_dir, root) = go_conditional_fixture();
        let bin = format!("{root}/.host-bin");
        // Written by a child shell so this test binary never holds it open
        // for writing: a test forking in another thread meanwhile would
        // inherit that descriptor, and running it while it is open fails
        // with ETXTBSY (rust-lang/rust#114554)
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"mkdir -p "$1" && printf '%s' "$2" > "$1/go" && chmod 755 "$1/go""#,
                "sh",
                &bin,
            ])
            .arg(format!(
                "#!/bin/sh\n: \"${{GOOS:={goos}}}\" \"${{GOARCH:={goarch}}}\"\nexport GOOS GOARCH\nexec {} \"$@\"\n",
                go.display()
            ))
            .status()
            .unwrap();
        assert!(status.success(), "writing {bin}/go");
        let host = Launcher::new(Some(bin.into()));
        assert_eq!(
            sync_go(&root, "pkg/watch/rules.star", host),
            WATCH_SYNCED,
            "host {goos}/{goarch}"
        );
    }
}
