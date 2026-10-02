//! pydeps-gen: Generate python-deps.toml from pylock.toml, pyproject.toml, or requirements.txt
//!
//! This tool reads Python dependency declarations and generates a python-deps.toml
//! file with Nix-compatible SRI hashes for use with Buck2/Nix integration.
//!
//! Recommended workflow for reproducible builds:
//!   1. uv lock                                    # Generate uv.lock from pyproject.toml
//!   2. uv export --format pylock.toml -o pylock.toml  # Export to PEP 751 format
//!   3. pydeps-gen --lock pylock.toml -o python-deps.toml

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use deps_gen_kit::{OutputArgs, PrefetchArgs, Prefetcher};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Generate python-deps.toml from Python dependency files
#[derive(Parser, Debug)]
#[command(name = "pydeps-gen")]
#[command(about = "Generate python-deps.toml from Python dependency files")]
#[command(after_help = "RECOMMENDED WORKFLOW:\n  \
    1. uv lock                                        # Generate uv.lock\n  \
    2. uv export --format pylock.toml -o pylock.toml  # Export to PEP 751\n  \
    3. pydeps-gen --lock pylock.toml -o python-deps.toml")]
struct Args {
    /// Path to pylock.toml file (PEP 751 lock file - recommended for reproducibility)
    #[arg(long, conflicts_with_all = ["pyproject", "requirements"])]
    lock: Option<PathBuf>,

    /// Path to pyproject.toml file (non-reproducible without lock file)
    #[arg(long, conflicts_with_all = ["lock", "requirements"])]
    pyproject: Option<PathBuf>,

    /// Path to requirements.txt file
    #[arg(long, conflicts_with_all = ["lock", "pyproject"])]
    requirements: Option<PathBuf>,

    #[command(flatten)]
    output: OutputArgs,

    #[command(flatten)]
    prefetch: PrefetchArgs,

    /// Include dev dependencies (from pyproject.toml optional-dependencies.dev)
    #[arg(long, default_value = "false")]
    include_dev: bool,

    /// uv.lock, whose dependency graph (each dependency's marker, each
    /// package's extras) is recorded with the --lock packages
    #[arg(long, requires = "lock")]
    uv_lock: Option<PathBuf>,
}

/// A Python package dependency with resolved version and hash
#[derive(Debug, Clone, Default)]
struct PythonDep {
    name: String,
    version: String,
    url: String,
    hash: String,
    /// The lock's environment marker for installing the package at all
    marker: Option<String>,
    /// Its dependencies, from uv.lock
    dependencies: Vec<UvDep>,
    /// Its extras and the dependencies each adds, from uv.lock
    extras: BTreeMap<String, Vec<UvDep>>,
    /// The extras some package or workspace member asks it for
    requested_extras: Vec<String>,
}

/// uv.lock, the part pydeps-gen reads: the dependency graph
#[derive(Debug, Deserialize)]
struct UvLock {
    #[serde(default)]
    package: Vec<UvPackage>,
}

#[derive(Debug, Deserialize)]
struct UvPackage {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    dependencies: Vec<UvDep>,
    #[serde(default, rename = "optional-dependencies")]
    optional_dependencies: BTreeMap<String, Vec<UvDep>>,
}

/// A dependency edge of uv.lock
#[derive(Debug, Clone, Deserialize)]
struct UvDep {
    name: String,
    #[serde(default)]
    marker: Option<String>,
    /// The extras the edge asks for
    #[serde(default)]
    extra: Vec<String>,
}

/// PyPI package JSON API response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PyPIPackageInfo {
    info: PyPIInfo,
    releases: BTreeMap<String, Vec<PyPIRelease>>,
    urls: Vec<PyPIRelease>,
}

