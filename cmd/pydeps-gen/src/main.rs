//! pydeps-gen: Generate python-deps.toml from pylock.toml, pyproject.toml, or requirements.txt
//!
//! This tool reads Python dependency declarations and generates a python-deps.toml
//! file with Nix-compatible SRI hashes for use with Buck2/Nix integration.
//!
//! Recommended workflow for reproducible builds:
//!   1. uv lock                                    # Generate uv.lock from pyproject.toml
//!   2. uv export --format pylock.toml -o pylock.toml  # Export to PEP 751 format
//!   3. pydeps-gen --lock pylock.toml -o python-deps.toml

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

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

    /// Output file path (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Skip prefetching (produces placeholder hashes)
    #[arg(long, default_value = "false")]
    no_prefetch: bool,

    /// Include dev dependencies (from pyproject.toml optional-dependencies.dev)
    #[arg(long, default_value = "false")]
    include_dev: bool,
}

/// A Python package dependency with resolved version and hash
#[derive(Debug, Clone)]
struct PythonDep {
    name: String,
    version: String,
    url: String,
    hash: String,
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
    version: String,
    #[serde(default)]
    index: Option<String>,
    sdist: Option<PyLockSdist>,
    #[serde(default)]
    wheels: Vec<PyLockWheel>,
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

    // Handle pylock.toml (recommended path - has exact versions and URLs)
    if let Some(path) = &args.lock {
        let resolved = parse_pylock(path, args.no_prefetch)?;

        if resolved.is_empty() {
            eprintln!("Warning: No dependencies found in lock file");
            return Ok(());
        }

        eprintln!("Found {} dependencies", resolved.len());

        let output = generate_toml(&resolved, "pylock.toml");

        // Write output
        if let Some(out_path) = &args.output {
            let mut file = fs::File::create(out_path)
                .with_context(|| format!("Failed to create output file: {}", out_path.display()))?;
            file.write_all(output.as_bytes())?;
            eprintln!("Wrote {}", out_path.display());
        } else {
            print!("{}", output);
        }

        return Ok(());
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
    let resolved = resolve_dependencies(&deps, args.no_prefetch)?;

    // Generate output
    let source = if args.pyproject.is_some() {
        "pyproject.toml"
    } else {
        "requirements.txt"
    };
    let output = generate_toml(&resolved, source);

    // Write output
    if let Some(path) = &args.output {
        let mut file = fs::File::create(path)
            .with_context(|| format!("Failed to create output file: {}", path.display()))?;
        file.write_all(output.as_bytes())?;
        eprintln!("Wrote {}", path.display());
    } else {
        print!("{}", output);
    }

    Ok(())
}

/// Parse a dependency specifier into (name, version_constraint)
/// Examples: "requests>=2.0", "flask==2.3.0", "numpy"
fn parse_dep_specifier(spec: &str) -> (String, Option<String>) {
    let spec = spec.trim();

    // Skip empty lines and comments
    if spec.is_empty() || spec.starts_with('#') {
        return (String::new(), None);
    }

    // Handle extras: package[extra1,extra2]>=1.0
    let spec = if let Some(bracket_pos) = spec.find('[') {
        if let Some(end_bracket) = spec.find(']') {
            format!("{}{}", &spec[..bracket_pos], &spec[end_bracket + 1..])
        } else {
            spec.to_string()
        }
    } else {
        spec.to_string()
    };

    // Handle environment markers: package>=1.0; python_version >= "3.8"
    let spec = spec.split(';').next().unwrap_or(&spec).trim();

    // Find version specifier
    for op in &["===", "==", "!=", "~=", ">=", "<=", ">", "<"] {
        if let Some(pos) = spec.find(op) {
            let name = spec[..pos].trim().to_lowercase();
            let version = spec[pos..].trim().to_string();
            return (normalize_name(&name), Some(version));
        }
    }

    // No version constraint
    (normalize_name(spec), None)
}

/// Normalize package name (PEP 503)
fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .replace('_', "-")
        .replace('.', "-")
}

/// Parse pyproject.toml and extract dependencies
fn parse_pyproject(path: &PathBuf, include_dev: bool) -> Result<Vec<(String, Option<String>)>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let pyproject: PyProject = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

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
        if include_dev {
            if let Some(optional) = &project.optional_dependencies {
                if let Some(dev_deps) = optional.get("dev") {
                    for dep in dev_deps {
                        let (name, version) = parse_dep_specifier(dep);
                        if !name.is_empty() {
                            deps.push((name, version));
                        }
                    }
                }
            }
        }
    }

    // Poetry style: [tool.poetry.dependencies]
    if let Some(tool) = &pyproject.tool {
        if let Some(poetry) = &tool.poetry {
            if let Some(poetry_deps) = &poetry.dependencies {
                for (name, value) in poetry_deps {
                    // Skip python itself
                    if name == "python" {
                        continue;
                    }
                    let version = match value {
                        toml::Value::String(v) => Some(format!("=={}", v.trim_start_matches('^'))),
                        toml::Value::Table(t) => {
                            t.get("version").and_then(|v| v.as_str()).map(|v| {
                                format!("=={}", v.trim_start_matches('^'))
                            })
                        }
                        _ => None,
                    };
                    deps.push((normalize_name(name), version));
                }
            }

            if include_dev {
                if let Some(dev_deps) = &poetry.dev_dependencies {
                    for (name, value) in dev_deps {
                        let version = match value {
                            toml::Value::String(v) => Some(format!("=={}", v.trim_start_matches('^'))),
                            toml::Value::Table(t) => {
                                t.get("version").and_then(|v| v.as_str()).map(|v| {
                                    format!("=={}", v.trim_start_matches('^'))
                                })
                            }
                            _ => None,
                        };
                        deps.push((normalize_name(name), version));
                    }
                }
            }
        }
    }

    Ok(deps)
}

