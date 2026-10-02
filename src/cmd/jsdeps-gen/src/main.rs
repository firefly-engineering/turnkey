//! jsdeps-gen: Generate js-deps.toml from pnpm-lock.yaml
//!
//! This tool parses pnpm-lock.yaml and generates a TOML file with package
//! information for use with turnkey's JavaScript deps cell (nix/buck2/languages.nix).
//!
//! pnpm lockfiles contain integrity hashes (SHA512) which we convert to
//! the format expected by Nix's fetchurl with SRI hashes.
//!
//! Besides the `[[package]]` records (a package's contents, one per
//! `name@version`), it records each pnpm snapshot as an `[[instance]]`, its
//! dependencies resolved to instance keys, and the root package.json's
//! resolution as `[direct]` (docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md).

/// Package version from VERSION.txt (works with both Cargo and Buck2)
const VERSION: &str = {
    // include_str! is relative to the source file location
    // From src/main.rs, VERSION.txt is at ../VERSION.txt
    const V: &str = include_str!("../VERSION.txt");
    // Trim trailing newline at compile time by taking a slice
    // VERSION.txt contains "0.1.0\n", we want "0.1.0"
    const fn trim_newline(s: &str) -> &str {
        let bytes = s.as_bytes();
        let mut end = bytes.len();
        while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
            end -= 1;
        }
        // SAFETY: We're trimming ASCII whitespace, so UTF-8 validity is preserved
        unsafe { std::str::from_utf8_unchecked(bytes.split_at(end).0) }
    }
    trim_newline(V)
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use deps_gen_kit::OutputArgs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

mod key;

/// Generate js-deps.toml from pnpm-lock.yaml for Buck2/Nix integration
#[derive(Parser, Debug)]
#[command(name = "jsdeps-gen")]
#[command(about = "Generate js-deps.toml from pnpm-lock.yaml")]
struct Args {
    /// Path to pnpm-lock.yaml file
    #[arg(long, default_value = "pnpm-lock.yaml")]
    lock: PathBuf,

    #[command(flatten)]
    output: OutputArgs,

    /// Include dev dependencies
    #[arg(long, default_value = "false")]
    include_dev: bool,
}

/// Represents a package in the output TOML
#[derive(Debug, Serialize)]
struct OutputPackage {
    name: String,
    version: String,
    /// NPM tarball URL
    url: String,
    /// SRI hash (sha512-...)
    integrity: String,
    /// Dependencies of this package, by name: the union over all its
    /// instances
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
    /// Optional dependencies of this package: installed only where their
    /// own os/cpu/libc allow, like esbuild's per-platform binaries
    #[serde(skip_serializing_if = "Vec::is_empty")]
    optional_dependencies: Vec<String>,
    /// The operating systems the package installs on, as npm names them
    /// (e.g. "darwin", or "!win32" to exclude one); empty for any
    #[serde(skip_serializing_if = "Vec::is_empty")]
    os: Vec<String>,
    /// The CPUs the package installs on (e.g. "x64", "arm64"); empty for any
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cpu: Vec<String>,
    /// The C libraries the package installs with on Linux (e.g. "glibc",
    /// "musl"); empty for any
    #[serde(skip_serializing_if = "Vec::is_empty")]
    libc: Vec<String>,
}

/// A package instance in the output TOML: one pnpm snapshot, as pnpm
/// resolved it (ADR 0012)
#[derive(Debug, Serialize)]
struct OutputInstance {
    /// The snapshot key verbatim, peer and patch groups included
    key: String,
    /// The name the package imports each dependency as, to the instance
    /// key it resolves to
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    dependencies: BTreeMap<String, String>,
    /// Likewise for optional dependencies
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    optional_dependencies: BTreeMap<String, String>,
}

/// pnpm lockfile structure (v9+)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmLockfile {
    lockfile_version: String,
    /// The workspace's package.json files, by path; `.` is the root one
    #[serde(default)]
    importers: BTreeMap<String, PnpmImporter>,
    #[serde(default)]
    packages: BTreeMap<String, PnpmPackage>,
    /// Snapshots section (pnpm v9+): each package instance's dependencies
    #[serde(default)]
    snapshots: BTreeMap<String, PnpmSnapshot>,
}