#[derive(Debug, Deserialize)]
struct PyPIInfo {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PyPIRelease {
    filename: String,
    url: String,
    packagetype: String,
    digests: PyPIDigests,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PyPIDigests {
    sha256: String,
}

/// PEP 751 pylock.toml structure
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PyLock {
    lock_version: String,
    #[allow(dead_code)]
    created_by: Option<String>,
    #[allow(dead_code)]
    requires_python: Option<String>,
    #[serde(default)]
    packages: Vec<PyLockPackage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PyLockPackage {
    name: String,
    // Optional: workspace-member entries (e.g., editable directories) omit
    // version, sdist, and wheels — they have only `name` and `directory`.
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    index: Option<String>,
    sdist: Option<PyLockSdist>,
    #[serde(default)]
    wheels: Vec<PyLockWheel>,
    #[serde(default)]
    directory: Option<toml::Value>,
    #[serde(default)]
    marker: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PyLockSdist {
    url: String,
    #[serde(default)]
    hashes: PyLockHashes,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct PyLockWheel {
    url: String,
    #[serde(default)]
    hashes: PyLockHashes,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct PyLockHashes {
    sha256: Option<String>,
}

/// pyproject.toml structure (partial)
#[derive(Debug, Deserialize)]
struct PyProject {
    project: Option<ProjectTable>,
    tool: Option<ToolTable>,
}

#[derive(Debug, Deserialize)]
struct ProjectTable {
    dependencies: Option<Vec<String>>,
    #[serde(rename = "optional-dependencies")]
    optional_dependencies: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize)]
struct ToolTable {
    poetry: Option<PoetryTable>,
}

#[derive(Debug, Deserialize)]
struct PoetryTable {
    dependencies: Option<BTreeMap<String, toml::Value>>,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: Option<BTreeMap<String, toml::Value>>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Require at least one input
    if args.lock.is_none() && args.pyproject.is_none() && args.requirements.is_none() {
        bail!("Must specify one of: --lock, --pyproject, or --requirements");
    }

    let mut prefetcher = args.prefetch.prefetcher();
    let prefetcher = prefetcher.as_mut().map(|p| p as &mut dyn Prefetcher);

    // Handle pylock.toml (recommended path - has exact versions and URLs)
    if let Some(path) = &args.lock {
        let mut resolved = parse_pylock(path, prefetcher)?;
        if let Some(uv_lock) = &args.uv_lock {
            add_uv_graph(&mut resolved, uv_lock)?;
        }

        if resolved.is_empty() {
            eprintln!("Warning: No dependencies found in lock file");
            return Ok(());
        }

        eprintln!("Found {} dependencies", resolved.len());
        return args.output.write(
            "pydeps-gen",
            "pylock.toml",
            &python_deps("pylock.toml", &resolved)?,
        );
    }

    // Parse dependencies from input file (legacy path - version ranges)
    let deps = if let Some(path) = &args.pyproject {
        parse_pyproject(path, args.include_dev)?
    } else if let Some(path) = &args.requirements {
        parse_requirements(path)?
    } else {
        unreachable!()
    };

    if deps.is_empty() {
        eprintln!("Warning: No dependencies found");
        return Ok(());
    }

    eprintln!("Found {} dependencies", deps.len());

    // Resolve versions and fetch hashes
    let resolved = resolve_dependencies(&deps, prefetcher)?;

    let source = if args.pyproject.is_some() {
        "pyproject.toml"
    } else {
        "requirements.txt"
    };
    args.output
        .write("pydeps-gen", source, &python_deps(source, &resolved)?)
}

/// Parse a dependency specifier (PEP 508) into (name, version_constraint).
/// Examples: "requests>=2.0", "flask==2.3.0", "numpy". Its marker and
/// extras are checked but not returned.
fn parse_dep_specifier(spec: &str) -> (String, Option<String>) {
    let spec = spec.trim();

    // Skip empty lines and comments
    if spec.is_empty() || spec.starts_with('#') {
        return (String::new(), None);
    }

    match pep508::parse_requirement(spec) {
        Ok(req) => {
            let version = (!req.version.is_empty()).then_some(req.version);
            (req.name, version)
        }
        Err(e) => {
            eprintln!("Warning: skipping {spec:?}: {e}");
            (String::new(), None)
        }
    }
}

/// Normalize package name (PEP 503)
fn normalize_name(name: &str) -> String {
    pep508::normalize_name(name)
}

/// Parse pyproject.toml and extract dependencies
fn parse_pyproject(path: &PathBuf, include_dev: bool) -> Result<Vec<(String, Option<String>)>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let pyproject: PyProject =
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

    let mut deps = Vec::new();

    // PEP 621 style: [project.dependencies]
    if let Some(project) = &pyproject.project {
        if let Some(dependencies) = &project.dependencies {
            for dep in dependencies {
                let (name, version) = parse_dep_specifier(dep);
                if !name.is_empty() {
                    deps.push((name, version));
                }
            }
        }

        // Optional dependencies (e.g., dev)
        if include_dev
            && let Some(optional) = &project.optional_dependencies
            && let Some(dev_deps) = optional.get("dev")
        {
            for dep in dev_deps {
                let (name, version) = parse_dep_specifier(dep);
                if !name.is_empty() {
                    deps.push((name, version));
                }
            }
        }
    }

    // Poetry style: [tool.poetry.dependencies]
    if let Some(tool) = &pyproject.tool
        && let Some(poetry) = &tool.poetry
    {
        if let Some(poetry_deps) = &poetry.dependencies {
            for (name, value) in poetry_deps {
                // Skip python itself
                if name == "python" {
                    continue;
                }
                let version = match value {
                    toml::Value::String(v) => Some(format!("=={}", v.trim_start_matches('^'))),
                    toml::Value::Table(t) => t
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("=={}", v.trim_start_matches('^'))),
                    _ => None,
                };
                deps.push((normalize_name(name), version));
            }
        }

        if include_dev && let Some(dev_deps) = &poetry.dev_dependencies {
            for (name, value) in dev_deps {
                let version = match value {
                    toml::Value::String(v) => Some(format!("=={}", v.trim_start_matches('^'))),
                    toml::Value::Table(t) => t
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("=={}", v.trim_start_matches('^'))),
                    _ => None,
                };
                deps.push((normalize_name(name), version));
            }
        }
    }

    Ok(deps)
}

/// Parse requirements.txt and extract dependencies
fn parse_requirements(path: &PathBuf) -> Result<Vec<(String, Option<String>)>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let mut deps = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip options like -r, -e, --index-url, etc.
        if line.starts_with('-') {
            continue;
        }

