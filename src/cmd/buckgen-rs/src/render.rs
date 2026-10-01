//! A Go module's rules.star files, one per Go package
//!
//! A package's deps are resolved in every configuration: on each platform,
//! and for each combination of the allowed build tags its files'
//! constraints use. Deps every configuration has are a plain list; the
//! others are a select() keyed as the conditions crate keys them, with no
//! DEFAULT.

use crate::config::Config;
use conditions::conditional::labels_value;
use conditions::{Configuration, Space};
use deps_gen_kit::starlark::render_indented;
use goparse::{DirEntry, GoPackage, Visit, config_context, scan_package, tag_dimension, walk_dir};
use gostd::path::{base, join, rel};
use gostd::strconv::quote;
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;

/// The rules.star of a Go package, and the non-stdlib import paths its
/// deps reference in any configuration (sorted), or `None` if no
/// configuration builds the package (e.g. a Windows-only package)
pub fn render_package(pkg: &GoPackage, cfg: &Config) -> Option<(String, Vec<String>)> {
    let space = package_space(pkg, cfg);
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut referenced = BTreeSet::new();
    let mut built = false;
    for config in &space.configurations {
        let imports = pkg.imports(&build_context(config, cfg));
        built |= imports.is_some();
        let mut targets = Vec::new();
        for imp in imports.unwrap_or_default() {
            // A package importing itself is skipped; its parent is a real dep
            if is_std_lib(&imp) || imp == pkg.import_path {
                continue;
            }
            targets.push(import_to_target(&imp, cfg));
            referenced.insert(imp);
        }
        deps.insert(config.to_string(), targets);
    }
    if !built {
        return None;
    }

    let mut out = String::new();
    if !cfg.buck.preambule.is_empty() {
        out.push_str(&cfg.buck.preambule);
        out.push_str("\n\n");
    }
    // The directory name (the import path's last component) is the target
    // name, so deps can name a target without knowing its package name
    // (github.com/pelletier/go-toml/v2 is "v2")
    let target_name = base(&pkg.import_path);
    out.push_str(&format!("{}(\n", cfg.buck.go_library_rule));
    out.push_str(&format!("    name = {},\n", quote(&target_name)));
    out.push_str(&format!(
        "    package_name = {},\n",
        quote(&pkg.import_path)
    ));
    // Go, assembly, and C/C++ sources can be part of a Go package
    out.push_str(
        "    srcs = native.glob([\"*.go\", \"*.s\", \"*.h\", \"*.c\", \"*.cc\", \"*.cpp\", \"*.S\"]),\n",
    );
    out.push_str("    header_namespace = \"\",\n");
    out.push_str("    visibility = [\"PUBLIC\"],\n");
    let value = labels_value(&space, |config: &Configuration| {
        deps.get(&config.to_string()).cloned().unwrap_or_default()
    });
    if let Some(value) = value {
        out.push_str(&format!(
            "    {} = {},\n",
            cfg.buck.deps_attr,
            render_indented(&value, "    ")
        ));
    }
    out.push_str(")\n");
    Some((out, referenced.into_iter().collect()))
}

/// The configurations a package's deps are resolved for: every platform,
/// crossed with each allowed build tag its files' constraints use
fn package_space(pkg: &GoPackage, cfg: &Config) -> Space {
    let mut dims = Vec::new();
    for f in &pkg.files {
        for tag in f.constraint_tags() {
            if cfg.conditions.go_tags.contains(&tag) {
                dims.push(tag_dimension(&tag));
            }
        }
    }
    Space::new(&cfg.conditions.platforms, &cfg.conditions.settings).with_dimensions(&dims)
}

/// The Go build of a configuration, with the toolchain's release tags
fn build_context(config: &Configuration, cfg: &Config) -> goparse::BuildContext {
    let (mut ctx, _) = config_context(config);
    ctx.go_version = cfg.go_version.clone();
    ctx
}

/// A Go package [`render_module`] wrote a rules.star for
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPackage {
    /// Its directory in the module ("." for the module's root)
    pub subdir: String,
    /// Its target
    pub target: String,
}

