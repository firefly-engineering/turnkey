//! Each crates.io crate's package slice, as Cargo's feature resolver
//! resolves it (ADR 0006).
//!
//! A slice is what a crate's own rules.star is generated from: its features
//! and its normal dependencies resolved to `name@version`, each with the
//! platforms it applies on, and the name the crate's code uses for a
//! dependency when its package name wouldn't give it. The resolution is the
//! one `cargo test --workspace` compiles: `cargo tree` gives it, once per
//! configured platform. `cargo metadata` gives the names dependents use:
//! its graph is the lock file's, larger than what Cargo compiles (it keeps
//! optional dependencies that weak dependency features only name), but it
//! names every real edge.

use anyhow::{Context, Result, bail};
use cargo_lock::SourceId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A platform the slices are resolved for: its Buck2 name ("<os>-<cpu>", as
/// turnkey's platforms record names it) and its Rust target triple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub name: String,
    pub triple: String,
}

impl std::str::FromStr for Platform {
    type Err = anyhow::Error;

    /// "<name>=<triple>", as `--platform` takes it
    fn from_str(s: &str) -> Result<Self> {
        match s.split_once('=') {
            Some((name, triple)) if !name.is_empty() && !triple.is_empty() => Ok(Platform {
                name: name.to_string(),
                triple: triple.to_string(),
            }),
            _ => bail!(
                "--platform takes <name>=<triple>, e.g. macos-arm64=aarch64-apple-darwin, not {s:?}"
            ),
        }
    }
}

/// A crate's package slice. `platforms` is left out of a feature or a
/// dependency that applies on every platform the crate is built on.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Slice {
    pub features: Vec<Feature>,
    pub dependencies: Vec<Dependency>,
}

/// One feature Cargo enables on a crate
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Feature {
    pub name: String,
    /// The platforms it is enabled on, when not all the crate is built on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platforms: Option<BTreeSet<String>>,
}

/// One normal dependency of a crate
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dependency {
    /// The resolved package, "name@version"
    pub package: String,
    /// The name the crate's code uses for it, when that isn't its package
    /// name with `-` turned into `_`: a `package = ...` rename, or a library
    /// named differently from its package
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<String>,
    /// The platforms it applies on, when not all the crate is built on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platforms: Option<BTreeSet<String>>,
}

/// The slices of a workspace's crates.io crates, and its members' manifests
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Slices {
    /// Keyed "name@version"
    pub crates: BTreeMap<String, Slice>,
    /// Every workspace member's Cargo.toml: a change to any of them can
    /// change the slices
    pub manifests: BTreeSet<PathBuf>,
}

/// One platform's resolution, as `cargo tree` prints it: each package's
/// features and its dependencies, packages keyed "name@version"
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub features: BTreeMap<String, BTreeSet<String>>,
    pub dependencies: BTreeMap<String, BTreeSet<String>>,
}

/// The `cargo tree` output format `parse_tree` reads: each line is the
/// package's depth, then `{p}|{f}`
const TREE_FORMAT: &str = "{p}|{f}";

/// Resolve the workspace of `manifest` for one platform, as `cargo test
/// --workspace` would build it: normal and dev edges, no build scripts'.
/// Cargo runs in the manifest's directory, so the workspace's own
/// `.cargo/config.toml` applies. `--locked` keeps it from ever rewriting
/// Cargo.lock.
pub fn cargo_tree(manifest: &Path, platform: &Platform) -> Result<Tree> {
    let output = cargo(
        manifest,
        &[
            "tree",
            "--locked",
            "--workspace",
            "--target",
            &platform.triple,
            "--edges",
            "normal,dev",
            "--prefix",
            "depth",
            "--format",
            TREE_FORMAT,
        ],
    )
    .with_context(|| format!("cargo tree --target {}", platform.triple))?;
    parse_tree(&output)
}

/// What rustdeps-gen reads of `cargo metadata --format-version 1`
#[derive(Debug, Deserialize)]
pub struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    manifest_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
struct NodeDep {
    /// The name the dependent's code uses for it: its library's, or the
    /// rename
    name: String,
    pkg: String,
}

/// The workspace's lock file graph, for every platform
pub fn cargo_metadata(manifest: &Path) -> Result<Metadata> {
    let output = cargo(manifest, &["metadata", "--format-version", "1", "--locked"])
        .context("cargo metadata")?;
    serde_json::from_str(&output).context("Failed to parse cargo metadata's output")
}

