//! src/go/pkg/mapper's tests, ported

use super::golang::{GoLanguage, classify_go_import, detect_go_config};
use super::*;
use conditions::{Platform, Space};
use deps_extract::extraction::{
    Import, ImportKind, Package as ExtractedPackage, Result as Extracted,
};
use project_sync::config;
use rules_star::Value;
use std::cell::RefCell;

/// A project's languages as sync.toml lists them, each with its default
/// cell and deps file
fn test_languages() -> Vec<config::Language> {
    [
        ("go", "godeps", "go-deps.toml"),
        ("rust", "rustdeps", "rust-deps.toml"),
        ("python", "pydeps", "python-deps.toml"),
        ("javascript", "jsdeps", "js-deps.toml"),
        ("solidity", "soldeps", "solidity-deps.toml"),
    ]
    .into_iter()
    .map(|(name, cell, deps_file)| config::Language {
        name: name.into(),
        cell: cell.into(),
        deps_file: deps_file.into(),
    })
    .collect()
}

/// The configuration of a mapper for the project at `root`, with
/// [`test_languages`]
fn test_config(root: &str) -> Config {
    Config {
        project_root: root.to_string(),
        cwd: "/".to_string(),
        languages: test_languages(),
        ..Config::default()
    }
}

/// The language of [`test_languages`] named `name`
fn test_language(name: &str) -> config::Language {
    test_languages()
        .into_iter()
        .find(|l| l.name == name)
        .expect("a test language")
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
fn write_tree(root: &str, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = std::path::Path::new(root).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn import(path: &str, kind: ImportKind) -> Import {
    Import {
        path: path.to_string(),
        kind,
    }
}

// --- language_test.go

/// Each language sync.toml lists gets its plug-in, in the listed order.
#[test]
fn new_languages() {
    let (_dir, root) = temp_dir();
    let m = Mapper::new(&test_config(&root)).unwrap();
    let names: Vec<&str> = m.languages().iter().map(|l| l.name()).collect();
    assert_eq!(names, ["go", "rust", "python", "typescript", "solidity"]);
}

/// A language no plug-in serves is an error, rather than a language whose
/// targets sync silently leaves alone.
#[test]
fn new_unknown_language() {
    let (_dir, root) = temp_dir();
    let cfg = Config {
        languages: vec![config::Language {
            name: "cobol".into(),
            cell: "cobdeps".into(),
            deps_file: "cobol-deps.toml".into(),
        }],
        ..test_config(&root)
    };
    let err = Mapper::new(&cfg).err().expect("an error");
    assert_eq!(
        err.to_string(),
        "no rules sync plug-in for language \"cobol\""
    );
}

/// How a plug-in classifies rule kinds: (rule, owned, kind)
fn assert_rule_kinds(lang: &dyn Language, cases: &[(&str, bool, TargetKind)]) {
    for &(rule, owned, kind) in cases {
        let got = lang.rule(rule);
        assert_eq!(got.is_some(), owned, "{}.rule({rule})", lang.name());
        if let Some(r) = got {
            assert_eq!(r.kind, kind, "{}.rule({rule})", lang.name());
        }
    }
}

/// The plug-in of `name`, for the project at a temporary directory
fn plugin(name: &str) -> (tempfile::TempDir, Box<dyn Language>) {
    let (dir, root) = temp_dir();
    let cfg = Config {
        languages: vec![test_language(name)],
        ..test_config(&root)
    };
    let mut m = Mapper::new(&cfg).unwrap();
    (dir, m.languages.remove(0))
}

#[test]
fn go_rule_kinds() {
    let (_dir, lang) = plugin("go");
    assert_rule_kinds(
        lang.as_ref(),
        &[
            ("go_library", true, TargetKind::Library),
            ("go_binary", true, TargetKind::Binary),
            ("go_test", true, TargetKind::Test),
            // contains "test" but isn't a test rule
            ("go_attested_library", false, TargetKind::NotSynced),
            ("rust_binary", false, TargetKind::NotSynced),
        ],
    );
}

#[test]
fn rust_rule_kinds() {
    let (_dir, lang) = plugin("rust");
    assert_rule_kinds(
        lang.as_ref(),
        &[
            ("rust_library", true, TargetKind::Library),
            ("rust_binary", true, TargetKind::Binary),
            ("rust_test", true, TargetKind::Test),
            ("rust_testdata", false, TargetKind::NotSynced),
            ("go_test", false, TargetKind::NotSynced),
        ],
    );
}

#[test]
fn python_rule_kinds() {
    let (_dir, lang) = plugin("python");
    assert_rule_kinds(
        lang.as_ref(),
        &[
            ("python_library", true, TargetKind::Library),
            ("python_binary", true, TargetKind::Binary),
            ("python_test", true, TargetKind::Test),
            ("python_test_utils", false, TargetKind::NotSynced),
            ("sh_binary", false, TargetKind::NotSynced),
        ],
    );
}

#[test]
fn typescript_rule_kinds() {
    let (_dir, lang) = plugin("javascript");
    assert_rule_kinds(
        lang.as_ref(),
        &[
            ("typescript_library", true, TargetKind::Library),
            ("typescript_binary", true, TargetKind::Binary),
            ("js_library", true, TargetKind::Library),
            ("js_test", true, TargetKind::Test),
            ("js_contest_bundle", false, TargetKind::NotSynced),
        ],
    );
}

#[test]
fn solidity_rule_kinds() {
    let (_dir, lang) = plugin("solidity");
    assert_rule_kinds(
        lang.as_ref(),
        &[
            ("solidity_library", true, TargetKind::Library),
            ("solidity_test", true, TargetKind::Test),
            // a Solidity rule whose deps sync leaves alone
            ("solidity_contract", true, TargetKind::NotSynced),
            ("solidity_attestation", false, TargetKind::NotSynced),
        ],
    );
}

/// Every binary rule of a supported language is synced like a library, and
/// a rule no plug-in owns is left alone.
#[test]
fn rule_language() {
    let (_dir, root) = temp_dir();
    let m = Mapper::new(&test_config(&root)).unwrap();
    for (rule, want) in [
        ("go_binary", "go"),
        ("rust_binary", "rust"),
        ("python_binary", "python"),
        ("sh_binary", ""),
        ("genrule", ""),
    ] {
        let got = m.rule_language(rule);
        assert_eq!(got.as_ref().map_or("", |(l, _)| l.name()), want, "{rule}");
        if let Some((_, r)) = got {
            assert_eq!(r.kind, TargetKind::Binary, "{rule}");
        }
    }
}

/// A Rust dep pinning a crate's version stands for the crate's target; any
/// other label stands for itself.
#[test]
fn rust_labels_stand_for_their_unversioned_target() {
    let (_dir, lang) = plugin("rust");
    let canonical = lang.rule("rust_library").unwrap().canonical.unwrap();
    for (label, want) in [
        (
            "rustdeps//vendor/tokio@1.50.0:tokio",
            "rustdeps//vendor/tokio:tokio",
        ),
        (
            "rustdeps//vendor/ring@0.17.14:ring_core_0_17_14__",
            "rustdeps//vendor/ring:ring_core_0_17_14__",
        ),
        (
            "rustdeps//vendor/tokio:tokio",
            "rustdeps//vendor/tokio:tokio",
        ),
        (
            "//src/rust/composition:composition-full",
            "//src/rust/composition:composition-full",
        ),
        ("//a@b/c:c", "//a@b/c:c"),
    ] {
        assert_eq!(canonical(label), want, "{label}");
    }
}

// --- mapper_test.go

fn go_config(members: &[(&str, &str)], external: &[&str]) -> GoConfig {
    GoConfig {
        members: members
            .iter()
            .map(|(path, dir)| GoMember {
                path: path.to_string(),
                dir: dir.to_string(),
            })
            .collect(),
        external_cell: "godeps".into(),
        external_deps: external.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn map_go_imports() {
    let lang = GoLanguage::with_config(
        "",
        go_config(
            &[("github.com/firefly-engineering/turnkey", ".")],
            &[
                "github.com/google/uuid",
                "golang.org/x/sys",
                "go.starlark.net",
            ],
        ),
    );
    let tests: &[(&str, Vec<Import>, &[&str])] = &[
        (
            "stdlib only",
            vec![
                import("fmt", ImportKind::Stdlib),
                import("os", ImportKind::Stdlib),
            ],
            &[],
        ),
        (
            "internal import",
            vec![import(
                "github.com/firefly-engineering/turnkey/src/go/pkg/foo",
                ImportKind::Internal,
            )],
            &["//src/go/pkg/foo:foo"],
        ),
        (
            "external import",
            vec![import("github.com/google/uuid", ImportKind::External)],
            &["godeps//vendor/github.com/google/uuid:uuid"],
        ),
        (
            "external subpackage",
            vec![import("golang.org/x/sys/cpu", ImportKind::External)],
            &["godeps//vendor/golang.org/x/sys/cpu:cpu"],
        ),
        (
            "mixed imports",
            vec![
                import("fmt", ImportKind::Stdlib),
                import(
                    "github.com/firefly-engineering/turnkey/src/go/pkg/bar",
                    ImportKind::Internal,
                ),
                import("go.starlark.net/syntax", ImportKind::External),
            ],
            &[
                "//src/go/pkg/bar:bar",
                "godeps//vendor/go.starlark.net/syntax:syntax",
            ],
        ),
        (
            "deduplication",
            vec![
                import("github.com/google/uuid", ImportKind::External),
                import("github.com/google/uuid", ImportKind::External),
            ],
            &["godeps//vendor/github.com/google/uuid:uuid"],
        ),
    ];
    for (name, imports, want) in tests {
        let (deps, _) = map_imports(&lang, imports);
        assert_eq!(deps_to_targets(&deps), *want, "{name}");
    }
}

/// An extraction result's imports are mapped with the language: stdlib
/// skipped, internal and external imports mapped.
#[test]
fn resolves_imports() {
    let lang = GoLanguage::with_config(
        "",
        go_config(
            &[("github.com/example/project", ".")],
            &["github.com/google/uuid"],
        ),
    );
    let mut result = Extracted::new("go");
    result.add_package(ExtractedPackage {
        path: "src/cmd/myapp".into(),
        files: strings(&["main.go"]),
        imports: vec![
            import("fmt", ImportKind::Stdlib),
            import(
                "github.com/example/project/src/go/pkg/lib",
                ImportKind::Internal,
            ),
            import("github.com/google/uuid", ImportKind::External),
        ],
        test_imports: vec![import("testing", ImportKind::Stdlib)],
    });
    let mapping = resolve_imports(&lang, &result);
    assert_eq!(
        deps_to_targets(&mapping.deps),
        [
            "//src/go/pkg/lib:lib",
            "godeps//vendor/github.com/google/uuid:uuid"
        ]
    );
}

/// go-deps.toml as godeps-gen writes it, with two modules and no hashes
/// (--no-prefetch)
const GODEPS_GEN_DEPS: &str = r#"# Auto-generated by godeps-gen
# Source: the files listed under sources
#
# Key format: deps."import-path@version" to support multiple versions
#
# IMPORTANT: Nix hashes must be obtained separately.
# Prefetching (on unless --no-prefetch) fetches them; or manually run:
# nix-prefetch-cached --unpack https://proxy.golang.org/MODULE/@v/VERSION.zip
#
# To regenerate: tk sync

schema_version = 2

[deps."github.com/pelletier/go-toml/v2@v2.2.4"]
import_path = "github.com/pelletier/go-toml/v2"
version = "v2.2.4"
hash = ""

[deps."golang.org/x/mod@v0.31.0"]
import_path = "golang.org/x/mod"
version = "v0.31.0"
hash = ""

"#;

/// go-deps.toml as godeps-gen writes it for a go.work workspace of two
/// members and no dependencies
const GODEPS_GEN_MEMBERS: &str = r#"# Auto-generated by godeps-gen
# Source: the files listed under sources
#
# Key format: deps."import-path@version" to support multiple versions
#
# To regenerate: tk sync

schema_version = 2

[members."example.com/b"]
dir = "b"

[members."example.com/a"]
dir = "a"

"#;

/// The mapper reads the go-deps.toml godeps-gen writes: its keys carry a
/// version ("path@version"), so imports must match on import_path.
#[test]
fn go_deps_from_godeps_gen() {
    let (_dir, root) = temp_dir();
    write_tree(
        &root,
        &[
            ("go.mod", "module github.com/example/project\n"),
            ("go-deps.toml", GODEPS_GEN_DEPS),
        ],
    );
    let lang = GoLanguage::with_config(&root, detect_go_config(&root, &test_language("go")));
    let (mapped, unmapped) = map_imports(
        &lang,
        &[
            import("github.com/pelletier/go-toml/v2", ImportKind::External),
            import("golang.org/x/mod/modfile", ImportKind::External),
        ],
    );
    assert!(unmapped.is_empty(), "unmapped = {unmapped:?}");
    assert_eq!(
        deps_to_targets(&mapped),
        [
            "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
            "godeps//vendor/golang.org/x/mod/modfile:modfile",
        ]
    );
}

#[test]
fn unmapped_external_dep() {
    let lang = GoLanguage::with_config(
        "",
        go_config(
            &[("github.com/example/project", ".")],
            &["github.com/google/uuid"],
        ),
    );
    let (deps, unmapped) = map_imports(
        &lang,
        &[import("github.com/unknown/package", ImportKind::External)],
    );
    assert!(deps.is_empty());
    assert_eq!(unmapped, ["github.com/unknown/package"]);
}

/// An import maps to the workspace member whose module path is its longest
/// prefix on a / boundary, at its directory, with the target named after
/// the import path's last component (as the godeps cell's forwarding
/// aliases name it), and the longest module path wins between a member and
/// a third-party module.
#[test]
fn map_go_workspace_imports() {
    let cfg = go_config(
        &[
            ("example.com/app", "."),
            ("example.com/app/tools", "tools"),
            ("example.com/lib", "libs/lib"),
            ("golang.org/x/sys", "third_party/x-sys"),
        ],
        &["example.com/libextra", "example.com/lib/proto"],
    );
    let lang = GoLanguage::with_config("", cfg.clone());
    for (imp, kind, want) in [
        (
            "example.com/app/internal/x",
            ImportKind::Internal,
            "//internal/x:x",
        ),
        ("example.com/app", ImportKind::Internal, "//:app"),
        (
            "example.com/app/tools/gen",
            ImportKind::Internal,
            "//tools/gen:gen",
        ),
        (
            "example.com/lib/sub",
            ImportKind::Internal,
            "//libs/lib/sub:sub",
        ),
        (
            "golang.org/x/sys",
            ImportKind::Internal,
            "//third_party/x-sys:sys",
        ),
        (
            "golang.org/x/sys/cpu",
            ImportKind::Internal,
            "//third_party/x-sys/cpu:cpu",
        ),
        // Not on a / boundary: a third-party module
        (
            "example.com/libextra/y",
            ImportKind::External,
            "godeps//vendor/example.com/libextra/y:y",
        ),
        // A third-party module nested in a member's module path
        (
            "example.com/lib/proto/v1",
            ImportKind::External,
            "godeps//vendor/example.com/lib/proto/v1:v1",
        ),
        ("fmt", ImportKind::Stdlib, ""),
    ] {
        assert_eq!(classify_go_import(imp, &cfg), kind, "classify {imp}");
        let (deps, unmapped) = map_imports(&lang, &[import(imp, kind)]);
        assert!(unmapped.is_empty(), "{imp}: unmapped");
        let got = deps_to_targets(&deps);
        assert_eq!(got.first().map_or("", String::as_str), want, "{imp}");
    }
}

/// An import is first-party only on a / boundary of a member's module
/// path.
#[test]
fn classify_go_import_boundary() {
    let cfg = go_config(&[("github.com/x/fo", ".")], &[]);
    assert_eq!(
        classify_go_import("github.com/x/foo", &cfg),
        ImportKind::External
    );
}

/// The members come from go-deps.toml's [members] when it records them,
/// and otherwise are the root go.mod alone.
#[test]
fn go_members_from_deps_file() {
    let (_dir, root) = temp_dir();
    write_tree(&root, &[("go.mod", "module example.com/root\n")]);
    let lang = test_language("go");
    let cfg = detect_go_config(&root, &lang);
    assert_eq!(
        cfg.members,
        [GoMember {
            path: "example.com/root".into(),
            dir: ".".into()
        }]
    );

    write_tree(&root, &[(&lang.deps_file, GODEPS_GEN_MEMBERS)]);
    let cfg = detect_go_config(&root, &lang);
    assert_eq!(
        cfg.members,
        [
            GoMember {
                path: "example.com/a".into(),
                dir: "a".into()
            },
            GoMember {
                path: "example.com/b".into(),
                dir: "b".into()
            },
        ]
    );
}

/// Sync manages a package's Go rules only when the nearest go.mod above it
/// is a member's.
#[test]
fn go_manages() {
    let (_dir, root) = temp_dir();
    for f in ["go.mod", "lib/go.mod", "src/testdata/fixture/go.mod"] {
        write_tree(&root, &[(f, "module example.com/m\n")]);
    }
    let members = go_config(&[("example.com/app", "."), ("example.com/lib", "lib")], &[]);
    let lang = GoLanguage::with_config(&root, members);
    for (dir, want) in [
        (".", true),
        ("cmd/app", true),
        ("lib", true),
        ("lib/sub", true),
        ("src/testdata/fixture", false),
        ("src/testdata/fixture/deep", false),
    ] {
        assert_eq!(
            lang.manages(&gostd::path::join(&[&root, dir])),
            want,
            "manages({dir})"
        );
    }

    // Without a member at the root, a package under no member isn't managed
    let lang = GoLanguage::with_config(&root, go_config(&[("example.com/lib", "lib")], &[]));
    assert!(!lang.manages(&gostd::path::join(&[&root, "cmd/app"])));
}

// --- cargo_test.go

/// A workspace with two members: app depends on lib and on external
/// crates, one of them renamed.
fn cargo_workspace_fixture() -> (tempfile::TempDir, String) {
    let (dir, root) = temp_dir();
    write_tree(
        &root,
        &[
            (
                "Cargo.toml",
                r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
anyhow = "1.0"
tree-sitter = "0.25"
json = { package = "serde_json", version = "1.0" }
tempfile = "3"
libc = "0.2"
my-lib = { path = "crates/lib" }
"#,
            ),
            (
                "rust-deps.toml",
                r#"schema_version = 1

[deps."anyhow@1.0.0"]
name = "anyhow"
[deps."tree-sitter@0.25.0"]
name = "tree-sitter"
[deps."serde_json@1.0.0"]
name = "serde_json"
[deps."tempfile@3.0.0"]
name = "tempfile"
[deps."libc@0.2.0"]
name = "libc"
[deps."log@0.4.0"]
name = "log"
"#,
            ),
            (
                "crates/lib/Cargo.toml",
                "[package]\nname = \"my-lib\"\n\n[dependencies]\nanyhow.workspace = true\n",
            ),
            (
                "crates/app/Cargo.toml",
                r#"[package]
name = "app"

[dependencies]
anyhow.workspace = true
tree-sitter.workspace = true
json.workspace = true
my-lib.workspace = true
log = "0.4"
unknown-crate = "1"
libc = { workspace = true, optional = true }

[dev-dependencies]
tempfile.workspace = true

[target.'cfg(unix)'.dependencies]
my-lib = { workspace = true }
"#,
            ),
        ],
    );
    (dir, root)
}

/// A Mapper's plug-in named `name`
fn language<'a>(m: &'a Mapper, name: &str) -> &'a dyn Language {
    m.language(name).expect("the plug-in")
}

/// A crate's external deps are in its language's cell, as listed in its
/// language's deps file, wherever sync.toml says they are.
#[test]
fn map_rust_crate_from_language() {
    let (_dir, root) = cargo_workspace_fixture();
    std::fs::create_dir_all(format!("{root}/third-party")).unwrap();
    std::fs::rename(
        format!("{root}/rust-deps.toml"),
        format!("{root}/third-party/rust-deps.toml"),
    )
    .unwrap();
    let m = Mapper::new(&Config {
        languages: vec![config::Language {
            name: "rust".into(),
            cell: "crates".into(),
            deps_file: "third-party/rust-deps.toml".into(),
        }],
        ..test_config(&root)
    })
    .unwrap();
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/lib"), &Request::default())
        .unwrap();
    assert_eq!(
        deps_to_targets(&mapping.deps),
        ["crates//vendor/anyhow:anyhow"]
    );
}

#[test]
fn map_rust_crate() {
    let (_dir, root) = cargo_workspace_fixture();
    let m = Mapper::new(&test_config(&root)).unwrap();
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/app"), &Request::default())
        .unwrap();
    assert_eq!(
        deps_to_targets(&mapping.deps),
        [
            // workspace member, found through its path
            "//crates/lib:lib",
            // workspace = true
            "rustdeps//vendor/anyhow:anyhow",
            // a plain version string
            "rustdeps//vendor/log:log",
            // renamed in [workspace.dependencies]: the package name
            "rustdeps//vendor/serde_json:serde_json",
            // hyphenated: the Cargo package name as rust-deps.toml has it
            "rustdeps//vendor/tree-sitter:tree-sitter",
        ]
    );
    // [dev-dependencies] go to test targets only
    assert_eq!(
        deps_to_targets(&mapping.test_deps),
        ["rustdeps//vendor/tempfile:tempfile"]
    );
    assert_eq!(mapping.unmapped_imports, ["unknown-crate"]);

    // An optional dependency no feature activates isn't a dep; without a
    // platform, a target-specific table isn't evaluated
    assert_eq!(
        mapping.unsynced_deps,
        [UnsyncedDep {
            dep: MappedDep {
                target: "//crates/lib:lib".into(),
                kind: DependencyType::Internal,
                import_path: "my-lib".into(),
            },
            reason: "target-specific (cfg(unix))".into(),
        }]
    );
}

/// A workspace member named in a dependency without a path still maps to
/// the member's target, through the members' package names.
#[test]
fn map_rust_crate_member_by_name() {
    let (_dir, root) = cargo_workspace_fixture();
    write_tree(
        &root,
        &[(
            "crates/other/Cargo.toml",
            "[package]\nname = \"other\"\n\n[dependencies]\nmy-lib = \"0.1\"\n",
        )],
    );
    let m = Mapper::new(&test_config(&root)).unwrap();
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/other"), &Request::default())
        .unwrap();
    assert_eq!(deps_to_targets(&mapping.deps), ["//crates/lib:lib"]);
}

/// A workspace = true entry the workspace doesn't declare is an error, not
/// a silently missing dep.
#[test]
fn map_rust_crate_missing_workspace_dep() {
    let (_dir, root) = cargo_workspace_fixture();
    write_tree(
        &root,
        &[(
            "crates/bad/Cargo.toml",
            "[package]\nname = \"bad\"\n\n[dependencies]\nnope.workspace = true\n",
        )],
    );
    let m = Mapper::new(&test_config(&root)).unwrap();
    let err = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/bad"), &Request::default())
        .unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "reading Cargo.toml: dependency nope: workspace = true, but [workspace.dependencies] has no nope"
    );
}

/// A crate without a Cargo.toml is an error worded as Go's
#[test]
fn map_rust_crate_without_manifest() {
    let (_dir, root) = cargo_workspace_fixture();
    let m = Mapper::new(&test_config(&root)).unwrap();
    let err = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/none"), &Request::default())
        .unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        format!(
            "reading Cargo.toml: open {root}/crates/none/Cargo.toml: no such file or directory"
        )
    );
}