/// Writes a rules.star for each Go package of the module in `module_dir`,
/// whose module path is `module_path`, as the go command sees its
/// packages: directories named testdata, or starting with "." or "_", hold
/// none. Returns the packages it rendered, in walk order, and the
/// non-stdlib import paths their deps reference, sorted.
pub fn render_module(
    module_dir: &str,
    module_path: &str,
    cfg: &Config,
) -> io::Result<(Vec<RenderedPackage>, Vec<String>)> {
    let mut rendered = Vec::new();
    let mut referenced = BTreeSet::new();
    walk_dir(module_dir, &mut |path: &str,
                               entry: &DirEntry|
     -> io::Result<Visit> {
        if !entry.is_dir {
            return Ok(Visit::Continue);
        }
        let rel = rel(module_dir, path).map_err(io::Error::other)?;
        let mut import_path = module_path.to_string();
        if rel != "." {
            let name = entry.name.as_str();
            if name == "testdata" || name.starts_with('.') || name.starts_with('_') {
                return Ok(Visit::SkipDir);
            }
            import_path = format!("{import_path}/{rel}");
        }

        let Ok(Some(pkg)) = scan_package(path, &import_path) else {
            // Not a Go package
            return Ok(Visit::Continue);
        };
        // Unless no configuration builds the package
        let Some((content, imports)) = render_package(&pkg, cfg) else {
            return Ok(Visit::Continue);
        };
        write_file(&join(&[path, &cfg.buck.buildfile_name]), content.as_bytes())?;
        rendered.push(RenderedPackage {
            subdir: rel,
            target: base(&import_path),
        });
        referenced.extend(imports);
        Ok(Visit::Continue)
    })?;
    Ok((rendered, referenced.into_iter().collect()))
}

/// `os.WriteFile` with mode 0644
pub fn write_file(path: &str, data: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(path)?
        .write_all(data)
}

/// Whether an import path is the standard library's (or cgo's "C"): its
/// first element has no dot
fn is_std_lib(import_path: &str) -> bool {
    import_path == "C" || !import_path.split('/').next().unwrap_or("").contains('.')
}

/// The label of an imported package in the deps cell: its directory name
/// (the import path's last component) is its target name, as
/// [`render_package`] names it
fn import_to_target(import_path: &str, cfg: &Config) -> String {
    let name = import_path.rsplit('/').next().unwrap_or("");
    format!("{}{import_path}:{name}", cfg.buck.deps_target_label_prefix)
}

#[cfg(test)]
mod tests {
    //! src/go/pkg/buckgen's tests

    use super::*;
    use crate::config::{BuckConfig, ConditionsConfig};
    use conditions::Platform;
    use goparse::GoFile;
    use std::fs;
    use std::path::Path;

    /// buckgen's configuration for turnkey's default platforms
    fn test_config() -> Config {
        Config {
            buck: BuckConfig {
                go_library_rule: "go_library".into(),
                deps_target_label_prefix: "godeps//".into(),
                deps_attr: "deps".into(),
                buildfile_name: "rules.star".into(),
                ..BuckConfig::default()
            },
            conditions: ConditionsConfig {
                settings: "toolchains//conditions".into(),
                platforms: [
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
                .collect(),
                go_tags: vec!["integration".into()],
            },
            go_version: "1.24".into(),
        }
    }

    fn file(path: &str, build: &str, imports: &[&str]) -> GoFile {
        GoFile {
            path: path.into(),
            imports: imports.iter().map(|i| i.to_string()).collect(),
            constraint: (!build.is_empty()).then(|| {
                gostd::constraint::parse(&format!("//go:build {build}")).expect("a constraint")
            }),
            ..GoFile::default()
        }
    }

    fn package(import_path: &str, files: Vec<GoFile>) -> GoPackage {
        GoPackage {
            import_path: import_path.into(),
            files,
            ..GoPackage::default()
        }
    }

    fn render(pkg: &GoPackage) -> String {
        render_package(pkg, &test_config())
            .expect("package built on some platform")
            .0
    }

    #[test]
    fn render_package_per_os() {
        let pkg = package(
            "github.com/example/testpkg",
            vec![
                file("common.go", "", &["fmt", "github.com/example/common"]),
                file("x_linux.go", "", &["github.com/example/linuxonly"]),
                file("x_darwin.go", "", &["github.com/example/maconly"]),
            ],
        );
        let want = r#"go_library(
    name = "testpkg",
    package_name = "github.com/example/testpkg",
    srcs = native.glob(["*.go", "*.s", "*.h", "*.c", "*.cc", "*.cpp", "*.S"]),
    header_namespace = "",
    visibility = ["PUBLIC"],
    deps = ["godeps//github.com/example/common:common"] + select({
        "config//os:linux": ["godeps//github.com/example/linuxonly:linuxonly"],
        "config//os:macos": ["godeps//github.com/example/maconly:maconly"],
    }),
)
"#;
        assert_eq!(render(&pkg), want);
    }

