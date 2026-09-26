//! soldeps-gen: Generate solidity-deps.toml from foundry.toml and package.json
//!
//! This tool parses Foundry's foundry.toml for git dependencies and optionally
//! package.json/pnpm-lock.yaml for npm Solidity packages (like @openzeppelin/contracts).
//! It generates a unified TOML file for use with turnkey's Solidity deps cell (nix/buck2/languages.nix).

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

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Generate solidity-deps.toml from foundry.toml and package.json
#[derive(Parser, Debug)]
#[command(name = "soldeps-gen")]
#[command(about = "Generate solidity-deps.toml from foundry.toml and package.json")]
struct Args {
    /// Path to foundry.toml file
    #[arg(long, default_value = "foundry.toml")]
    foundry: PathBuf,

    /// Path to package.json file (optional, for npm deps)
    #[arg(long)]
    package_json: Option<PathBuf>,

    /// Path to pnpm-lock.yaml file (optional, for npm integrity hashes)
    #[arg(long)]
    pnpm_lock: Option<PathBuf>,

    /// Output file path (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Pin every dependency for Nix: resolve git refs to commits, and fetch
    /// the Nix hashes of GitHub archives and of npm tarballs the pnpm lock
    /// has no integrity for (through nix-prefetch-cached)
    #[arg(long, default_value = "false")]
    prefetch: bool,
}

/// Represents a package in the output TOML
#[derive(Debug, Serialize)]
struct OutputPackage {
    name: String,
    version: String,
    source: String, // "git" or "npm"
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    /// Nix SRI hash of the unpacked `url` archive (git packages only)
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    /// Auto-generated remapping for this package
    #[serde(skip_serializing_if = "Option::is_none")]
    remapping: Option<String>,
}

/// Foundry configuration structure
#[derive(Debug, Deserialize)]
struct FoundryConfig {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    /// Profile configurations - parsed for schema completeness
    #[serde(default)]
    #[allow(dead_code)]
    profile: BTreeMap<String, FoundryProfile>,
}

/// Foundry profile configuration
/// Parsed for schema completeness; remappings may be used in future enhancements.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FoundryProfile {
    #[serde(default)]
    remappings: Vec<String>,
    #[serde(default)]
    libs: Vec<String>,
}

/// package.json structure (simplified)
#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    #[serde(rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, String>,
}

/// pnpm-lock.yaml structure (simplified for v9)
#[derive(Debug, Deserialize)]
struct PnpmLockfile {
    #[serde(default)]
    packages: BTreeMap<String, PnpmPackage>,
}

/// Package entry in pnpm-lock.yaml
#[derive(Debug, Deserialize)]
struct PnpmPackage {
    resolution: Option<PnpmResolution>,
}

/// Resolution info containing integrity hash
#[derive(Debug, Deserialize)]
struct PnpmResolution {
    integrity: Option<String>,
}

/// Output TOML structure
#[derive(Debug, Serialize)]
struct OutputToml {
    meta: OutputMeta,
    #[serde(rename = "package")]
    packages: Vec<OutputPackage>,
}

#[derive(Debug, Serialize)]
struct OutputMeta {
    generator: String,
}

/// Parse Foundry git dependency format
/// Examples:
///   "solady": "https://github.com/vectorized/solady"
///   "forge-std": "https://github.com/foundry-rs/forge-std@v1.8.0"
///   "openzeppelin": "openzeppelin/openzeppelin-contracts@v5.0.0"
fn parse_foundry_dep(name: &str, spec: &str) -> OutputPackage {
    let (repo, rev) = if spec.contains('@') {
        let parts: Vec<&str> = spec.splitn(2, '@').collect();
        (parts[0].to_string(), Some(parts[1].to_string()))
    } else {
        (spec.to_string(), None)
    };

    // Normalize GitHub shorthand
    let full_repo = if !repo.starts_with("http") && repo.contains('/') {
        format!("https://github.com/{}", repo)
    } else {
        repo
    };

    // Generate remapping - point to lib/<name>/src/ for Foundry-style deps
    let remapping = format!("{}/=lib/{}/src/", name, name);

    OutputPackage {
        name: name.to_string(),
        version: rev.clone().unwrap_or_else(|| "main".to_string()),
        source: "git".to_string(),
        url: None,
        integrity: None,
        repo: Some(full_repo),
        rev,
        hash: None,
        remapping: Some(remapping),
    }
}