fn config(os: &str, cpu: &str) -> Configuration {
    Configuration::new([("os", os), ("cpu", cpu)])
}

/// A [target.'<spec>'.*] table applies in the configurations whose platform
/// its spec holds on, like the unconditional tables.
#[test]
fn map_rust_crate_target_specific() {
    let (_dir, root) = temp_dir();
    write_tree(
        &root,
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "rust-deps.toml",
                r#"[deps."libc@0.2.0"]
name = "libc"
[deps."inotify@0.11.0"]
name = "inotify"
[deps."core-foundation@0.10.0"]
name = "core-foundation"
[deps."tempfile@3.0.0"]
name = "tempfile"
"#,
            ),
            (
                "crates/app/Cargo.toml",
                r#"[package]
name = "app"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(target_os = "linux")'.dependencies]
inotify = "0.11"

[target.aarch64-apple-darwin.dependencies]
core-foundation = "0.10"

[target.'cfg(target_os = "linux")'.dev-dependencies]
tempfile = "3"
"#,
            ),
        ],
    );
    let m = Mapper::new(&test_config(&root)).unwrap();
    let lang = language(&m, "rust");
    let krate = format!("{root}/crates/app");
    assert_eq!(
        lang.dimensions(&krate).unwrap(),
        Dimensions {
            platform: strings(&["os", "cpu"]),
            on_off: vec![]
        }
    );
    for (os, cpu, deps, test_deps) in [
        (
            "linux",
            "x86_64",
            &[
                "rustdeps//vendor/inotify:inotify",
                "rustdeps//vendor/libc:libc",
            ][..],
            &["rustdeps//vendor/tempfile:tempfile"][..],
        ),
        (
            "macos",
            "x86_64",
            &["rustdeps//vendor/libc:libc"][..],
            &[][..],
        ),
        (
            "macos",
            "arm64",
            &[
                "rustdeps//vendor/core-foundation:core-foundation",
                "rustdeps//vendor/libc:libc",
            ][..],
            &[][..],
        ),
    ] {
        let req = Request {
            config: config(os, cpu),
            ..Request::default()
        };
        let mapping = lang.resolve_deps(&krate, &req).unwrap();
        assert_eq!(deps_to_targets(&mapping.deps), deps, "{os}-{cpu}: deps");
        assert_eq!(
            deps_to_targets(&mapping.test_deps),
            test_deps,
            "{os}-{cpu}: test deps"
        );
        assert!(mapping.unsynced_deps.is_empty(), "{os}-{cpu}: unsynced");
    }

    // Without a platform, target-specific tables stay unsynced
    let mapping = lang.resolve_deps(&krate, &Request::default()).unwrap();
    assert!(mapping.deps.is_empty());
    assert_eq!(mapping.unsynced_deps.len(), 4);
}