        let (name, version) = parse_dep_specifier(line);
        if !name.is_empty() {
            deps.push((name, version));
        }
    }

    Ok(deps)
}

/// Parse pylock.toml (PEP 751 lock file) and extract resolved dependencies
/// This is the recommended path for reproducible builds since it has exact versions and URLs
fn parse_pylock(
    path: &PathBuf,
    mut prefetcher: Option<&mut dyn Prefetcher>,
) -> Result<Vec<PythonDep>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let pylock: PyLock =
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

    eprintln!("Parsed pylock.toml (lock-version: {})", pylock.lock_version);

    // Before anything is fetched, so a forked lock fails fast
    check_one_version_per_name(
        "pylock.toml",
        pylock
            .packages
            .iter()
            .filter(|pkg| pkg.directory.is_none())
            .filter_map(|pkg| {
                Some((
                    pkg.name.as_str(),
                    pkg.version.as_deref()?,
                    pkg.marker.as_deref(),
                ))
            }),
    )?;

    let mut resolved = Vec::new();

    for (i, pkg) in pylock.packages.iter().enumerate() {
        // Skip workspace-member entries (editable directory installs); they
        // have no archive to fetch and shouldn't appear in the Nix cell.
        if pkg.directory.is_some() {
            continue;
        }

        let version = match &pkg.version {
            Some(v) => v.clone(),
            None => {
                eprintln!("  Warning: No version for {}, skipping", pkg.name);
                continue;
            }
        };

        eprintln!(
            "[{}/{}] Processing {} {}",
            i + 1,
            pylock.packages.len(),
            pkg.name,
            version
        );

        // Prefer sdist (source distribution) for Nix builds
        let (url, _archive_hash) = if let Some(sdist) = &pkg.sdist {
            (sdist.url.clone(), sdist.hashes.sha256.clone())
        } else if let Some(wheel) = pkg.wheels.first() {
            // Fall back to wheel if no sdist
            eprintln!("  Warning: No sdist for {}, using wheel", pkg.name);
            (wheel.url.clone(), wheel.hashes.sha256.clone())
        } else {
            eprintln!("  Warning: No sdist or wheel for {}, skipping", pkg.name);
            continue;
        };

        // Get the Nix hash (for unpacked content)
        // Note: pylock.toml hash is for the archive file, but Nix needs hash of unpacked content
        let hash = match prefetcher.as_deref_mut() {
            None => PLACEHOLDER_HASH.to_string(),
            Some(prefetcher) => match prefetcher.prefetch(&url, true) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("  Warning: Failed to prefetch {}: {}", pkg.name, e);
                    continue;
                }
            },
        };

        resolved.push(PythonDep {
            name: pkg.name.clone(),
            version,
            url,
            hash,
            marker: pkg.marker.clone(),
            ..Default::default()
        });
    }

    // Sort by name
    resolved.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(resolved)
}