/// An importer in pnpm-lock.yaml: what one package.json resolves to
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmImporter {
    #[serde(default)]
    dependencies: BTreeMap<String, PnpmDepRef>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, PnpmDepRef>,
    #[serde(default)]
    dev_dependencies: BTreeMap<String, PnpmDepRef>,
}

/// Package entry in pnpm-lock.yaml
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmPackage {
    resolution: Option<PnpmResolution>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    dev: bool,
    #[serde(default)]
    os: Vec<String>,
    #[serde(default)]
    cpu: Vec<String>,
    #[serde(default)]
    libc: Vec<String>,
}

/// Snapshot entry in pnpm-lock.yaml (v9+): one instance of a package, keyed
/// like its package entry plus any peer or patch groups, and the instance
/// each of its dependencies resolves to
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmSnapshot {
    #[serde(default)]
    dependencies: BTreeMap<String, PnpmDepRef>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, PnpmDepRef>,
}

/// A dependency's resolution: a bare value in a snapshot, or an importer's
/// `{specifier, version}`
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PnpmDepRef {
    Simple(String),
    Complex { version: String },
}

impl PnpmDepRef {
    /// The resolved value: a version, an alias's key, or a `link:`/`file:`
    fn value(&self) -> &str {
        match self {
            PnpmDepRef::Simple(value) | PnpmDepRef::Complex { version: value } => value,
        }
    }
}

/// Resolution info for a package
#[derive(Debug, Deserialize)]
struct PnpmResolution {
    integrity: Option<String>,
    tarball: Option<String>,
}

/// Output TOML structure
#[derive(Debug, Serialize)]
struct OutputToml {
    /// Generator metadata
    meta: OutputMeta,
    /// Packages map
    #[serde(rename = "package")]
    packages: Vec<OutputPackage>,
    /// Package instances, sorted by key
    #[serde(rename = "instance", skip_serializing_if = "Vec::is_empty")]
    instances: Vec<OutputInstance>,
    /// The root package.json's dependencies, by bare npm name, to the
    /// instance each resolves to
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    direct: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct OutputMeta {
    generator: String,
    lockfile_version: String,
}

/// Generate NPM tarball URL for a package
fn npm_tarball_url(name: &str, version: &str) -> String {
    // Scoped packages have a different URL structure
    if name.starts_with('@') {
        let encoded_name = name.replace('/', "%2f");
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            encoded_name,
            name.split('/').next_back().unwrap_or(name),
            version
        )
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, version
        )
    }
}

/// Whether a lockfile has snapshots and importers as pnpm v9 writes them:
/// before v9, packages were keyed `/name@version` and importers' versions
/// weren't instance keys
fn has_instances(lockfile_version: &str) -> bool {
    lockfile_version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 9)
}

/// Resolves each of `deps` (import name to value) to an instance key, which
/// must be a snapshot; `from` names the dependent in errors
fn resolve_deps(
    lockfile: &PnpmLockfile,
    from: &str,
    deps: &BTreeMap<String, PnpmDepRef>,
) -> Result<BTreeMap<String, String>> {
    let mut resolved = BTreeMap::new();
    for (import_name, dep) in deps {
        let value = dep.value();
        let to = key::resolve(import_name, value)
            .with_context(|| format!("{from} depends on `{import_name}`: `{value}`"))?;
        if !lockfile.snapshots.contains_key(&to) {
            bail!(
                "{from} depends on `{import_name}` as `{to}`, which pnpm-lock.yaml has no snapshot of"
            );
        }
        resolved.insert(import_name.clone(), to);
    }
    Ok(resolved)
}

/// The dependency names that every instance of each package (`name`,
/// `version`) imports, plain and optional
type DepNames<'a> = BTreeMap<(&'a str, &'a str), (BTreeSet<String>, BTreeSet<String>)>;