/// A workspace whose member lib has a primary target and a variant, and
/// whose member app depends on lib.
fn features_fixture(app_deps: &str) -> (tempfile::TempDir, String, Mapper) {
    let (dir, root) = temp_dir();
    write_tree(
        &root,
        &[
            (
                "Cargo.toml",
                r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
lib = { path = "crates/lib", features = ["base"] }
"#,
            ),
            (
                "rust-deps.toml",
                r#"[deps."libc@0.2.0"]
name = "libc"
[deps."notify@8.0.0"]
name = "notify"
[deps."log@0.4.0"]
name = "log"
"#,
            ),
            (
                "crates/lib/Cargo.toml",
                r#"[package]
name = "lib"

[features]
default = ["base"]
base = []
watch = ["dep:notify", "log/std"]
fuse = ["dep:libc"]

[dependencies]
log = "0.4"
libc = { version = "0.2", optional = true }
notify = { version = "8", optional = true }
"#,
            ),
            (
                "crates/lib/rules.star",
                r#"rust_library(
    name = "lib",
)

rust_library(
    name = "lib-full",
    cargo_features = ["watch"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": [],
    }),
)

rust_library(
    name = "lib-bare",
    default_features = False,
)
"#,
            ),
            (
                "crates/app/Cargo.toml",
                &format!("[package]\nname = \"app\"\n\n[dependencies]\n{app_deps}"),
            ),
        ],
    );
    let mut cfg = test_config(&root);
    cfg.conditions.platforms = vec![
        Platform {
            os: "linux".into(),
            cpu: "x86_64".into(),
        },
        Platform {
            os: "macos".into(),
            cpu: "arm64".into(),
        },
    ];
    let m = Mapper::new(&cfg).unwrap();
    (dir, root, m)
}