/// Add uv.lock's dependency graph to the resolved packages: each one's
/// dependencies and extras, and the extras some edge (from a package or a
/// workspace member) asks it for. Markers are checked, and kept as
/// written.
fn add_uv_graph(resolved: &mut [PythonDep], path: &PathBuf) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let lock: UvLock =
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

    let mut requested: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pkg in &lock.package {
        let edges = pkg
            .dependencies
            .iter()
            .chain(pkg.optional_dependencies.values().flatten());
        for edge in edges {
            if let Some(marker) = &edge.marker {
                pep508::parse_marker(marker)
                    .with_context(|| format!("{}'s dependency on {}", pkg.name, edge.name))?;
            }
            let extras = requested.entry(normalize_name(&edge.name)).or_default();
            for extra in &edge.extra {
                let extra = normalize_name(extra);
                if !extras.contains(&extra) {
                    extras.push(extra);
                }
            }
        }
    }

    // A name uv.lock holds several versions of has one set of edges per
    // version: merging them would be the same silent last-wins as a forked
    // pylock.toml, so only names held once are read. The packages pylock.toml
    // left out (dev tooling) may still fork.
    let mut graph: BTreeMap<String, Vec<&UvPackage>> = BTreeMap::new();
    for pkg in &lock.package {
        graph
            .entry(normalize_name(&pkg.name))
            .or_default()
            .push(pkg);
    }
    for dep in resolved.iter_mut() {
        let name = normalize_name(&dep.name);
        if let Some(versions) = graph.get(&name) {
            let [pkg] = versions.as_slice() else {
                bail!(
                    "{} locks {} versions of {} (the pydeps cell holds one version per \
                     distribution): {}",
                    path.display(),
                    versions.len(),
                    dep.name,
                    versions
                        .iter()
                        .map(|p| p.version.as_deref().unwrap_or("no version"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            };
            dep.dependencies = pkg.dependencies.clone();
            dep.extras = pkg
                .optional_dependencies
                .iter()
                .map(|(extra, deps)| (normalize_name(extra), deps.clone()))
                .collect();
        }
        let mut extras = requested.remove(&name).unwrap_or_default();
        extras.sort();
        dep.requested_extras = extras;
    }
    Ok(())
}

/// Resolve dependencies: fetch version info from PyPI and compute hashes
fn resolve_dependencies(
    deps: &[(String, Option<String>)],
    mut prefetcher: Option<&mut dyn Prefetcher>,
) -> Result<Vec<PythonDep>> {
    let mut resolved = Vec::new();

    for (i, (name, version_constraint)) in deps.iter().enumerate() {
        eprintln!(
            "[{}/{}] Resolving {}{}",
            i + 1,
            deps.len(),
            name,
            version_constraint.as_deref().unwrap_or("")
        );

        match resolve_single_dep(
            name,
            version_constraint.as_deref(),
            prefetcher.as_mut().map(|p| &mut **p as &mut dyn Prefetcher),
        ) {
            Ok(dep) => resolved.push(dep),
            Err(e) => {
                eprintln!("Warning: Failed to resolve {}: {}", name, e);
            }
        }
    }

    // Sort by name
    resolved.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(resolved)
}

/// Resolve a single dependency from PyPI
fn resolve_single_dep(
    name: &str,
    version_constraint: Option<&str>,
    prefetcher: Option<&mut dyn Prefetcher>,
) -> Result<PythonDep> {
    // Query PyPI API
    let url = format!("https://pypi.org/pypi/{}/json", name);
    let response: PyPIPackageInfo = ureq::get(&url)
        .call()
        .with_context(|| format!("Failed to fetch PyPI info for {}", name))?
        .into_json()
        .with_context(|| format!("Failed to parse PyPI response for {}", name))?;

    // Determine version to use
    let version = if let Some(constraint) = version_constraint {
        // Extract pinned version from constraint like "==1.2.3"
        if let Some(pinned) = constraint.strip_prefix("==") {
            pinned.to_string()
        } else {
            // For non-pinned constraints, use latest version
            response.info.version.clone()
        }
    } else {
        response.info.version.clone()
    };

    // Find sdist (source distribution) URL for this version
    let releases = response
        .releases
        .get(&version)
        .ok_or_else(|| anyhow!("Version {} not found for {}", version, name))?;

    // Prefer .tar.gz sdist
    let release = releases
        .iter()
        .find(|r| r.packagetype == "sdist" && r.filename.ends_with(".tar.gz"))
        .or_else(|| releases.iter().find(|r| r.packagetype == "sdist"))
        .ok_or_else(|| anyhow!("No source distribution found for {} {}", name, version))?;

    // Get hash
    let hash = match prefetcher {
        None => PLACEHOLDER_HASH.to_string(),
        // Prefetch with nix to get correct hash for unpacked content
        Some(prefetcher) => prefetcher.prefetch(&release.url, true)?,
    };

    Ok(PythonDep {
        name: response.info.name.clone(),
        version,
        url: release.url.clone(),
        hash,
        ..Default::default()
    })
}

/// The hash --no-prefetch writes: one no archive has
const PLACEHOLDER_HASH: &str = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// python-deps.toml
#[derive(Debug, Serialize)]
struct PythonDeps {
    /// Schema version for forward compatibility: 2 records markers and
    /// extras (docs/user-manual/src/languages/python.md)
    schema_version: u32,
    /// Keyed by each package's name in the pydeps cell
    deps: BTreeMap<String, DepRecord>,
}

/// A package in python-deps.toml
#[derive(Debug, Serialize)]
struct DepRecord {
    version: String,
    hash: String,
    url: String,
    /// The lock's environment marker for installing the package at all
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<EdgeRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    requested_extras: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    extras: BTreeMap<String, Vec<EdgeRecord>>,
}

/// A dependency edge in python-deps.toml: the package by its key, with its
/// marker and the extras it asks for, if any
#[derive(Debug, Serialize)]
struct EdgeRecord {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extras: Vec<String>,
}

/// The python-deps.toml record of the resolved packages. It holds one
/// version per key, so it fails rather than keep one of several.
fn python_deps(source: &str, deps: &[PythonDep]) -> Result<PythonDeps> {
    check_one_version_per_name(
        source,
        deps.iter().map(|dep| {
            (
                dep.name.as_str(),
                dep.version.as_str(),
                dep.marker.as_deref(),
            )
        }),
    )?;
    let edges = |deps: &[UvDep]| -> Vec<EdgeRecord> {
        deps.iter()
            .map(|d| EdgeRecord {
                name: dep_key(&d.name),
                marker: d.marker.clone(),
                extras: d.extra.iter().map(|e| normalize_name(e)).collect(),
            })
            .collect()
    };
    Ok(PythonDeps {
        schema_version: 2,
        deps: deps
            .iter()
            .map(|dep| {
                let record = DepRecord {
                    version: dep.version.clone(),
                    hash: dep.hash.clone(),
                    url: dep.url.clone(),
                    marker: dep.marker.clone(),
                    dependencies: edges(&dep.dependencies),
                    requested_extras: dep.requested_extras.clone(),
                    extras: dep
                        .extras
                        .iter()
                        .map(|(extra, deps)| (extra.clone(), edges(deps)))
                        .collect(),
                };
                (dep_key(&dep.name), record)
            })
            .collect(),
    })
}

/// A distribution's name, version and marker, as a lock writes them
type LockedVersion<'a> = (&'a str, &'a str, Option<&'a str>);

/// Fail when `source` locks several versions of one distribution, naming
/// each with its version and marker. The pydeps cell holds one version per
/// distribution (docs/adr/0010-pydeps-stores-one-distribution-per-store-link.md),
/// where a uv resolution that forks on a marker locks one per fork.
/// Distributions are compared by key, so Foo-Bar and foo_bar are one.
fn check_one_version_per_name<'a>(
    source: &str,
    packages: impl IntoIterator<Item = LockedVersion<'a>>,
) -> Result<()> {
    let mut by_key: BTreeMap<String, Vec<LockedVersion>> = BTreeMap::new();
    for package in packages {
        by_key.entry(dep_key(package.0)).or_default().push(package);
    }
    let forks: Vec<String> = by_key
        .values()
        .filter(|entries| entries.len() > 1)
        .map(|entries| {
            let versions: Vec<String> = entries
                .iter()
                .map(|(name, version, marker)| {
                    format!("  {name} {version} ({})", marker.unwrap_or("no marker"))
                })
                .collect();
            format!(
                "{} versions of {}:\n{}",
                entries.len(),
                entries[0].0,
                versions.join("\n")
            )
        })
        .collect();
    if forks.is_empty() {
        return Ok(());
    }
    bail!(
        "{source} locks several versions of one distribution, and the pydeps cell \
         holds one version per distribution.\n{}\n\
         Pin it, in the pyproject.toml that depends on it, to a range one version \
         satisfies on every Python the workspace allows, so the lock no longer \
         forks; then run tk sync.",
        forks.join("\n")
    )
}