/// Build the output entry for an npm package at a resolved version
fn npm_package(name: &str, version: &str, integrity: Option<String>) -> OutputPackage {
    OutputPackage {
        name: name.to_string(),
        version: version.to_string(),
        source: "npm".to_string(),
        url: Some(npm_tarball_url(name, version)),
        integrity,
        repo: None,
        rev: None,
        hash: None,
        remapping: Some(format!("{}/=node_modules/{}/", name, name)),
    }
}

/// Check if a package is a Solidity-related npm package
fn is_solidity_package(name: &str) -> bool {
    // Common Solidity npm packages
    let solidity_prefixes = [
        "@openzeppelin/contracts",
        "@openzeppelin/contracts-upgradeable",
        "@chainlink/contracts",
        "@uniswap/",
        "solmate",
        "forge-std",
        "ds-test",
    ];

    solidity_prefixes
        .iter()
        .any(|p| name.starts_with(p) || name == *p)
}

/// Generate NPM tarball URL for a package
fn npm_tarball_url(name: &str, version: &str) -> String {
    // Remove semver prefix (^, ~, etc.)
    let clean_version =
        version.trim_start_matches(|c| c == '^' || c == '~' || c == '=' || c == 'v');

    if name.starts_with('@') {
        let encoded_name = name.replace('/', "%2f");
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            encoded_name,
            name.split('/').next_back().unwrap_or(name),
            clean_version
        )
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, clean_version
        )
    }
}

/// Parse pnpm-lock.yaml to extract integrity hashes
/// Returns a map of "package@version" -> integrity hash
fn parse_pnpm_lock(path: &PathBuf) -> Result<BTreeMap<String, String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let lockfile: PnpmLockfile = serde_saphyr::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    let mut integrity_map = BTreeMap::new();

    // pnpm-lock v9 structure: packages: { "@pkg/name@version": { resolution: { integrity: "sha..." } } }
    for (key, pkg) in &lockfile.packages {
        if let Some(resolution) = &pkg.resolution {
            if let Some(integrity) = &resolution.integrity {
                integrity_map.insert(key.clone(), integrity.clone());
            }
        }
    }

    Ok(integrity_map)
}

/// The network lookups prefetching needs
trait Prefetcher {
    /// Resolve a git ref (tag, branch or HEAD) of a repository to a commit
    fn resolve_ref(&self, repo: &str, git_ref: &str) -> Result<String>;

    /// Nix SRI hash of a URL, of its unpacked contents when `unpack` is set
    fn prefetch_url(&self, url: &str, unpack: bool) -> Result<String>;
}

/// Prefetcher backed by `git ls-remote` and `nix-prefetch-cached`
struct NixPrefetcher;