/// Run cargo on the workspace of `manifest`, from the manifest's directory
fn cargo(manifest: &Path, args: &[&str]) -> Result<String> {
    let manifest = manifest
        .canonicalize()
        .with_context(|| format!("Failed to find {}", manifest.display()))?;
    let mut command = Command::new("cargo");
    command.args(args).arg("--manifest-path").arg(&manifest);
    if let Some(dir) = manifest.parent() {
        command.current_dir(dir);
    }
    let output = command.output().context("Failed to run cargo")?;
    if !output.status.success() {
        bail!("cargo failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).context("cargo's output is not UTF-8")
}

/// Read `cargo tree --prefix depth --format '{p}|{f}'`.
///
/// Each non-empty line is: the depth (decimal digits), the package (`{p}`:
/// its name, a space, `v` and its version, then optional parenthesised
/// notes such as `(proc-macro)` or a non-crates.io source), `|`, and its
/// features (`{f}`: comma-separated, possibly none). A package whose
/// dependencies were printed earlier ends in ` (*)`. A line's package is a
/// dependency of the nearest line above it one level shallower. Blank lines
/// separate the workspace members' trees. A package appearing more than
/// once (built for the host and the target, say) gets the union.
pub fn parse_tree(output: &str) -> Result<Tree> {
    let mut tree = Tree::default();
    // The package at each depth on the current path from the root
    let mut path: Vec<String> = Vec::new();

    for (number, line) in output.lines().enumerate() {
        if line.is_empty() {
            path.clear();
            continue;
        }
        let context = || format!("cargo tree output, line {}: {line:?}", number + 1);
        let line = TreeLine::parse(line).with_context(context)?;
        if line.depth > path.len() {
            bail!("{}: deeper than the line above allows", context());
        }
        path.truncate(line.depth);
        if let Some(parent) = path.last() {
            tree.dependencies
                .entry(parent.clone())
                .or_default()
                .insert(line.package.clone());
        }
        tree.features
            .entry(line.package.clone())
            .or_default()
            .extend(line.features);
        tree.dependencies.entry(line.package.clone()).or_default();
        path.push(line.package);
    }
    Ok(tree)
}

/// One line of `cargo tree --prefix depth --format '{p}|{f}'`
struct TreeLine {
    depth: usize,
    /// "name@version"
    package: String,
    features: Vec<String>,
}

impl TreeLine {
    fn parse(line: &str) -> Result<TreeLine> {
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        let depth = line[..digits].parse().context("no depth")?;
        let (package, features) = line[digits..].split_once('|').context("no `|`")?;
        let features = features.strip_suffix(" (*)").unwrap_or(features);

        let mut words = package.split(' ');
        let name = words
            .next()
            .filter(|n| !n.is_empty())
            .context("no package name")?;
        let version = words
            .next()
            .and_then(|v| v.strip_prefix('v'))
            .context("no package version")?;
        Ok(TreeLine {
            depth,
            package: format!("{name}@{version}"),
            features: features
                .split(',')
                .filter(|f| !f.is_empty())
                .map(String::from)
                .collect(),
        })
    }
}

/// Merge each platform's resolution into each crates.io crate's slice, with
/// the names dependents use from the lock file graph.
pub fn merge(trees: &[(Platform, Tree)], metadata: &Metadata) -> Result<Slices> {
    let packages: HashMap<&str, &Package> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let package = |id: &str| {
        packages
            .get(id)
            .copied()
            .with_context(|| format!("cargo metadata: no package {id}"))
    };

    let mut slices = Slices::default();
    for member in &metadata.workspace_members {
        slices
            .manifests
            .insert(package(member)?.manifest_path.clone());
    }

    let crates_io: BTreeSet<String> = metadata
        .packages
        .iter()
        .filter(|p| is_crates_io(p))
        .map(|p| p.key())
        .collect();

    // What each dependent's code calls each of its dependencies
    let mut names: HashMap<(String, String), &str> = HashMap::new();
    for node in &metadata.resolve.nodes {
        let dependent = package(&node.id)?.key();
        for dep in &node.deps {
            names.insert((dependent.clone(), package(&dep.pkg)?.key()), &dep.name);
        }
    }

    // Per crate: the platforms building it, and each feature's and each
    // dependency's
    type ByPlatform<K> = BTreeMap<K, BTreeSet<String>>;
    let mut built: ByPlatform<String> = BTreeMap::new();
    let mut features: BTreeMap<String, ByPlatform<String>> = BTreeMap::new();
    let mut dependencies: BTreeMap<String, ByPlatform<String>> = BTreeMap::new();
    for (platform, tree) in trees {
        for (key, crate_features) in &tree.features {
            if !crates_io.contains(key) {
                continue;
            }
            built
                .entry(key.clone())
                .or_default()
                .insert(platform.name.clone());
            for feature in crate_features {
                features
                    .entry(key.clone())
                    .or_default()
                    .entry(feature.clone())
                    .or_default()
                    .insert(platform.name.clone());
            }
            for dep in &tree.dependencies[key] {
                dependencies
                    .entry(key.clone())
                    .or_default()
                    .entry(dep.clone())
                    .or_default()
                    .insert(platform.name.clone());
            }
        }
    }

    for (key, platforms) in built {
        let only = |on: BTreeSet<String>| (on != platforms).then_some(on);
        let slice = Slice {
            features: features
                .remove(&key)
                .unwrap_or_default()
                .into_iter()
                .map(|(name, on)| Feature {
                    name,
                    platforms: only(on),
                })
                .collect(),
            dependencies: dependencies
                .remove(&key)
                .unwrap_or_default()
                .into_iter()
                .map(|(dep, on)| {
                    let name = names.get(&(key.clone(), dep.clone())).with_context(|| {
                        format!(
                            "cargo metadata has no edge from {key} to {dep}, which cargo tree has"
                        )
                    })?;
                    let crate_name = dep.split('@').next().unwrap_or(&dep).replace('-', "_");
                    Ok(Dependency {
                        rename: (*name != crate_name).then(|| name.to_string()),
                        package: dep,
                        platforms: only(on),
                    })
                })
                .collect::<Result<_>>()?,
        };
        slices.crates.insert(key, slice);
    }
    Ok(slices)
}

fn is_crates_io(pkg: &Package) -> bool {
    pkg.source
        .as_deref()
        .and_then(|s| SourceId::from_url(s).ok())
        .is_some_and(|s| s.is_default_registry())
}

impl Package {
    fn key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_tree_lines_into_features_and_edges() {
        let output = "\
0app v0.1.0 (/ws/app)|
1gamma v1.0.0|
2beta v1.0.0|
1serde_derive v1.0.0 (proc-macro)|default
2proc-macro2 v1.0.0|default,proc-macro
1beta v1.0.0|std (*)

0tool v0.1.0 (/ws/tool)|
1proc-macro2 v1.0.0|default (*)
";
        let tree = parse_tree(output).unwrap();

        assert_eq!(tree.features["beta@1.0.0"], set(&["std"]));
        assert_eq!(
            tree.features["proc-macro2@1.0.0"],
            set(&["default", "proc-macro"])
        );
        assert_eq!(tree.features["serde_derive@1.0.0"], set(&["default"]));
        assert_eq!(
            tree.dependencies["app@0.1.0"],
            set(&["gamma@1.0.0", "serde_derive@1.0.0", "beta@1.0.0"])
        );
        assert_eq!(tree.dependencies["gamma@1.0.0"], set(&["beta@1.0.0"]));
        assert_eq!(tree.dependencies["tool@0.1.0"], set(&["proc-macro2@1.0.0"]));
        assert_eq!(tree.dependencies["beta@1.0.0"], set(&[]));
    }

    #[test]
    fn refuses_lines_it_cannot_read() {
        assert!(parse_tree("0app v0.1.0\n").is_err());
        assert!(parse_tree("app v0.1.0|\n").is_err());
        assert!(parse_tree("0app v0.1.0|\n2beta v1.0.0|\n").is_err());
    }

    #[test]
    fn parses_platforms() {
        assert_eq!(
            "macos-arm64=aarch64-apple-darwin"
                .parse::<Platform>()
                .unwrap(),
            Platform {
                name: "macos-arm64".into(),
                triple: "aarch64-apple-darwin".into()
            }
        );
        assert!("aarch64-apple-darwin".parse::<Platform>().is_err());
    }

    /// A workspace whose crates.io dependencies are vendored in a directory
    /// source, so cargo resolves it with no network
    fn fixture(root: &Path) {
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n",
        );
        write(
            ".cargo/config.toml",
            "[source.crates-io]\nreplace-with = \"fixture\"\n\n[source.fixture]\ndirectory = \"vendor\"\n",
        );
        write(
            "app/Cargo.toml",
            r#"[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
weak = { version = "1", features = ["std"] }
gamma = "1"

[dev-dependencies]
alpha = { version = "1", features = ["dev"] }
"#,
        );
        write("app/src/lib.rs", "");

        let vendored = |name: &str, rest: &str| {
            write(
                &format!("vendor/{name}-1.0.0/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n{rest}"
                ),
            );
            write(&format!("vendor/{name}-1.0.0/src/lib.rs"), "");
            write(
                &format!("vendor/{name}-1.0.0/.cargo-checksum.json"),
                &format!(r#"{{"files":{{}},"package":"{}"}}"#, "0".repeat(64)),
            );
        };
        // A weak dependency feature: it names `opt` without enabling it
        vendored(
            "weak",
            "\n[dependencies]\nopt = { version = \"1\", optional = true }\n\n[features]\nstd = [\"opt?/std\"]\n",
        );
        vendored("opt", "\n[features]\nstd = []\n");
        vendored(
            "gamma",
            r#"
[dependencies]
b1 = { package = "beta", version = "1" }
pkgname = "1"

[target.'cfg(target_os = "linux")'.dependencies]
alpha = { version = "1", features = ["linuxfeat"] }

[target.'cfg(windows)'.dependencies]
winonly = "1"

[build-dependencies]
builddep = "1"
"#,
        );
        vendored(
            "alpha",
            "\n[features]\ndefault = []\nlinuxfeat = []\ndev = []\n",
        );
        vendored("beta", "");
        // A library named differently from its package
        vendored("pkgname", "\n[lib]\nname = \"libname\"\n");
        vendored("winonly", "");
        vendored("builddep", "");
    }

    fn feature(name: &str, platforms: Option<&[&str]>) -> Feature {
        Feature {
            name: name.into(),
            platforms: platforms.map(set),
        }
    }

    fn dep(package: &str, rename: Option<&str>, platforms: Option<&[&str]>) -> Dependency {
        Dependency {
            package: package.into(),
            rename: rename.map(Into::into),
            platforms: platforms.map(set),
        }
    }

    /// The slices of a vendored fixture workspace, as cargo resolves it. This
    /// pins both how rustdeps-gen reads cargo and cargo's output format. It
    /// runs cargo and rustc, which Buck2's tests don't get (their PATH is
    /// fixed): the Nix build runs it, with its pinned cargo
    /// (nix/packages/rustdeps-gen.nix).
    #[test]
    #[ignore = "runs cargo: the Nix build runs it"]
    fn slices_of_a_fixture_workspace_follow_cargos_feature_resolver() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let manifest = dir.path().join("Cargo.toml");
        cargo(&manifest, &["generate-lockfile", "--offline"]).unwrap();

        let platforms: Vec<Platform> = [
            "linux-x86_64=x86_64-unknown-linux-gnu",
            "macos-arm64=aarch64-apple-darwin",
        ]
        .iter()
        .map(|p| p.parse().unwrap())
        .collect();
        let trees: Vec<(Platform, Tree)> = platforms
            .iter()
            .map(|p| (p.clone(), cargo_tree(&manifest, p).unwrap()))
            .collect();
        let slices = merge(&trees, &cargo_metadata(&manifest).unwrap()).unwrap();

        let linux = Some(&["linux-x86_64"][..]);
        let expected: BTreeMap<String, Slice> = [
            (
                "alpha@1.0.0",
                Slice {
                    // Its dev-dependency features count; linuxfeat comes
                    // only through gamma's Linux-only dependency
                    features: vec![
                        feature("default", None),
                        feature("dev", None),
                        feature("linuxfeat", linux),
                    ],
                    dependencies: vec![],
                },
            ),
            ("beta@1.0.0", Slice::default()),
            (
                "gamma@1.0.0",
                Slice {
                    features: vec![],
                    // No winonly on these platforms, no build-dependency
                    dependencies: vec![
                        dep("alpha@1.0.0", None, linux),
                        dep("beta@1.0.0", Some("b1"), None),
                        dep("pkgname@1.0.0", Some("libname"), None),
                    ],
                },
            ),
            ("pkgname@1.0.0", Slice::default()),
            (
                "weak@1.0.0",
                Slice {
                    // No opt: a weak dependency feature doesn't enable it,
                    // though the lock file graph has the edge
                    features: vec![feature("std", None)],
                    dependencies: vec![],
                },
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        assert_eq!(slices.crates, expected);
        assert_eq!(
            slices.manifests,
            [dir.path().join("app/Cargo.toml").canonicalize().unwrap()].into()
        );
    }
}