    /// A //go:build unix file is included on Linux and macOS, and a go1.21
    /// file with a newer toolchain: their imports are common
    #[test]
    fn render_package_unix_and_release_tags() {
        let pkg = package(
            "github.com/example/sys",
            vec![
                file("unix.go", "unix", &["github.com/example/unixdep"]),
                file("new.go", "go1.21", &["github.com/example/newdep"]),
                file("future.go", "go1.99", &["github.com/example/futuredep"]),
            ],
        );
        let got = render(&pkg);
        let want = r#"    deps = [
        "godeps//github.com/example/newdep:newdep",
        "godeps//github.com/example/unixdep:unixdep",
    ],
"#;
        assert!(got.contains(want), "{got}");
    }

    /// A linux/arm64-only import is keyed on the combined OS-and-CPU
    /// setting, not on a duplicated OS key, and there is no DEFAULT
    #[test]
    fn render_package_linux_arm64_only() {
        let pkg = package(
            "github.com/example/cpu",
            vec![
                file("cpu.go", "", &[]),
                file("cpu_linux_arm64.go", "", &["github.com/example/armdep"]),
            ],
        );
        let got = render(&pkg);
        let want = r#"    deps = select({
        "toolchains//conditions:linux-arm64": ["godeps//github.com/example/armdep:armdep"],
        "toolchains//conditions:linux-x86_64": [],
        "toolchains//conditions:macos-arm64": [],
        "toolchains//conditions:macos-x86_64": [],
    }),
"#;
        assert!(got.contains(want) && !got.contains("DEFAULT"), "{got}");
    }

    /// An allowed tag is a dimension; a tag that isn't allowed is never set
    #[test]
    fn render_package_tags() {
        let pkg = package(
            "github.com/example/tagged",
            vec![
                file("it.go", "integration", &["github.com/example/harness"]),
                file("other.go", "othertag", &["github.com/example/never"]),
                file("base.go", "", &[]),
            ],
        );
        let got = render(&pkg);
        let want = r#"    deps = select({
        "prelude//go/tags/constraints:integration[set]": ["godeps//github.com/example/harness:harness"],
        "prelude//go/tags/constraints:integration[unset]": [],
    }),