fn linux() -> Configuration {
    config("linux", "x86_64")
}

fn macos() -> Configuration {
    config("macos", "arm64")
}

/// A primary target (no variant attributes) builds the crate's default
/// features, expanded, and the optional dependencies they activate.
#[test]
fn rust_primary_target_builds_defaults() {
    let (_dir, root, m) = features_fixture("");
    let req = Request {
        config: linux(),
        ..Request::default()
    };
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/lib"), &req)
        .unwrap();
    assert_eq!(mapping.attrs["features"], ["base"]);
    assert_eq!(deps_to_targets(&mapping.deps), ["rustdeps//vendor/log:log"]);
}

/// default_features = False leaves the defaults out of the request.
#[test]
fn rust_default_features_false() {
    let (_dir, root, m) = features_fixture("");
    let req = Request {
        config: linux(),
        variant: Variant::from([("default_features".into(), Value::Bool(false))]),
        ..Request::default()
    };
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/lib"), &req)
        .unwrap();
    assert!(mapping.attrs["features"].is_empty());
}

/// cargo_features are expanded with the defaults: dep: activates optional
/// dependencies, and x/feat forwards.
#[test]
fn rust_cargo_features_expand() {
    let (_dir, root, m) = features_fixture("");
    let req = Request {
        config: linux(),
        variant: Variant::from([(
            "cargo_features".into(),
            Value::StringList(strings(&["watch", "fuse"])),
        )]),
        ..Request::default()
    };
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/lib"), &req)
        .unwrap();
    assert_eq!(mapping.attrs["features"], ["base", "fuse", "watch"]);
    assert_eq!(
        deps_to_targets(&mapping.deps),
        [
            "rustdeps//vendor/libc:libc",
            "rustdeps//vendor/log:log",
            "rustdeps//vendor/notify:notify"
        ]
    );
}