/// Builds js-deps.toml's contents from a parsed lockfile
fn generate(lockfile: &PnpmLockfile, include_dev: bool) -> Result<OutputToml> {
    let with_instances = has_instances(&lockfile.lockfile_version);
    if !with_instances {
        eprintln!(
            "warning: lockfile version {} predates pnpm v9: no [[instance]] or [direct] written",
            lockfile.lockfile_version
        );
    }

    // Instances: one per snapshot, as pnpm resolved it
    let mut instances: Vec<OutputInstance> = Vec::new();
    let mut dep_names: DepNames = BTreeMap::new();
    if with_instances {
        for (snapshot_key, snapshot) in &lockfile.snapshots {
            let parsed = key::parse(snapshot_key)
                .with_context(|| format!("Failed to parse snapshot `{snapshot_key}`"))?;
            let from = format!("`{snapshot_key}`");
            let instance = OutputInstance {
                key: snapshot_key.clone(),
                dependencies: resolve_deps(lockfile, &from, &snapshot.dependencies)?,
                optional_dependencies: resolve_deps(
                    lockfile,
                    &from,
                    &snapshot.optional_dependencies,
                )?,
            };
            let (deps, optional) = dep_names.entry((parsed.name, parsed.version)).or_default();
            deps.extend(instance.dependencies.keys().cloned());
            optional.extend(instance.optional_dependencies.keys().cloned());
            instances.push(instance);
        }
    }

    // The root package.json's resolution
    let mut direct = BTreeMap::new();
    if let (true, Some(root)) = (with_instances, lockfile.importers.get(".")) {
        let from = "the root importer (`.`)";
        direct.extend(resolve_deps(lockfile, from, &root.dependencies)?);
        direct.extend(resolve_deps(lockfile, from, &root.optional_dependencies)?);
        if include_dev {
            direct.extend(resolve_deps(lockfile, from, &root.dev_dependencies)?);
        }
    }

    // Collect packages
    let mut output_packages: Vec<OutputPackage> = Vec::new();

    for (spec, pkg) in &lockfile.packages {
        // Skip dev dependencies if not requested
        if pkg.dev && !include_dev {
            continue;
        }

        // Parse package name and version from the spec
        let (name, version) = match key::parse(spec) {
            Ok(parsed) => (parsed.name, parsed.version),
            Err(e) => {
                eprintln!("warning: could not parse package spec: {:#}", e);
                continue;
            }
        };

        // Get integrity hash
        let integrity = match &pkg.resolution {
            Some(res) => res.integrity.clone().unwrap_or_default(),
            None => String::new(),
        };

        if integrity.is_empty() {
            eprintln!("warning: no integrity hash for {}", spec);
            continue;
        }

        // Get tarball URL (use default npm URL if not specified)
        let url = match &pkg.resolution {
            Some(res) => res
                .tarball
                .clone()
                .unwrap_or_else(|| npm_tarball_url(name, version)),
            None => npm_tarball_url(name, version),
        };

        // Collect dependencies: from the package entry (pnpm before v9),
        // and from every snapshot of the package, peer-suffixed ones too
        let mut dependencies: BTreeSet<String> = pkg.dependencies.keys().cloned().collect();
        let mut optional_dependencies: BTreeSet<String> =
            pkg.optional_dependencies.keys().cloned().collect();
        if let Some((deps, optional)) = dep_names.get(&(name, version)) {
            dependencies.extend(deps.iter().cloned());
            optional_dependencies.extend(optional.iter().cloned());
        }

        output_packages.push(OutputPackage {
            name: name.to_string(),
            version: version.to_string(),
            url,
            integrity,
            dependencies: dependencies.into_iter().collect(),
            optional_dependencies: optional_dependencies.into_iter().collect(),
            os: pkg.os.clone(),
            cpu: pkg.cpu.clone(),
            libc: pkg.libc.clone(),
        });
    }

    // Sort packages by name, then version, for deterministic output
    output_packages.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));

    Ok(OutputToml {
        meta: OutputMeta {
            generator: format!("jsdeps-gen {}", VERSION),
            lockfile_version: lockfile.lockfile_version.clone(),
        },
        packages: output_packages,
        instances,
        direct,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Read and parse lockfile
    let lockfile_content = fs::read_to_string(&args.lock)
        .with_context(|| format!("Failed to read {}", args.lock.display()))?;

    let lockfile: PnpmLockfile = serde_saphyr::from_str(&lockfile_content)
        .with_context(|| format!("Failed to parse {}", args.lock.display()))?;

    eprintln!(
        "Parsed pnpm-lock.yaml (version {})",
        lockfile.lockfile_version
    );

    let output = generate(&lockfile, args.include_dev)?;

    eprintln!(
        "Found {} packages, {} instances",
        output.packages.len(),
        output.instances.len()
    );

    args.output.write("jsdeps-gen", "pnpm-lock.yaml", &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_fields_and_optional_deps() {
        let lock: PnpmLockfile = serde_saphyr::from_str(
            r#"lockfileVersion: '9.0'
packages:
  chokidar@3.6.0:
    resolution: {integrity: sha512-a}
  fsevents@2.3.3:
    resolution: {integrity: sha512-b}
    os: [darwin]
  '@swc/core-linux-x64-musl@1.0.0':
    resolution: {integrity: sha512-c}
    cpu: [x64]
    os: [linux]
    libc: [musl]
snapshots:
  chokidar@3.6.0:
    dependencies:
      braces: 3.0.3
    optionalDependencies:
      fsevents: 2.3.3
  fsevents@2.3.3:
    optional: true
"#,
        )
        .unwrap();
        let fsevents = &lock.packages["fsevents@2.3.3"];
        assert_eq!(fsevents.os, vec!["darwin"]);
        let swc = &lock.packages["@swc/core-linux-x64-musl@1.0.0"];
        assert_eq!(
            (swc.cpu.clone(), swc.libc.clone()),
            (vec!["x64".to_string()], vec!["musl".to_string()])
        );
        let chokidar = &lock.snapshots["chokidar@3.6.0"];
        assert!(chokidar.optional_dependencies.contains_key("fsevents"));
        assert!(chokidar.dependencies.contains_key("braces"));
    }

    /// Parses a lockfile and generates its js-deps.toml contents
    fn generate_from(lock: &str, include_dev: bool) -> Result<OutputToml> {
        let lockfile: PnpmLockfile = serde_saphyr::from_str(lock).unwrap();
        generate(&lockfile, include_dev)
    }

    fn instance<'a>(output: &'a OutputToml, key: &str) -> &'a OutputInstance {
        output
            .instances
            .iter()
            .find(|i| i.key == key)
            .unwrap_or_else(|| panic!("no instance `{key}`"))
    }

    /// A v9 lock where `react-dom` is installed twice, once per `react` it
    /// sees as a peer, and `ui` depends on one of them
    const PEER_SPLIT_LOCK: &str = r#"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      react:
        specifier: ^18.2.0
        version: 18.2.0
      ui:
        specifier: ^1.0.0
        version: 1.0.0(react@18.2.0)
    devDependencies:
      react-dom:
        specifier: ^18.2.0
        version: 18.2.0(react@17.0.2)
packages:
  react@17.0.2:
    resolution: {integrity: sha512-r17}
  react@18.2.0:
    resolution: {integrity: sha512-r18}
  react-dom@18.2.0:
    resolution: {integrity: sha512-rd}
  scheduler@0.23.0:
    resolution: {integrity: sha512-s}
  ui@1.0.0:
    resolution: {integrity: sha512-ui}
snapshots:
  react@17.0.2: {}
  react@18.2.0: {}
  react-dom@18.2.0(react@17.0.2):
    dependencies:
      react: 17.0.2
  react-dom@18.2.0(react@18.2.0):
    dependencies:
      react: 18.2.0
      scheduler: 0.23.0
  scheduler@0.23.0: {}
  ui@1.0.0(react@18.2.0):
    dependencies:
      react: 18.2.0
      react-dom: 18.2.0(react@18.2.0)
"#;

    #[test]
    fn test_instance_keeps_peer_suffix_and_points_to_peer_instance() {
        let output = generate_from(PEER_SPLIT_LOCK, false).unwrap();
        let ui = instance(&output, "ui@1.0.0(react@18.2.0)");
        assert_eq!(ui.dependencies["react"], "react@18.2.0");
        assert_eq!(
            ui.dependencies["react-dom"],
            "react-dom@18.2.0(react@18.2.0)"
        );
    }

    #[test]
    fn test_two_peer_resolutions_are_two_instances_of_one_package() {
        let output = generate_from(PEER_SPLIT_LOCK, false).unwrap();
        assert_eq!(
            instance(&output, "react-dom@18.2.0(react@17.0.2)").dependencies["react"],
            "react@17.0.2"
        );
        assert_eq!(
            instance(&output, "react-dom@18.2.0(react@18.2.0)").dependencies["react"],
            "react@18.2.0"
        );
        let react_doms: Vec<_> = output
            .packages
            .iter()
            .filter(|p| p.name == "react-dom")
            .collect();
        assert_eq!(react_doms.len(), 1);
        // The bare-name deps are the union over both instances, which no
        // snapshot keyed exactly `react-dom@18.2.0` would have given
        assert_eq!(react_doms[0].dependencies, vec!["react", "scheduler"]);
        // Instances are sorted by key, packages by name then version
        let keys: Vec<_> = output.instances.iter().map(|i| i.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        let reacts: Vec<_> = output
            .packages
            .iter()
            .filter(|p| p.name == "react")
            .map(|p| p.version.as_str())
            .collect();
        assert_eq!(reacts, vec!["17.0.2", "18.2.0"]);
    }

    #[test]
    fn test_direct_is_the_root_importer_dev_only_when_asked() {
        let output = generate_from(PEER_SPLIT_LOCK, false).unwrap();
        assert_eq!(
            output.direct,
            BTreeMap::from([
                ("react".to_string(), "react@18.2.0".to_string()),
                ("ui".to_string(), "ui@1.0.0(react@18.2.0)".to_string()),
            ])
        );
        let output = generate_from(PEER_SPLIT_LOCK, true).unwrap();
        assert_eq!(output.direct.len(), 3);
        assert_eq!(output.direct["react-dom"], "react-dom@18.2.0(react@17.0.2)");
    }

    #[test]
    fn test_alias_maps_import_name_to_actual_instance() {
        let output = generate_from(
            r#"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      lodash-legacy:
        specifier: npm:lodash@3.10.1
        version: lodash@3.10.1
packages:
  cliui@8.0.1:
    resolution: {integrity: sha512-c}
  string-width@4.2.3:
    resolution: {integrity: sha512-s}
  lodash@3.10.1:
    resolution: {integrity: sha512-l}
snapshots:
  cliui@8.0.1:
    dependencies:
      string-width-cjs: string-width@4.2.3
  string-width@4.2.3: {}
  lodash@3.10.1: {}
"#,
            false,
        )
        .unwrap();
        assert_eq!(
            instance(&output, "cliui@8.0.1").dependencies["string-width-cjs"],
            "string-width@4.2.3"
        );
        assert_eq!(output.direct["lodash-legacy"], "lodash@3.10.1");
    }

    #[test]
    fn test_nested_peer_groups_and_patch_hash_are_kept_verbatim() {
        let output = generate_from(
            r#"lockfileVersion: '9.0'
packages:
  '@testing-library/react@14.0.0':
    resolution: {integrity: sha512-t}
  '@types/react@18.2.0':
    resolution: {integrity: sha512-tr}
  react@18.2.0:
    resolution: {integrity: sha512-r}
  app@1.0.0:
    resolution: {integrity: sha512-a}
snapshots:
  '@testing-library/react@14.0.0(@types/react@18.2.0(react@18.2.0))(react@18.2.0)':
    dependencies:
      react: 18.2.0(patch_hash=abc123)
  '@types/react@18.2.0(react@18.2.0)': {}
  react@18.2.0(patch_hash=abc123): {}
  app@1.0.0:
    dependencies:
      '@testing-library/react': 14.0.0(@types/react@18.2.0(react@18.2.0))(react@18.2.0)
"#,
            false,
        )
        .unwrap();
        assert_eq!(
            instance(&output, "app@1.0.0").dependencies["@testing-library/react"],
            "@testing-library/react@14.0.0(@types/react@18.2.0(react@18.2.0))(react@18.2.0)"
        );
        let testing = output
            .packages
            .iter()
            .find(|p| p.name == "@testing-library/react")
            .unwrap();
        assert_eq!(testing.dependencies, vec!["react"]);
    }

    #[test]
    fn test_release_is_not_confused_with_prerelease_instance() {
        let lock = r#"lockfileVersion: '9.0'
packages:
  foo@1.0.0:
    resolution: {integrity: sha512-a}
  foo@1.0.0-beta.1:
    resolution: {integrity: sha512-b}
  bar@2.0.0:
    resolution: {integrity: sha512-c}
  app@1.0.0:
    resolution: {integrity: sha512-d}
snapshots:
  foo@1.0.0: {}
  foo@1.0.0-beta.1(bar@2.0.0):
    dependencies:
      bar: 2.0.0
  bar@2.0.0: {}
  app@1.0.0:
    dependencies:
      foo: 1.0.0
"#;
        let output = generate_from(lock, false).unwrap();
        assert_eq!(
            instance(&output, "app@1.0.0").dependencies["foo"],
            "foo@1.0.0"
        );
        let foo = |version: &str| {
            output
                .packages
                .iter()
                .find(|p| p.name == "foo" && p.version == version)
                .unwrap()
        };
        assert!(foo("1.0.0").dependencies.is_empty());
        assert_eq!(foo("1.0.0-beta.1").dependencies, vec!["bar"]);
    }

    #[test]
    fn test_missing_snapshot_names_both_ends() {
        let err = generate_from(
            r#"lockfileVersion: '9.0'
snapshots:
  app@1.0.0:
    dependencies:
      foo: 1.0.0(bar@2.0.0)
"#,
            false,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("app@1.0.0") && msg.contains("foo@1.0.0(bar@2.0.0)"),
            "{msg}"
        );
    }

    #[test]
    fn test_link_and_file_values_fail_naming_them() {
        let err = generate_from(
            r#"lockfileVersion: '9.0'
snapshots:
  app@1.0.0:
    dependencies:
      mylib: link:../mylib
"#,
            false,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("app@1.0.0") && msg.contains("mylib") && msg.contains("link:../mylib"),
            "{msg}"
        );

        let err = generate_from(
            r#"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      vendored:
        specifier: file:vendor/vendored.tgz
        version: file:vendor/vendored.tgz
"#,
            false,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("root importer")
                && msg.contains("vendored")
                && msg.contains("file:vendor/vendored.tgz"),
            "{msg}"
        );
    }

    #[test]
    fn test_output_toml_shape() {
        let output = generate_from(PEER_SPLIT_LOCK, false).unwrap();
        let text = deps_gen_kit::render("jsdeps-gen", "pnpm-lock.yaml", &output).unwrap();
        for expected in [
            "[[instance]]\nkey = \"react-dom@18.2.0(react@18.2.0)\"\n\n\
             [instance.dependencies]\nreact = \"react@18.2.0\"\nscheduler = \"scheduler@0.23.0\"\n",
            "[direct]\nreact = \"react@18.2.0\"\nui = \"ui@1.0.0(react@18.2.0)\"\n",
        ] {
            assert!(text.contains(expected), "no {expected:?} in:\n{text}");
        }
    }

    #[test]
    fn test_npm_tarball_url_simple() {
        let url = npm_tarball_url("lodash", "4.17.21");
        assert_eq!(
            url,
            "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
        );
    }

    #[test]
    fn test_npm_tarball_url_scoped() {
        let url = npm_tarball_url("@types/node", "22.10.10");
        assert_eq!(
            url,
            "https://registry.npmjs.org/@types%2fnode/-/node-22.10.10.tgz"
        );
    }
}