impl Prefetcher for NixPrefetcher {
    fn resolve_ref(&self, repo: &str, git_ref: &str) -> Result<String> {
        // Asking for `<ref>^{}` too makes ls-remote list the commit an
        // annotated tag points to, next to the tag object itself
        let output = Command::new("git")
            .args(["ls-remote", repo, git_ref, &format!("{}^{{}}", git_ref)])
            .output()
            .context("Failed to run git ls-remote")?;

        if !output.status.success() {
            anyhow::bail!(
                "git ls-remote failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        peeled_commit(&String::from_utf8_lossy(&output.stdout), git_ref)
            .ok_or_else(|| anyhow::anyhow!("Could not resolve {} in {}", git_ref, repo))
    }

    fn prefetch_url(&self, url: &str, unpack: bool) -> Result<String> {
        // nix-prefetch-cached keeps turnkey's prefetch cache and returns an SRI
        // hash; soldeps-gen's wrapper puts it on PATH
        let mut cmd = Command::new("nix-prefetch-cached");
        if unpack {
            cmd.arg("--unpack");
        }
        let output = cmd
            .arg(url)
            .output()
            .context("Failed to run nix-prefetch-cached")?;

        if !output.status.success() {
            anyhow::bail!(
                "prefetch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)
            .context("Invalid UTF-8 from nix-prefetch-cached")?
            .trim()
            .to_string())
    }
}

/// Pick the commit `git_ref` names out of `git ls-remote` output
///
/// Each line is `<object>\t<refname>`. ls-remote matches patterns by path
/// suffix, so the output can hold refs that merely end in `git_ref`; only
/// the ref itself counts, a tag before a branch as in `git rev-parse`. An
/// annotated tag lists both the tag object and, as `<refname>^{}`, the
/// commit it points to; the commit wins.
fn peeled_commit(ls_remote_output: &str, git_ref: &str) -> Option<String> {
    let entries: BTreeMap<&str, &str> = ls_remote_output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(object, refname)| (refname, object))
        .collect();

    [
        format!("refs/tags/{}^{{}}", git_ref),
        format!("refs/tags/{}", git_ref),
        format!("refs/heads/{}", git_ref),
        format!("{}^{{}}", git_ref),
        git_ref.to_string(),
    ]
    .iter()
    .find_map(|refname| entries.get(refname.as_str()))
    .map(|object| object.to_string())
}

/// Whether a git rev is already a full commit hash
fn is_commit_hash(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

/// URL of GitHub's source archive of a commit, if the repository is on GitHub
fn github_archive_url(repo: &str, commit: &str) -> Option<String> {
    let path = repo.strip_prefix("https://github.com/")?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some(format!(
        "https://github.com/{}/{}/archive/{}.tar.gz",
        owner, name, commit
    ))
}

/// Pin a package for Nix, leaving it as it was on any lookup failure
///
/// A git package gets its ref resolved to a commit and, on GitHub, the
/// archive of that commit and its hash, which the soldeps cell fetches as a
/// fixed-output derivation. An npm package the pnpm lock gave no integrity
/// gets the hash of its tarball.
fn prefetch_package(pkg: &mut OutputPackage, prefetcher: &dyn Prefetcher) {
    match pkg.source.as_str() {
        "git" => {
            let Some(repo) = pkg.repo.clone() else { return };
            let git_ref = pkg.rev.clone().unwrap_or_else(|| "HEAD".to_string());

            let commit = if is_commit_hash(&git_ref) {
                git_ref
            } else {
                eprintln!("  resolving {}@{}...", repo, git_ref);
                match prefetcher.resolve_ref(&repo, &git_ref) {
                    Ok(commit) => {
                        eprintln!("  {} -> {}", git_ref, &commit[..12.min(commit.len())]);
                        commit
                    }
                    Err(e) => {
                        eprintln!("  warning: failed to resolve {}: {}", git_ref, e);
                        return;
                    }
                }
            };
            pkg.rev = Some(commit.clone());

            let Some(url) = github_archive_url(&repo, &commit) else {
                eprintln!("  {}: not on GitHub, pinned by commit only", pkg.name);
                return;
            };
            match prefetcher.prefetch_url(&url, true) {
                Ok(hash) => {
                    pkg.url = Some(url);
                    pkg.hash = Some(hash);
                }
                Err(e) => eprintln!("  warning: failed to prefetch {}: {}", url, e),
            }
        }
        "npm" => {
            if pkg.integrity.is_some() {
                return;
            }
            let Some(url) = pkg.url.clone() else { return };
            eprintln!("  prefetching {}...", url);
            match prefetcher.prefetch_url(&url, false) {
                Ok(hash) => pkg.integrity = Some(hash),
                Err(e) => eprintln!("  warning: failed to prefetch {}: {}", url, e),
            }
        }
        _ => {}
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut output_packages: Vec<OutputPackage> = Vec::new();

    // Parse pnpm-lock.yaml for integrity hashes if provided
    let integrity_map = if let Some(pnpm_lock_path) = &args.pnpm_lock {
        if pnpm_lock_path.exists() {
            eprintln!("Parsing pnpm-lock.yaml for integrity hashes...");
            parse_pnpm_lock(pnpm_lock_path)?
        } else {
            eprintln!(
                "Note: {} not found, npm packages won't have integrity hashes",
                pnpm_lock_path.display()
            );
            BTreeMap::new()
        }
    } else {
        BTreeMap::new()
    };

    // Parse foundry.toml if it exists
    if args.foundry.exists() {
        let foundry_content = fs::read_to_string(&args.foundry)
            .with_context(|| format!("Failed to read {}", args.foundry.display()))?;

        let foundry_config: FoundryConfig = toml::from_str(&foundry_content)
            .with_context(|| format!("Failed to parse {}", args.foundry.display()))?;

        eprintln!(
            "Parsed foundry.toml with {} dependencies",
            foundry_config.dependencies.len()
        );

        for (name, spec) in &foundry_config.dependencies {
            output_packages.push(parse_foundry_dep(name, spec));
        }
    } else {
        eprintln!(
            "Note: {} not found, skipping Foundry deps",
            args.foundry.display()
        );
    }

    // Parse package.json if provided
    if let Some(package_json_path) = &args.package_json {
        if package_json_path.exists() {
            let package_json_content = fs::read_to_string(package_json_path)
                .with_context(|| format!("Failed to read {}", package_json_path.display()))?;

            let package_json: PackageJson = serde_json::from_str(&package_json_content)
                .with_context(|| format!("Failed to parse {}", package_json_path.display()))?;

            let all_deps: Vec<(&String, &String)> = package_json
                .dependencies
                .iter()
                .chain(package_json.dev_dependencies.iter())
                .filter(|(name, _)| is_solidity_package(name))
                .collect();

            eprintln!(
                "Found {} Solidity-related npm packages in package.json",
                all_deps.len()
            );

            for (name, version) in all_deps {
                let clean_version =
                    version.trim_start_matches(|c| c == '^' || c == '~' || c == '=');

                // Look up integrity hash from pnpm-lock.yaml
                // The lock file uses resolved versions, not specifier versions
                // Key format: "@scope/pkg@version" or "pkg@version"
                // First try exact match, then search for any version of this package
                let lock_key = format!("{}@{}", name, clean_version);
                let (resolved_version, integrity) = if let Some(hash) = integrity_map.get(&lock_key) {
                    (clean_version.to_string(), Some(hash.clone()))
                } else {
                    // Search for any version of this package in the lock file
                    let prefix = format!("{}@", name);
                    let found = integrity_map
                        .iter()
                        .find(|(k, _)| k.starts_with(&prefix));

                    if let Some((key, hash)) = found {
                        // Extract version from key (e.g., "@openzeppelin/contracts@5.4.0" -> "5.4.0")
                        let ver = key.strip_prefix(&prefix).unwrap_or(clean_version);
                        (ver.to_string(), Some(hash.clone()))
                    } else {
                        (clean_version.to_string(), None)
                    }
                };

                if integrity.is_some() {
                    eprintln!("  {} -> found integrity hash (resolved to {})", name, resolved_version);
                } else {
                    eprintln!("  {} -> no integrity hash found in lock file", name);
                }

                output_packages.push(npm_package(name, &resolved_version, integrity));
            }
        } else {
            eprintln!(
                "Note: {} not found, skipping npm deps",
                package_json_path.display()
            );
        }
    }

    if args.prefetch {
        eprintln!("Prefetching...");
        for pkg in &mut output_packages {
            prefetch_package(pkg, &NixPrefetcher);
        }
    }

    // Sort packages by name for deterministic output
    output_packages.sort_by(|a, b| a.name.cmp(&b.name));

    eprintln!("Total: {} packages", output_packages.len());

    // Create output TOML
    let output = OutputToml {
        meta: OutputMeta {
            generator: format!("soldeps-gen {}", VERSION),
        },
        packages: output_packages,
    };

    // Serialize to TOML
    let toml_str = toml::to_string_pretty(&output).context("Failed to serialize to TOML")?;

    // Write output
    if let Some(output_path) = args.output {
        let mut file = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        file.write_all(toml_str.as_bytes())?;
        eprintln!("Wrote {}", output_path.display());
    } else {
        print!("{}", toml_str);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_foundry_dep_simple() {
        let pkg = parse_foundry_dep("solady", "https://github.com/vectorized/solady");
        assert_eq!(pkg.name, "solady");
        assert_eq!(pkg.source, "git");
        assert_eq!(
            pkg.repo,
            Some("https://github.com/vectorized/solady".to_string())
        );
        assert_eq!(pkg.rev, None);
    }

    #[test]
    fn test_parse_foundry_dep_with_version() {
        let pkg = parse_foundry_dep("forge-std", "https://github.com/foundry-rs/forge-std@v1.8.0");
        assert_eq!(pkg.name, "forge-std");
        assert_eq!(
            pkg.repo,
            Some("https://github.com/foundry-rs/forge-std".to_string())
        );
        assert_eq!(pkg.rev, Some("v1.8.0".to_string()));
    }

    #[test]
    fn test_parse_foundry_dep_shorthand() {
        let pkg = parse_foundry_dep("openzeppelin", "openzeppelin/openzeppelin-contracts@v5.0.0");
        assert_eq!(
            pkg.repo,
            Some("https://github.com/openzeppelin/openzeppelin-contracts".to_string())
        );
        assert_eq!(pkg.rev, Some("v5.0.0".to_string()));
    }

    #[test]
    fn test_is_solidity_package() {
        assert!(is_solidity_package("@openzeppelin/contracts"));
        assert!(is_solidity_package("@openzeppelin/contracts-upgradeable"));
        assert!(is_solidity_package("@chainlink/contracts"));
        assert!(!is_solidity_package("lodash"));
        assert!(!is_solidity_package("typescript"));
    }

    /// Records every call and answers from fixed tables.
    #[derive(Default)]
    struct FakePrefetcher {
        commits: BTreeMap<(String, String), String>,
        hashes: BTreeMap<(String, bool), String>,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl Prefetcher for FakePrefetcher {
        fn resolve_ref(&self, repo: &str, git_ref: &str) -> Result<String> {
            self.calls.borrow_mut().push(format!("resolve {repo} {git_ref}"));
            self.commits
                .get(&(repo.to_string(), git_ref.to_string()))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown ref {git_ref}"))
        }

        fn prefetch_url(&self, url: &str, unpack: bool) -> Result<String> {
            self.calls.borrow_mut().push(format!("prefetch {url} unpack={unpack}"));
            self.hashes
                .get(&(url.to_string(), unpack))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown url {url}"))
        }
    }

    const COMMIT: &str = "b6a506db2262cad5ff982a87789ee6d1558ec861";

    #[test]
    fn test_prefetch_github_dep_pins_commit_and_archive_hash() {
        let mut fake = FakePrefetcher::default();
        fake.commits.insert(
            ("https://github.com/foundry-rs/forge-std".into(), "v1.8.0".into()),
            COMMIT.into(),
        );
        let archive = format!("https://github.com/foundry-rs/forge-std/archive/{COMMIT}.tar.gz");
        fake.hashes.insert((archive.clone(), true), "sha256-forge".into());

        let mut pkg = parse_foundry_dep("forge-std", "https://github.com/foundry-rs/forge-std@v1.8.0");
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.version, "v1.8.0");
        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert_eq!(pkg.url.as_deref(), Some(archive.as_str()));
        assert_eq!(pkg.hash.as_deref(), Some("sha256-forge"));
    }

    #[test]
    fn test_prefetch_git_dep_without_rev_resolves_head() {
        let mut fake = FakePrefetcher::default();
        fake.commits.insert(
            ("https://github.com/vectorized/solady".into(), "HEAD".into()),
            COMMIT.into(),
        );
        fake.hashes.insert(
            (format!("https://github.com/vectorized/solady/archive/{COMMIT}.tar.gz"), true),
            "sha256-solady".into(),
        );

        let mut pkg = parse_foundry_dep("solady", "https://github.com/vectorized/solady");
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert_eq!(pkg.hash.as_deref(), Some("sha256-solady"));
    }

    #[test]
    fn test_prefetch_git_dep_already_at_commit_skips_resolution() {
        let mut fake = FakePrefetcher::default();
        fake.hashes.insert(
            (format!("https://github.com/foundry-rs/forge-std/archive/{COMMIT}.tar.gz"), true),
            "sha256-forge".into(),
        );

        let mut pkg = parse_foundry_dep(
            "forge-std",
            &format!("https://github.com/foundry-rs/forge-std@{COMMIT}"),
        );
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert!(fake.calls.borrow().iter().all(|c| !c.starts_with("resolve")));
    }

    #[test]
    fn test_prefetch_non_github_dep_pins_commit_without_hash() {
        let mut fake = FakePrefetcher::default();
        fake.commits.insert(
            ("https://gitlab.com/acme/lib".into(), "v1".into()),
            COMMIT.into(),
        );

        let mut pkg = parse_foundry_dep("lib", "https://gitlab.com/acme/lib@v1");
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert_eq!(pkg.url, None);
        assert_eq!(pkg.hash, None);
    }

    #[test]
    fn test_prefetch_git_dep_keeps_ref_when_resolution_fails() {
        let fake = FakePrefetcher::default();

        let mut pkg = parse_foundry_dep("forge-std", "https://github.com/foundry-rs/forge-std@v9");
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.rev.as_deref(), Some("v9"));
        assert_eq!(pkg.hash, None);
    }

    #[test]
    fn test_prefetch_npm_dep_without_integrity_hashes_tarball() {
        let url = npm_tarball_url("@openzeppelin/contracts", "5.4.0");
        let mut fake = FakePrefetcher::default();
        fake.hashes.insert((url.clone(), false), "sha256-oz".into());

        let mut pkg = npm_package("@openzeppelin/contracts", "5.4.0", None);
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.integrity.as_deref(), Some("sha256-oz"));
    }

    #[test]
    fn test_prefetch_npm_dep_keeps_lockfile_integrity() {
        let fake = FakePrefetcher::default();

        let mut pkg = npm_package("@openzeppelin/contracts", "5.4.0", Some("sha512-lock".into()));
        prefetch_package(&mut pkg, &fake);

        assert_eq!(pkg.integrity.as_deref(), Some("sha512-lock"));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn test_peeled_commit_prefers_dereferenced_tag() {
        let output = "\
1111111111111111111111111111111111111111\trefs/tags/v1.8.0
2222222222222222222222222222222222222222\trefs/tags/v1.8.0^{}
";
        assert_eq!(
            peeled_commit(output, "v1.8.0").as_deref(),
            Some("2222222222222222222222222222222222222222")
        );
    }

    #[test]
    fn test_peeled_commit_lightweight_ref() {
        let output = "3333333333333333333333333333333333333333\tHEAD\n";
        assert_eq!(
            peeled_commit(output, "HEAD").as_deref(),
            Some("3333333333333333333333333333333333333333")
        );
        assert_eq!(peeled_commit("", "HEAD"), None);
    }

    #[test]
    fn test_peeled_commit_ignores_refs_that_only_share_a_suffix() {
        // ls-remote matches patterns by path suffix, so `v1` also lists a
        // nested tag `release/v1`; only the ref that was asked for counts
        let output = "\
4444444444444444444444444444444444444444\trefs/heads/v1
5555555555555555555555555555555555555555\trefs/tags/release/v1
6666666666666666666666666666666666666666\trefs/tags/release/v1^{}
";
        assert_eq!(
            peeled_commit(output, "v1").as_deref(),
            Some("4444444444444444444444444444444444444444")
        );
    }

    #[test]
    fn test_peeled_commit_prefers_tag_over_branch() {
        // Like git rev-parse, a tag wins over a branch of the same name
        let output = "\
7777777777777777777777777777777777777777\trefs/heads/v2
8888888888888888888888888888888888888888\trefs/tags/v2
";
        assert_eq!(
            peeled_commit(output, "v2").as_deref(),
            Some("8888888888888888888888888888888888888888")
        );
    }

    #[test]
    fn test_github_archive_url() {
        for repo in [
            "https://github.com/foundry-rs/forge-std",
            "https://github.com/foundry-rs/forge-std/",
            "https://github.com/foundry-rs/forge-std.git",
        ] {
            assert_eq!(
                github_archive_url(repo, COMMIT).as_deref(),
                Some(format!("https://github.com/foundry-rs/forge-std/archive/{COMMIT}.tar.gz").as_str()),
                "{repo}"
            );
        }
        assert_eq!(github_archive_url("https://gitlab.com/acme/lib", COMMIT), None);
        assert_eq!(github_archive_url("https://github.com/acme", COMMIT), None);
    }

    #[test]
    fn test_npm_tarball_url() {
        let url = npm_tarball_url("@openzeppelin/contracts", "5.0.0");
        assert_eq!(
            url,
            "https://registry.npmjs.org/@openzeppelin%2fcontracts/-/contracts-5.0.0.tgz"
        );
    }
}