/// A package's key in python-deps.toml, and its name in the pydeps cell:
/// its normalized name with _ for -
fn dep_key(name: &str) -> String {
    normalize_name(name).replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    const UV_LOCK: &str = r#"
version = 1

[[package]]
name = "app"
version = "0.1.0"
source = { editable = "." }
dependencies = [{ name = "Requests", extra = ["socks"] }]

[[package]]
name = "requests"
version = "2.32.0"
dependencies = [
    { name = "urllib3" },
    { name = "colorama", marker = "sys_platform == 'win32'" },
]

[package.optional-dependencies]
socks = [{ name = "PySocks", marker = "python_version >= '3.8'" }]
"#;

    #[test]
    fn records_markers_and_extras() {
        let dir = std::env::temp_dir().join(format!("pydeps-gen-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("uv.lock");
        fs::write(&path, UV_LOCK).unwrap();

        let mut resolved = vec![PythonDep {
            name: "requests".into(),
            version: "2.32.0".into(),
            url: "https://example.com/requests.tar.gz".into(),
            hash: "sha256-x".into(),
            marker: Some("python_version >= \"3.8\"".into()),
            ..Default::default()
        }];
        add_uv_graph(&mut resolved, &path).unwrap();
        let toml = deps_gen_kit::render(
            "pydeps-gen",
            "pylock.toml",
            &python_deps("pylock.toml", &resolved).unwrap(),
        )
        .unwrap();
        fs::remove_dir_all(&dir).unwrap();

        let parsed: toml::Value = toml::from_str(&toml).unwrap();
        let requests = &parsed["deps"]["requests"];
        assert_eq!(parsed["schema_version"].as_integer(), Some(2));
        assert_eq!(
            requests["marker"].as_str(),
            Some(r#"python_version >= "3.8""#)
        );
        let deps = requests["dependencies"].as_array().unwrap();
        assert_eq!(deps[0]["name"].as_str(), Some("urllib3"));
        assert_eq!(deps[0].get("marker"), None);
        assert_eq!(deps[1]["name"].as_str(), Some("colorama"));
        assert_eq!(deps[1]["marker"].as_str(), Some("sys_platform == 'win32'"));
        assert_eq!(
            requests["requested_extras"].as_array().unwrap(),
            &vec![toml::Value::from("socks")]
        );
        let socks = &requests["extras"]["socks"][0];
        assert_eq!(socks["name"].as_str(), Some("pysocks"));
        assert_eq!(socks["marker"].as_str(), Some("python_version >= '3.8'"));
    }

    #[test]
    fn prefetches_each_sdist_unpacked() {
        let dir = std::env::temp_dir().join(format!("pydeps-gen-lock-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pylock.toml");
        fs::write(
            &path,
            r#"
lock-version = "1.0"

[[packages]]
name = "six"
version = "1.17.0"
sdist = { url = "https://example.com/six-1.17.0.tar.gz" }

[[packages]]
name = "member"
directory = { path = "." }
"#,
        )
        .unwrap();
        let mut fake = deps_gen_kit::MemoryPrefetcher::default().with(
            "https://example.com/six-1.17.0.tar.gz",
            true,
            "sha256-six",
        );

        let resolved = parse_pylock(&path, Some(&mut fake)).unwrap();
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].hash, "sha256-six");
        assert_eq!(fake.calls.len(), 1);
    }

    /// parse_pylock's result for `lock`, with a prefetcher that knows no
    /// archive, and the archives it was asked for
    fn parse_lock(lock: &str, test: &str) -> (Result<Vec<PythonDep>>, usize) {
        let dir = std::env::temp_dir().join(format!("pydeps-gen-{test}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pylock.toml");
        fs::write(&path, lock).unwrap();
        let mut fake = deps_gen_kit::MemoryPrefetcher::default();
        let resolved = parse_pylock(&path, Some(&mut fake));
        fs::remove_dir_all(&dir).unwrap();
        (resolved, fake.calls.len())
    }

    #[test]
    fn a_forked_lock_fails_naming_each_version_and_marker() {
        let (resolved, fetched) = parse_lock(
            r#"
lock-version = "1.0"

[[packages]]
name = "numpy"
version = "1.26.4"
marker = "python_full_version < '3.10'"
sdist = { url = "https://example.com/numpy-1.26.4.tar.gz" }

[[packages]]
name = "numpy"
version = "2.1.0"
marker = "python_full_version >= '3.10'"
sdist = { url = "https://example.com/numpy-2.1.0.tar.gz" }

[[packages]]
name = "six"
version = "1.17.0"
sdist = { url = "https://example.com/six-1.17.0.tar.gz" }
"#,
            "fork",
        );

        let error = format!("{:#}", resolved.unwrap_err());
        for expected in [
            "2 versions of numpy",
            "numpy 1.26.4 (python_full_version < '3.10')",
            "numpy 2.1.0 (python_full_version >= '3.10')",
            "one version per distribution",
        ] {
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
        assert!(!error.contains("six"), "{error:?}");
        assert_eq!(fetched, 0, "a forked lock fails before fetching");
    }

    #[test]
    fn names_differing_in_case_or_separators_are_one_distribution() {
        let (resolved, _) = parse_lock(
            r#"
lock-version = "1.0"

[[packages]]
name = "Foo-Bar"
version = "1.0"
sdist = { url = "https://example.com/foo-bar-1.0.tar.gz" }

[[packages]]
name = "foo_bar"
version = "2.0"
marker = "sys_platform == 'linux'"
sdist = { url = "https://example.com/foo_bar-2.0.tar.gz" }

[[packages]]
name = "foo.BAR"
version = "3.0"
sdist = { url = "https://example.com/foo.bar-3.0.tar.gz" }
"#,
            "names",
        );

        let error = format!("{:#}", resolved.unwrap_err());
        for expected in [
            "3 versions of Foo-Bar",
            "Foo-Bar 1.0 (no marker)",
            "foo_bar 2.0 (sys_platform == 'linux')",
            "foo.BAR 3.0 (no marker)",
        ] {
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
    }

    #[test]
    fn the_record_holds_one_version_per_key() {
        let dep = |name: &str, version: &str| PythonDep {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        };
        let error = python_deps(
            "requirements.txt",
            &[dep("Six", "1.16.0"), dep("six", "1.17.0")],
        )
        .unwrap_err();
        assert!(error.to_string().contains("2 versions of Six"), "{error:?}");
    }

    #[test]
    fn a_dependency_uv_lock_holds_twice_fails() {
        let dir = std::env::temp_dir().join(format!("pydeps-gen-uvfork-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("uv.lock");
        fs::write(
            &path,
            r#"
version = 1

[[package]]
name = "numpy"
version = "1.26.4"

[[package]]
name = "numpy"
version = "2.1.0"

[[package]]
name = "pytest"
version = "7.0"

[[package]]
name = "pytest"
version = "8.0"
"#,
        )
        .unwrap();

        let dep = |name: &str| PythonDep {
            name: name.into(),
            version: "1.0".into(),
            ..Default::default()
        };
        // pytest is dev tooling pylock.toml leaves out: its fork is not ours
        let unforked = add_uv_graph(&mut [dep("six")], &path);
        let forked = add_uv_graph(&mut [dep("numpy")], &path);
        fs::remove_dir_all(&dir).unwrap();

        unforked.unwrap();
        let error = forked.unwrap_err().to_string();
        assert!(error.contains("2 versions of numpy"), "{error:?}");
        assert!(error.contains("1.26.4, 2.1.0"), "{error:?}");
    }

    #[test]
    fn dep_specifiers_parse_as_pep508() {
        assert_eq!(
            parse_dep_specifier("Foo_Bar[x] >= 1.0 ; sys_platform == 'linux'"),
            ("foo-bar".to_string(), Some(">= 1.0".to_string()))
        );
        assert_eq!(parse_dep_specifier("numpy"), ("numpy".to_string(), None));
        // A malformed marker is skipped, not mangled
        assert_eq!(parse_dep_specifier("x ; nonsense == 'y'").0, "");
    }
}