/// Parse requirements.txt and extract dependencies
fn parse_requirements(path: &PathBuf) -> Result<Vec<(String, Option<String>)>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

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
fn parse_pylock(path: &PathBuf, no_prefetch: bool) -> Result<Vec<PythonDep>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let pylock: PyLock = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    eprintln!("Parsed pylock.toml (lock-version: {})", pylock.lock_version);

    let mut resolved = Vec::new();

    for (i, pkg) in pylock.packages.iter().enumerate() {
        eprintln!(
            "[{}/{}] Processing {} {}",
            i + 1,
            pylock.packages.len(),
            pkg.name,
            pkg.version
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
        let hash = if no_prefetch {
            "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()
        } else {
            match prefetch_url(&url) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("  Warning: Failed to prefetch {}: {}", pkg.name, e);
                    continue;
                }
            }
        };

        resolved.push(PythonDep {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            url,
            hash,
        });
    }

    // Sort by name
    resolved.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(resolved)
}

/// Resolve dependencies: fetch version info from PyPI and compute hashes
fn resolve_dependencies(
    deps: &[(String, Option<String>)],
    no_prefetch: bool,
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

        match resolve_single_dep(name, version_constraint.as_deref(), no_prefetch) {
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
    no_prefetch: bool,
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
        if constraint.starts_with("==") {
            constraint[2..].to_string()
        } else {
            // For non-pinned constraints, use latest version
            response.info.version.clone()
        }
    } else {
        response.info.version.clone()
    };

    // Find sdist (source distribution) URL for this version
    let releases = response.releases.get(&version).ok_or_else(|| {
        anyhow!("Version {} not found for {}", version, name)
    })?;

    // Prefer .tar.gz sdist
    let release = releases
        .iter()
        .find(|r| r.packagetype == "sdist" && r.filename.ends_with(".tar.gz"))
        .or_else(|| releases.iter().find(|r| r.packagetype == "sdist"))
        .ok_or_else(|| anyhow!("No source distribution found for {} {}", name, version))?;

    // Get hash
    let hash = if no_prefetch {
        // Use placeholder
        "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()
    } else {
        // Prefetch with nix to get correct hash for unpacked content
        prefetch_url(&release.url)?
    };

    Ok(PythonDep {
        name: response.info.name.clone(),
        version,
        url: release.url.clone(),
        hash,
    })
}

/// Prefetch a URL and return its Nix SRI hash
fn prefetch_url(url: &str) -> Result<String> {
    // Use nix-prefetch-cached for automatic caching
    // Falls back to nix-prefetch-url if wrapper not available
    let output = Command::new("nix-prefetch-cached")
        .args(["--unpack", url])
        .output()
        .or_else(|_| {
            // Fallback to nix-prefetch-url if cached version not available
            Command::new("nix-prefetch-url")
                .args(["--unpack", "--type", "sha256", url])
                .output()
        })
        .context("Failed to run nix-prefetch-cached or nix-prefetch-url")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("prefetch failed: {}", stderr);
    }

    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // nix-prefetch-cached already returns SRI format
    // If using fallback nix-prefetch-url, we need to convert
    if hash.starts_with("sha256-") {
        Ok(hash)
    } else {
        // Convert base32 to SRI using nix hash convert
        let convert_output = Command::new("nix")
            .args(["hash", "convert", "--hash-algo", "sha256", "--to", "sri", &hash])
            .output()
            .context("Failed to run nix hash convert")?;

        if !convert_output.status.success() {
            let stderr = String::from_utf8_lossy(&convert_output.stderr);
            bail!("nix hash convert failed: {}", stderr);
        }

        let sri_hash = String::from_utf8_lossy(&convert_output.stdout).trim().to_string();
        Ok(sri_hash)
    }
}

/// Generate python-deps.toml content
fn generate_toml(deps: &[PythonDep], source: &str) -> String {
    let mut output = String::new();

    output.push_str("# Auto-generated by pydeps-gen\n");
    output.push_str(&format!("# Source: {}\n", source));
    output.push_str("#\n");

    // Show appropriate regeneration command based on source
    let regen_cmd = match source {
        "pylock.toml" => "pydeps-gen --lock pylock.toml -o python-deps.toml",
        "pyproject.toml" => "pydeps-gen --pyproject pyproject.toml -o python-deps.toml",
        "requirements.txt" => "pydeps-gen --requirements requirements.txt -o python-deps.toml",
        _ => "pydeps-gen --lock pylock.toml -o python-deps.toml",
    };
    output.push_str(&format!("# To regenerate: {}\n", regen_cmd));
    output.push('\n');

    for dep in deps {
        output.push_str(&format!("[deps.{}]\n", dep.name.to_lowercase().replace('-', "_")));
        output.push_str(&format!("version = \"{}\"\n", dep.version));
        output.push_str(&format!("hash = \"{}\"\n", dep.hash));
        output.push_str(&format!("url = \"{}\"\n", dep.url));
        output.push('\n');
    }

    output
}