"#;
        assert!(got.contains(want), "{got}");
    }

    /// Go lets a package import its parent (cobra/doc imports cobra):
    /// that's a dep, not a self-reference
    #[test]
    fn render_package_imports_parent() {
        let pkg = package(
            "github.com/example/parent/child",
            vec![file(
                "child.go",
                "",
                &[
                    "github.com/example/parent",
                    "github.com/example/parent/child",
                ],
            )],
        );
        let got = render(&pkg);
        assert!(
            got.contains("    deps = [\"godeps//github.com/example/parent:parent\"],\n"),
            "{got}"
        );
    }

    #[test]
    fn render_package_no_deps() {
        let pkg = package(
            "github.com/example/nodeps",
            vec![file("a.go", "", &["fmt", "os"])],
        );
        assert!(!render(&pkg).contains("deps ="));
    }

    /// A package no platform builds gets no rules.star
    #[test]
    fn render_package_not_built() {
        let pkg = package(
            "github.com/example/win",
            vec![file("w_windows.go", "", &[])],
        );
        assert_eq!(render_package(&pkg, &test_config()), None);
    }

    /// The preamble comes first, then a blank line
    #[test]
    fn render_package_preamble() {
        let mut cfg = test_config();
        cfg.buck.preambule = "# Auto-generated by turnkey buckgen\n".into();
        let pkg = package("example.com/a", vec![file("a.go", "", &[])]);
        let (got, _) = render_package(&pkg, &cfg).unwrap();
        assert!(
            got.starts_with("# Auto-generated by turnkey buckgen\n\n\ngo_library(\n"),
            "{got}"
        );
    }

    fn write_files(dir: &Path, files: &[(&str, &str)]) {
        for (name, content) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn render_module_writes_each_package() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                ("go.mod", "module example.com/mod\n"),
                (
                    "mod.go",
                    "package mod\n\nimport (\n\t\"fmt\"\n\t\"example.com/other/x\"\n)\n",
                ),
                (
                    "sub/sub.go",
                    "package sub\n\nimport \"example.com/mod\"\nimport \"example.com/other/y\"\n",
                ),
                (
                    "win/win.go",
                    "//go:build windows\n\npackage win\n\nimport \"example.com/never\"\n",
                ),
                ("docs/README.md", "not a package\n"),
                (
                    "testdata/t/t.go",
                    "package t\n\nimport \"example.com/testonly\"\n",
                ),
                ("_examples/e/e.go", "package e\n"),
                (
                    "internal/deep/d.go",
                    "package deep\n\nimport \"example.com/other/x\"\n",
                ),
                (
                    "sub/sub_test.go",
                    "package sub\n\nimport \"example.com/testdep\"\n",
                ),
                (".hidden/h/h.go", "package h\n"),
                ("internal/deep/d.s", "#include \"textflag.h\"\n"),
                ("internal/deep/doc.h", "\n"),
            ],
        );
        let dir = tmp.path().to_str().unwrap();
        let (rendered, imports) = render_module(dir, "example.com/mod", &test_config()).unwrap();
        let pkg = |subdir: &str, target: &str| RenderedPackage {
            subdir: subdir.into(),
            target: target.into(),
        };
        assert_eq!(
            rendered,
            [
                pkg(".", "mod"),
                pkg("internal/deep", "deep"),
                pkg("sub", "sub")
            ]
        );
        // sub's import of its parent is a dep; test-only and unbuilt
        // packages reference nothing
        assert_eq!(
            imports,
            [
                "example.com/mod",
                "example.com/other/x",
                "example.com/other/y"
            ]
        );

        let sub = fs::read_to_string(tmp.path().join("sub/rules.star")).unwrap();
        for want in [
            r#"package_name = "example.com/mod/sub""#,
            r#""godeps//example.com/mod:mod""#,
            r#""godeps//example.com/other/y:y""#,
        ] {
            assert!(sub.contains(want), "sub/rules.star: {sub}");
        }
        for unrendered in ["win", "docs", "testdata/t", "_examples/e", ".hidden/h"] {
            assert!(
                !tmp.path().join(unrendered).join("rules.star").exists(),
                "{unrendered} rendered"
            );
        }
        assert!(render_module(&format!("{dir}/missing"), "x", &test_config()).is_err());
    }

    #[test]
    fn std_lib_and_targets() {
        assert!(is_std_lib("C") && is_std_lib("fmt") && is_std_lib("net/http"));
        assert!(!is_std_lib("example.com/x") && !is_std_lib("golang.org/x/mod"));
        assert_eq!(
            import_to_target("github.com/pelletier/go-toml/v2", &test_config()),
            "godeps//github.com/pelletier/go-toml/v2:v2"
        );
    }
}