/// A dependency asking for a member's features maps to the member target
/// whose request enables exactly those features, per configuration: its
/// cargo_features may be a select().
#[test]
fn rust_dependency_maps_to_variant_target() {
    let (_dir, root, m) = features_fixture(
        r#"lib = { workspace = true, features = ["watch"] }

[target.'cfg(target_os = "linux")'.dependencies]
lib = { workspace = true, features = ["watch", "fuse"] }
"#,
    );
    for config in [linux(), macos()] {
        let req = Request {
            config: config.clone(),
            ..Request::default()
        };
        let mapping = language(&m, "rust")
            .resolve_deps(&format!("{root}/crates/app"), &req)
            .unwrap();
        assert_eq!(
            deps_to_targets(&mapping.deps),
            ["//crates/lib:lib-full"],
            "{config}: unmapped {:?}",
            mapping.unmapped_imports
        );
    }
}

/// With no target building exactly the features asked for, or several, the
/// dependency is unmapped, and the report names the features.
#[test]
fn rust_dependency_without_matching_target() {
    let (_dir, root, m) = features_fixture(r#"lib = { workspace = true, features = ["fuse"] }"#);
    let req = Request {
        config: linux(),
        ..Request::default()
    };
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/app"), &req)
        .unwrap();
    assert!(mapping.deps.is_empty());
    assert_eq!(
        mapping.unmapped_imports,
        ["lib (needs features base, fuse: no rust_library of //crates/lib builds exactly them)"]
    );

    // lib and lib-bare both build base only with default-features = false
    // and features = ["base"]
    let (_dir, root, m) = features_fixture(
        r#"lib = { path = "../lib", default-features = false, features = ["base"] }"#,
    );
    write_tree(
        &root,
        &[(
            "crates/lib/rules.star",
            "rust_library(name = \"lib\")\n\nrust_library(\n    name = \"lib-base\",\n    cargo_features = [\"base\"],\n    default_features = False,\n)\n",
        )],
    );
    let mapping = language(&m, "rust")
        .resolve_deps(&format!("{root}/crates/app"), &req)
        .unwrap();
    assert!(mapping.deps.is_empty());
    assert_eq!(
        mapping.unmapped_imports,
        [
            "lib (needs features base: several rust_library targets of //crates/lib build them: lib, lib-base)"
        ]
    );
}

// --- target_test.go

/// A plug-in that resolves every package to `mapping`, and on Linux adds
/// //linux:only to its deps. A target's "features" variant adds
/// //feature/<name>:<name> per feature. It records what it resolved.
#[derive(Default)]
struct FakeLanguage {
    mapping: PackageMapping,
    requests: RefCell<Vec<Request>>,
}

impl Language for FakeLanguage {
    fn name(&self) -> &str {
        "fake"
    }

    fn rule(&self, rule: &str) -> Option<Rule> {
        let kind = match rule {
            "fake_library" => TargetKind::Library,
            "fake_test" => TargetKind::Test,
            _ => return None,
        };
        Some(Rule {
            kind,
            deps_attribute: "deps",
            variant: &["features"],
            canonical: None,
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
        let mut m = self.mapping.clone();
        if req.config.get(OS) == "linux" {
            m.deps.push(dep("//linux:only"));
        }
        for f in variant_labels(&req.variant, "features") {
            m.deps.push(dep(&format!("//feature/{f}:{f}")));
        }
        Ok(m)
    }
}

fn dep(target: &str) -> MappedDep {
    MappedDep {
        target: target.to_string(),
        ..MappedDep::default()
    }
}

fn fake_platforms() -> Vec<Platform> {
    [("linux", "x86_64"), ("linux", "arm64"), ("macos", "arm64")]
        .into_iter()
        .map(|(os, cpu)| Platform {
            os: os.into(),
            cpu: cpu.into(),
        })
        .collect()
}

/// The package /repo/pkg opened with `lang`, over `platforms`
fn open_fake<'a>(lang: &'a FakeLanguage, platforms: &[Platform]) -> Package<'a> {
    Package::open(lang, "/repo", "/repo/pkg", &Space::new(platforms, "")).unwrap()
}

/// The only target of a rules.star source
fn parse_target(src: &str) -> rules_star::Target {
    rules_star::parse("rules.star", src.into())
        .unwrap()
        .targets
        .remove(0)
}

/// What a fake target wants
fn fake_target<'p, 'a>(pkg: &'p Package<'a>, lang: &FakeLanguage, src: &str) -> Target<'p, 'a> {
    let t = parse_target(src);
    let rule = lang.rule(&t.rule).unwrap();
    pkg.target(&t, rule)
        .unwrap_or_else(|attr| panic!("attribute {attr} unreadable"))
}

/// A library wants its package's deps in each configuration, and the
/// package is resolved once per value of the dimensions the language
/// names, however many configurations share it.
#[test]
fn target_wants_deps_per_configuration() {
    let lang = FakeLanguage {
        mapping: PackageMapping {
            deps: vec![dep("//unix:unix")],
            ..PackageMapping::default()
        },
        ..FakeLanguage::default()
    };
    let pkg = open_fake(&lang, &fake_platforms());
    let lib = fake_target(&pkg, &lang, r#"fake_library(name = "lib")"#);
    assert_eq!(
        lib.deps(&linux(), &[]).unwrap().labels,
        ["//unix:unix", "//linux:only"]
    );
    assert_eq!(lib.deps(&macos(), &[]).unwrap().labels, ["//unix:unix"]);
    for config in &pkg.space().configurations {
        lib.deps(config, &[]).unwrap();
    }
    let requests = lang.requests.borrow();
    assert_eq!(requests.len(), 2, "resolved once per OS: {requests:?}");
    for req in requests.iter() {
        assert!(
            !req.config.has(CPU),
            "request config {} has the CPU, which the language doesn't depend on",
            req.config
        );
    }
}

/// A target's variant is read in each configuration, a select() included,
/// and resolved with.
#[test]
fn target_resolves_its_variant() {
    let lang = FakeLanguage::default();
    let pkg = open_fake(&lang, &fake_platforms());
    let lib = fake_target(
        &pkg,
        &lang,
        r#"fake_library(
    name = "lib",
    features = ["a"] + select({
        "config//os:linux": [],
        "config//os:macos": ["b"],
    }),
)"#,
    );
    assert_eq!(
        lib.deps(&macos(), &[]).unwrap().labels,
        ["//feature/a:a", "//feature/b:b"]
    );
}

/// A variant attribute whose select() has a key sync doesn't know makes the
/// target unreadable.
#[test]
fn target_unreadable_variant() {
    let lang = FakeLanguage::default();
    let pkg = open_fake(&lang, &fake_platforms());
    let t =
        parse_target(r#"fake_library(name = "lib", features = select({"//my:setting": ["a"]}))"#);
    let rule = lang.rule("fake_library").unwrap();
    assert_eq!(pkg.target(&t, rule).err(), Some("features".to_string()));
}

/// A test wants its test-only deps, and its library's deps unless it gets
/// them through a target_under_test or a same-package dep it has. Test-only
/// imports that couldn't be mapped leave only the test incomplete.
#[test]
fn test_target_composition() {
    let mapping = PackageMapping {
        deps: vec![dep("//lib:lib")],
        test_deps: vec![dep("//check:check"), dep("//lib:lib")],
        unmapped_imports: strings(&["example.com/lib"]),
        unmapped_test_imports: strings(&["example.com/check"]),
        ..PackageMapping::default()
    };
    for (name, src, old, want_labels, want_unmapped) in [
        (
            "a library wants what its sources need",
            r#"fake_library(name = "lib")"#,
            &[][..],
            &["//lib:lib"][..],
            &["example.com/lib"][..],
        ),
        (
            "a test on its own wants its library's deps and its own",
            r#"fake_test(name = "test")"#,
            &[],
            &["//lib:lib", "//check:check"],
            &["example.com/lib", "example.com/check"],
        ),
        (
            "a test with a target_under_test gets the library's deps through it",
            r#"fake_test(name = "test", target_under_test = ":lib")"#,
            &[],
            &["//check:check", "//lib:lib"],
            &["example.com/lib", "example.com/check"],
        ),
        (
            "a test with a same-package dep gets the library's deps through it",
            r#"fake_test(name = "test")"#,
            &[":lib"],
            &["//check:check", "//lib:lib"],
            &["example.com/lib", "example.com/check"],
        ),
    ] {
        // No platforms: the fake's Linux dep stays out of it
        let lang = FakeLanguage {
            mapping: mapping.clone(),
            ..FakeLanguage::default()
        };
        let pkg = open_fake(&lang, &[]);
        let w = fake_target(&pkg, &lang, src)
            .deps(&Configuration::default(), &strings(old))
            .unwrap();
        assert_eq!(w.labels, want_labels, "{name}: labels");
        assert_eq!(w.unmapped, want_unmapped, "{name}: unmapped");
    }
}

/// Deps on the package's own targets are dropped, whatever they are named:
/// a Python member's imports of its own modules map to it. A package nested
/// in it is another package.
#[test]
fn target_drops_self_reference() {
    let lang = FakeLanguage {
        mapping: PackageMapping {
            deps: vec![
                dep("//pkg:pkg"),
                dep("//pkg:lib"),
                dep("//pkg/sub:sub"),
                dep("//cfg:cfg"),
            ],
            ..PackageMapping::default()
        },
        ..FakeLanguage::default()
    };
    let pkg = open_fake(&lang, &fake_platforms());
    let lib = fake_target(&pkg, &lang, r#"fake_library(name = "lib")"#);
    assert_eq!(
        lib.deps(&macos(), &[]).unwrap().labels,
        ["//pkg/sub:sub", "//cfg:cfg"]
    );
}

/// Deps sync doesn't own are wanted as unsynced, not as labels, and every
/// resolution's reports are collected once each.
#[test]
fn target_unsynced_deps_and_reports() {
    let lang = FakeLanguage {
        mapping: PackageMapping {
            unmapped_imports: strings(&["example.com/x"]),
            unsynced_deps: vec![UnsyncedDep {
                dep: MappedDep {
                    target: "//build:build".into(),
                    import_path: "build".into(),
                    ..MappedDep::default()
                },
                reason: "build".into(),
            }],
            ..PackageMapping::default()
        },
        ..FakeLanguage::default()
    };
    let pkg = open_fake(&lang, &fake_platforms());
    pkg.resolve_all().unwrap();
    let w = fake_target(&pkg, &lang, r#"fake_library(name = "lib")"#)
        .deps(&macos(), &[])
        .unwrap();
    assert_eq!(w.unsynced, ["//build:build"]);
    assert_eq!(
        pkg.messages(),
        [
            "unmapped import: example.com/x",
            "build dependency build not synced"
        ]
    );
}

/// The attributes a language owns are those any configuration's resolution
/// sets, with their value in each.
#[test]
fn target_owned_attributes() {
    let lang = FakeLanguage {
        mapping: PackageMapping {
            attrs: BTreeMap::from([("features".into(), strings(&["default"]))]),
            ..PackageMapping::default()
        },
        ..FakeLanguage::default()
    };
    let pkg = open_fake(&lang, &fake_platforms());
    let lib = fake_target(&pkg, &lang, r#"fake_library(name = "lib")"#);
    assert_eq!(lib.owned().unwrap(), ["features"]);
    assert_eq!(lib.attr(&linux(), "features").unwrap(), ["default"]);
}

// --- typescript_test.go

/// A project whose js-deps.toml is in jsdeps-gen's format.
fn js_deps_fixture() -> (tempfile::TempDir, String) {
    let (dir, root) = temp_dir();
    write_tree(
        &root,
        &[
            ("package.json", "{}\n"),
            (
                "js-deps.toml",
                r#"[meta]
generator = "jsdeps-gen 0.1.0"

[[package]]
name = "lodash"
version = "4.17.21"
url = "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
integrity = "sha512-x"

[[package]]
name = "@types/lodash"
version = "4.17.23"
url = "https://registry.npmjs.org/@types%2flodash/-/lodash-4.17.23.tgz"
integrity = "sha512-y"

[[package]]
name = "@openzeppelin/contracts"
version = "5.4.0"
url = "https://registry.npmjs.org/@openzeppelin%2fcontracts/-/contracts-5.4.0.tgz"
integrity = "sha512-z"
"#,
            ),
        ],
    );
    (dir, root)
}

#[test]
fn map_typescript_imports() {
    let (_dir, root) = js_deps_fixture();
    let lang =
        typescript::TypeScriptLanguage::new(&test_config(&root), &test_language("javascript"));
    assert_eq!(
        lang.rule("typescript_library").unwrap().deps_attribute,
        "npm_deps"
    );
    let mut result = Extracted::new("typescript");
    result.add_package(ExtractedPackage {
        path: "app".into(),
        imports: vec![
            // unscoped, with a subpath
            import("lodash/fp", ImportKind::External),
            // scoped
            import("@openzeppelin/contracts/token", ImportKind::External),
            // relative: same target
            import("./util", ImportKind::Internal),
            import("left-pad", ImportKind::External),
        ],
        ..ExtractedPackage::default()
    });
    let mut mapping = resolve_imports(&lang, &result);
    mapping.deps = lang.with_types(mapping.deps);
    assert_eq!(
        deps_to_targets(&mapping.deps),
        [
            "jsdeps//:lodash",
            "jsdeps//:openzeppelin_contracts",
            // lodash's DefinitelyTyped package, which code never imports
            "jsdeps//:types_lodash",
        ]
    );
    assert_eq!(mapping.unmapped_imports, ["left-pad"]);
}

#[test]
fn types_packages() {
    for (pkg, want) in [
        ("lodash", "@types/lodash"),
        ("@openzeppelin/contracts", "@types/openzeppelin__contracts"),
        ("@types/node", "@types/node"),
    ] {
        assert_eq!(typescript::types_package(pkg), want, "{pkg}");
    }
}

// --- uv_test.go

/// A uv workspace whose members share the acme.* namespace: libs/cfg in a
/// flat layout, apps/app in a src/ layout, and two members that both
/// provide a top-level "tools" package.
fn uv_workspace_fixture() -> (tempfile::TempDir, String) {
    let (dir, root) = temp_dir();
    write_tree(
        &root,
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"acme\"\n\n[tool.uv.workspace]\nmembers = [\"libs/*\", \"apps/app\"]\n",
            ),
            ("python-deps.toml", "[deps.requests]\nversion = \"2.0\"\n"),
            (
                "libs/cfg/pyproject.toml",
                "[project]\nname = \"acme-cfg\"\n",
            ),
            ("libs/cfg/acme/cfg/__init__.py", ""),
            ("libs/cfg/acme/cfg/parser.py", ""),
            ("libs/cfg/tests/test_parser.py", ""),
            ("libs/cfg/tools/__init__.py", ""),
            (
                "libs/other/pyproject.toml",
                "[project]\nname = \"acme-other\"\n",
            ),
            ("libs/other/tools/__init__.py", ""),
            (
                "apps/app/pyproject.toml",
                "[project]\nname = \"acme-app\"\n",
            ),
            ("apps/app/src/acme/app/__init__.py", ""),
            ("apps/app/src/acme/app/main.py", ""),
            ("libs/not-a-member/acme/x/__init__.py", ""),
        ],
    );
    (dir, root)
}

#[test]
fn load_uv_workspace_modules() {
    let (_dir, root) = uv_workspace_fixture();
    let modules = uv::load_uv_workspace_modules(&root).unwrap();
    let want: std::collections::HashMap<String, String> = [
        ("acme.cfg", "libs/cfg"),
        ("acme.app", "apps/app"),
        ("tests", "libs/cfg"),
        // "tools" is provided by two members: ambiguous, left out
    ]
    .into_iter()
    .map(|(m, d)| (m.to_string(), d.to_string()))
    .collect();
    assert_eq!(modules, want);
}

#[test]
fn map_python_workspace_imports() {
    let (_dir, root) = uv_workspace_fixture();
    let lang = python::PythonLanguage::new(&test_config(&root), &test_language("python"));
    let mut result = Extracted::new("python");
    result.add_package(ExtractedPackage {
        path: "acme/app".into(),
        imports: vec![
            // dotted import of another member's module
            import("acme.cfg.parser", ImportKind::External),
            // from acme import cfg, as the extractor reports it
            import("acme.cfg", ImportKind::External),
            // the member's own module: maps to its own target, which sync
            // filters out, rather than being reported unmapped
            import("acme.app.main", ImportKind::External),
            // third party, unchanged
            import("requests.adapters", ImportKind::External),
            // ambiguous between two members
            import("tools", ImportKind::External),
        ],
        ..ExtractedPackage::default()
    });
    let mapping = resolve_imports(&lang, &result);
    assert_eq!(
        deps_to_targets(&mapping.deps),
        [
            "//apps/app:app",
            "//libs/cfg:cfg",
            "pydeps//vendor/requests:requests"
        ]
    );
    assert_eq!(mapping.unmapped_imports, ["tools"]);
}
