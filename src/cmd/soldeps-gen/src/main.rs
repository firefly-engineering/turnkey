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
use deps_gen_kit::{OutputArgs, PrefetchArgs, Prefetcher};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
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

    /// The previously generated solidity-deps.toml, which this run replaces
    /// (optional). A git package whose pin is unchanged keeps the remapping
    /// target recorded there, without looking it up again; that is the only
    /// way to get one without prefetching
    #[arg(long)]
    previous: Option<PathBuf>,

    /// The directory the soldeps cell vendors packages into, one `<name>/`
    /// per package, relative to the project root. Remapping overrides in
    /// foundry.toml must point inside it; the sync rule passes the cell
    /// link's (nix/buck2/languages.nix)
    #[arg(long)]
    vendor_dir: Option<String>,

    #[command(flatten)]
    output: OutputArgs,

    /// Prefetching pins every dependency for Nix: it resolves git refs to
    /// commits, and fetches the Nix hashes of GitHub archives and of npm
    /// tarballs the pnpm lock has no integrity for
    #[command(flatten)]
    prefetch: PrefetchArgs,
}

/// Where a package comes from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    /// A Foundry-style git dependency, from foundry.toml
    Git,
    /// An npm package, from package.json
    Npm,
}

/// A package in solidity-deps.toml: what this run writes, and what it reads
/// back from the previous one (--previous)
#[derive(Debug, Serialize, Deserialize)]
struct OutputPackage {
    name: String,
    version: String,
    source: Source,
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
    /// The package's remapping, in a Foundry checkout's layout: its target
    /// sits under lib/<name>/ (git) or node_modules/<name>/ (npm), which the
    /// soldeps cell moves to vendor/<name>/
    #[serde(skip_serializing_if = "Option::is_none")]
    remapping: Option<String>,
    /// Set when `remapping` comes from the root foundry.toml's `remappings`
    /// rather than from the package itself, so a later run without the
    /// override does not reuse it
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    remapping_overridden: bool,
}

/// Foundry configuration structure
#[derive(Debug, Deserialize)]
struct FoundryConfig {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    profile: BTreeMap<String, FoundryProfile>,
}

/// The foundry profile keys soldeps-gen reads
#[derive(Debug, Default, Deserialize)]
struct FoundryProfile {
    /// Where the project's sources live; a git dependency's remapping
    /// targets its own
    src: Option<String>,
    /// In the root foundry.toml: per-package overrides of the remapping
    #[serde(default)]
    remappings: Vec<String>,
}

/// The profile forge uses when none is selected
const DEFAULT_PROFILE: &str = "default";

/// Forge's default `src`, for a foundry.toml that does not set one
///
/// A literal rather than `foundry_config::Config::default().src`: that
/// crate pulls in a large part of Foundry (compilers, alloy, ...) for one
/// string that has not changed since forge's first release.
const FORGE_DEFAULT_SRC: &str = "src";

/// The previous solidity-deps.toml, as far as reusing remappings needs it
#[derive(Debug, Default, Deserialize)]
struct PreviousToml {
    #[serde(default)]
    package: Vec<OutputPackage>,
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

    OutputPackage {
        name: name.to_string(),
        version: rev.clone().unwrap_or_else(|| "main".to_string()),
        source: Source::Git,
        url: None,
        integrity: None,
        repo: Some(full_repo),
        rev,
        hash: None,
        // Derived from the dependency's own foundry.toml once it is pinned
        // (assign_remapping)
        remapping: None,
        remapping_overridden: false,
    }
}

/// Build the output entry for an npm package at a resolved version
fn npm_package(name: &str, version: &str, integrity: Option<String>) -> OutputPackage {
    let mut pkg = OutputPackage {
        name: name.to_string(),
        version: version.to_string(),
        source: Source::Npm,
        url: Some(npm_tarball_url(name, version)),
        integrity,
        repo: None,
        rev: None,
        hash: None,
        remapping: None,
        remapping_overridden: false,
    };
    // An npm package maps to its root
    pkg.remapping = Some(remapping_to(&pkg, ""));
    pkg
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

/// Read the solidity-deps.toml a run replaces
fn read_previous(path: &PathBuf) -> Result<PreviousToml> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

/// Parse pnpm-lock.yaml to extract integrity hashes
/// Returns a map of "package@version" -> integrity hash
fn parse_pnpm_lock(path: &PathBuf) -> Result<BTreeMap<String, String>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

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

/// The git lookup prefetching needs, next to the hashes deps-gen-kit's
/// Prefetcher gets
trait RefResolver {
    /// Resolve a git ref (tag, branch or HEAD) of a repository to a commit
    fn resolve_ref(&self, repo: &str, git_ref: &str) -> Result<String>;
}

/// RefResolver backed by `git ls-remote`
struct GitRefResolver;

impl RefResolver for GitRefResolver {
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

/// Owner and name of a repository, if it is on GitHub
fn github_repo(repo: &str) -> Option<(&str, &str)> {
    let path = repo.strip_prefix("https://github.com/")?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((owner, name))
}

/// URL of GitHub's source archive of a commit, if the repository is on GitHub
fn github_archive_url(repo: &str, commit: &str) -> Option<String> {
    let (owner, name) = github_repo(repo)?;
    Some(format!(
        "https://github.com/{}/{}/archive/{}.tar.gz",
        owner, name, commit
    ))
}

/// The seam through which prefetching reads a file of a git dependency, at
/// the commit it is pinned to
///
/// deps-gen-kit's Prefetcher only yields hashes (a prefetch-cache hit never
/// touches the store), so a dependency's own foundry.toml is read here.
trait SourceReader {
    /// Contents of `path` in `repo` at `commit`, or None if the commit has
    /// no such file
    fn read_file(&mut self, repo: &str, commit: &str, path: &str) -> Result<Option<String>>;
}

/// SourceReader backed by GitHub's raw file downloads
///
/// A commit is immutable, and a package reads it only when its pin
/// changed (assign_remapping), so nothing is cached.
struct GitHubRawReader {
    agent: ureq::Agent,
}

impl GitHubRawReader {
    /// How long a download may take, so a stalled connection cannot hang
    /// `tk sync`
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(Self::TIMEOUT).build(),
        }
    }
}

impl SourceReader for GitHubRawReader {
    fn read_file(&mut self, repo: &str, commit: &str, path: &str) -> Result<Option<String>> {
        let (owner, name) = github_repo(repo).ok_or_else(|| {
            anyhow::anyhow!(
                "{} is not on GitHub, the only host files are read from",
                repo
            )
        })?;
        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            owner, name, commit, path
        );
        match self.agent.get(&url).call() {
            Ok(response) => response
                .into_string()
                .map(Some)
                .with_context(|| format!("Failed to read {}", url)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(e).with_context(|| format!("Failed to fetch {}", url)),
        }
    }
}

/// An in-memory SourceReader for tests: answers from a fixed table and
/// records every call
#[cfg(test)]
#[derive(Debug, Default)]
struct MemorySourceReader {
    /// (repo, commit, path) -> contents, None for a file the commit lacks
    files: BTreeMap<(String, String, String), Option<String>>,
    /// Every (repo, commit, path) asked for, in order
    calls: Vec<(String, String, String)>,
}

#[cfg(test)]
impl MemorySourceReader {
    /// This reader, answering `contents` for `path` in `repo` at `commit`
    fn with(mut self, repo: &str, commit: &str, path: &str, contents: Option<&str>) -> Self {
        self.files.insert(
            (repo.to_string(), commit.to_string(), path.to_string()),
            contents.map(str::to_string),
        );
        self
    }
}

#[cfg(test)]
impl SourceReader for MemorySourceReader {
    fn read_file(&mut self, repo: &str, commit: &str, path: &str) -> Result<Option<String>> {
        let key = (repo.to_string(), commit.to_string(), path.to_string());
        self.calls.push(key.clone());
        self.files
            .get(&key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("cannot read {} in {} at {}", path, repo, commit))
    }
}

/// The directory a git dependency's remapping targets, relative to its
/// repository root, given its foundry.toml (None when it has none)
///
/// That is the file's `[profile.default] src`, forge's default `src` when
/// the file does not set it, and the repository root without the file.
/// The result is empty for the root, and ends in `/` otherwise.
fn source_subpath(foundry_toml: Option<&str>) -> Result<String> {
    /// A dependency's foundry.toml, as far as its `src` goes
    #[derive(Deserialize)]
    struct Profiles {
        #[serde(default)]
        profile: BTreeMap<String, FoundryProfile>,
    }

    let Some(content) = foundry_toml else {
        return Ok(String::new());
    };
    let profiles: Profiles = toml::from_str(content).context("Failed to parse its foundry.toml")?;
    let src = profiles
        .profile
        .get(DEFAULT_PROFILE)
        .and_then(|profile| profile.src.as_deref())
        .unwrap_or(FORGE_DEFAULT_SRC);
    relative_dir(src).ok_or_else(|| {
        anyhow::anyhow!("its foundry.toml's src `{}` is outside the repository", src)
    })
}

/// `path` as a directory relative to its root: empty for the root itself,
/// ending in `/` otherwise, or None if it leaves the root
fn relative_dir(path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
    let mut dir = String::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => return None,
            name => {
                dir.push_str(name);
                dir.push('/');
            }
        }
    }
    Some(dir)
}

/// Where a Foundry checkout puts a package, as soldeps-gen's remappings
/// record it (the soldeps cell moves it to vendor/<name>/)
fn layout_root(pkg: &OutputPackage) -> String {
    match pkg.source {
        Source::Git => format!("lib/{}/", pkg.name),
        Source::Npm => format!("node_modules/{}/", pkg.name),
    }
}

/// A package's remapping to `subpath` inside it
fn remapping_to(pkg: &OutputPackage, subpath: &str) -> String {
    format!("{}/={}{}", pkg.name, layout_root(pkg), subpath)
}

/// Whether `previous` records `pkg` at the same pin: the same repository
/// and declared ref and, once prefetching resolved the ref, the same commit
fn same_pin(pkg: &OutputPackage, previous: &OutputPackage) -> bool {
    previous.source == pkg.source
        && previous.repo == pkg.repo
        && previous.version == pkg.version
        && match pkg.rev.as_deref() {
            Some(rev) if is_commit_hash(rev) => previous.rev.as_deref() == Some(rev),
            _ => true,
        }
}

/// Per-package remapping overrides from the root foundry.toml's
/// `remappings`: package name -> the target's path inside the package
///
/// Each entry is an ordinary forge remapping, `<name>/=<vendor>/<name>/...`
/// with `<vendor>` the directory the soldeps cell vendors packages into
/// (--vendor-dir). Its prefix must name a declared package: the rules
/// mapper maps an import's first path segment to the package, so there are
/// no aliases. Its target must stay inside that package.
fn parse_overrides(
    remappings: &[String],
    packages: &[OutputPackage],
    vendor_dir: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    let mut overrides = BTreeMap::new();
    for entry in remappings {
        let invalid =
            |why: String| anyhow::anyhow!("remapping `{}` in foundry.toml: {}", entry, why);
        let vendor_dir = vendor_dir
            .and_then(relative_dir)
            .filter(|dir| !dir.is_empty())
            .ok_or_else(|| {
                invalid("overrides need --vendor-dir, the soldeps cell's vendor directory".into())
            })?;
        let (prefix, target) = entry
            .split_once('=')
            .ok_or_else(|| invalid("not a `<prefix>=<target>` remapping".to_string()))?;
        let pkg = prefix
            .strip_suffix('/')
            .and_then(|name| packages.iter().find(|pkg| pkg.name == name))
            .ok_or_else(|| {
                invalid(format!(
                    "`{}` is not `<name>/` for a declared package; an import's first \
                     path segment names its package, so remappings cannot alias",
                    prefix
                ))
            })?;
        let subpath = target
            .strip_prefix(vendor_dir.as_str())
            .and_then(|rest| rest.strip_prefix(pkg.name.as_str()))
            .and_then(|rest| rest.strip_prefix('/'))
            .and_then(relative_dir)
            .ok_or_else(|| {
                invalid(format!(
                    "the target is not inside {}{}/",
                    vendor_dir, pkg.name
                ))
            })?;
        if overrides.insert(pkg.name.clone(), subpath).is_some() {
            return Err(invalid(format!("{} is remapped more than once", pkg.name)));
        }
    }
    Ok(overrides)
}

/// What prefetching did for a package, as far as deriving its remapping
/// target goes
enum Prefetch<'a> {
    /// Prefetching was off (--no-prefetch)
    Skipped,
    /// Prefetching ran but could not pin or fetch the package
    Failed(&'a anyhow::Error),
    /// Prefetching ran and pinned the package; `reader` reads its files
    Done(&'a mut dyn SourceReader),
}

/// Give a package its remapping
///
/// An override wins. Otherwise an npm package maps to its root, as it
/// already does, and a git package keeps the target recorded in `previous`
/// while its pin is unchanged. Failing that, the target is derived from the
/// package's foundry.toml at its pinned commit. Without a recorded target or
/// a way to derive one, this fails: guessing would write a remapping to a
/// directory that may not exist.
fn assign_remapping(
    pkg: &mut OutputPackage,
    overrides: &BTreeMap<String, String>,
    previous: Option<&OutputPackage>,
    prefetch: Prefetch,
) -> Result<()> {
    if let Some(subpath) = overrides.get(&pkg.name) {
        pkg.remapping = Some(remapping_to(pkg, subpath));
        pkg.remapping_overridden = true;
        return Ok(());
    }
    pkg.remapping_overridden = false;
    match pkg.source {
        Source::Npm => return Ok(()),
        Source::Git => {}
    }

    if let Some(recorded) = previous
        .filter(|previous| same_pin(pkg, previous) && !previous.remapping_overridden)
        .and_then(|previous| previous.remapping.clone())
    {
        pkg.remapping = Some(recorded);
        return Ok(());
    }

    let repo = pkg.repo.clone().unwrap_or_default();
    let pin = format!("{}@{}", repo, pkg.version);
    let reader = match prefetch {
        Prefetch::Skipped => anyhow::bail!(
            "{}: no remapping target is recorded for {}; run a prefetching sync \
             (`tk sync`, without --no-prefetch) to derive one",
            pkg.name,
            pin
        ),
        Prefetch::Failed(e) => anyhow::bail!(
            "{}: cannot derive the remapping target of {}, and none is recorded for \
             it: {:#}",
            pkg.name,
            pin,
            e
        ),
        Prefetch::Done(reader) => reader,
    };
    let Some(commit) = pkg.rev.clone().filter(|rev| is_commit_hash(rev)) else {
        anyhow::bail!("{}: {} was not pinned to a commit", pkg.name, pin);
    };
    if github_repo(&repo).is_none() {
        anyhow::bail!(
            "{}: cannot derive the remapping target of {}: soldeps-gen reads a git \
             dependency's foundry.toml only from GitHub. Add a `{}/=` remapping to \
             the root foundry.toml's [profile.default] remappings, targeting its \
             sources inside the soldeps vendor directory",
            pkg.name,
            pin,
            pkg.name
        );
    }
    let cannot_derive = || {
        format!(
            "{}: cannot derive the remapping target of {} at {}, and none is \
             recorded for it",
            pkg.name, pin, commit
        )
    };
    let foundry_toml = reader
        .read_file(&repo, &commit, "foundry.toml")
        .with_context(cannot_derive)?;
    // GitHub answers 404 for a private repository too. The archive of the
    // same commit, fetched while prefetching, proves the repository is
    // readable, so only then does a 404 mean the commit has no foundry.toml.
    if foundry_toml.is_none() && pkg.hash.is_none() {
        return Err(anyhow::anyhow!(
            "foundry.toml was not found, and the archive of the commit was not \
             fetched to tell a missing file from an unreadable repository"
        ))
        .with_context(cannot_derive);
    }
    let subpath = source_subpath(foundry_toml.as_deref()).with_context(cannot_derive)?;
    pkg.remapping = Some(remapping_to(pkg, &subpath));
    Ok(())
}

/// Pin a package for Nix
///
/// A git package gets its ref resolved to a commit and, on GitHub, the
/// archive of that commit and its hash, which the soldeps cell fetches as a
/// fixed-output derivation. An npm package the pnpm lock gave no integrity
/// gets the hash of its tarball. A failed lookup leaves the package as far
/// as it got and is returned.
fn prefetch_package(
    pkg: &mut OutputPackage,
    refs: &dyn RefResolver,
    prefetcher: &mut dyn Prefetcher,
) -> Result<()> {
    match pkg.source {
        Source::Git => {
            let Some(repo) = pkg.repo.clone() else {
                return Ok(());
            };
            let git_ref = pkg.rev.clone().unwrap_or_else(|| "HEAD".to_string());

            let commit = if is_commit_hash(&git_ref) {
                git_ref
            } else {
                eprintln!("  resolving {}@{}...", repo, git_ref);
                let commit = refs
                    .resolve_ref(&repo, &git_ref)
                    .with_context(|| format!("failed to resolve {}", git_ref))?;
                eprintln!("  {} -> {}", git_ref, &commit[..12.min(commit.len())]);
                commit
            };
            pkg.rev = Some(commit.clone());

            let Some(url) = github_archive_url(&repo, &commit) else {
                eprintln!("  {}: not on GitHub, pinned by commit only", pkg.name);
                return Ok(());
            };
            let hash = prefetcher
                .prefetch(&url, true)
                .with_context(|| format!("failed to prefetch {}", url))?;
            pkg.url = Some(url);
            pkg.hash = Some(hash);
        }
        Source::Npm => {
            if pkg.integrity.is_some() {
                return Ok(());
            }
            let Some(url) = pkg.url.clone() else {
                return Ok(());
            };
            eprintln!("  prefetching {}...", url);
            let hash = prefetcher
                .prefetch(&url, false)
                .with_context(|| format!("failed to prefetch {}", url))?;
            pkg.integrity = Some(hash);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut output_packages: Vec<OutputPackage> = Vec::new();
    // The root foundry.toml's remappings: per-package overrides
    let mut override_remappings: Vec<String> = Vec::new();

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

        if let Some(profile) = foundry_config.profile.get(DEFAULT_PROFILE) {
            override_remappings = profile.remappings.clone();
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
                let (resolved_version, integrity) = if let Some(hash) = integrity_map.get(&lock_key)
                {
                    (clean_version.to_string(), Some(hash.clone()))
                } else {
                    // Search for any version of this package in the lock file
                    let prefix = format!("{}@", name);
                    let found = integrity_map.iter().find(|(k, _)| k.starts_with(&prefix));

                    if let Some((key, hash)) = found {
                        // Extract version from key (e.g., "@openzeppelin/contracts@5.4.0" -> "5.4.0")
                        let ver = key.strip_prefix(&prefix).unwrap_or(clean_version);
                        (ver.to_string(), Some(hash.clone()))
                    } else {
                        (clean_version.to_string(), None)
                    }
                };

                if integrity.is_some() {
                    eprintln!(
                        "  {} -> found integrity hash (resolved to {})",
                        name, resolved_version
                    );
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

    // Before any network work, so an invalid override fails fast
    let overrides = parse_overrides(
        &override_remappings,
        &output_packages,
        args.vendor_dir.as_deref(),
    )
    .with_context(|| format!("Invalid remappings in {}", args.foundry.display()))?;
    let previous = match &args.previous {
        Some(path) if path.exists() => read_previous(path)?,
        _ => PreviousToml::default(),
    };

    // Each package's prefetch failure, None where prefetching succeeded
    let mut failures: Vec<Option<anyhow::Error>> = Vec::new();
    let mut reader = None;
    if let Some(mut prefetcher) = args.prefetch.prefetcher() {
        eprintln!("Prefetching...");
        for pkg in &mut output_packages {
            let result = prefetch_package(pkg, &GitRefResolver, &mut prefetcher);
            if let Err(e) = &result {
                eprintln!("  warning: {}: {:#}", pkg.name, e);
            }
            failures.push(result.err());
        }
        reader = Some(GitHubRawReader::new());
    }

    for (i, pkg) in output_packages.iter_mut().enumerate() {
        let recorded = previous.package.iter().find(|p| p.name == pkg.name);
        let prefetch = match (&mut reader, failures.get(i).and_then(Option::as_ref)) {
            (None, _) => Prefetch::Skipped,
            (Some(_), Some(e)) => Prefetch::Failed(e),
            (Some(reader), None) => Prefetch::Done(reader),
        };
        assign_remapping(pkg, &overrides, recorded, prefetch)?;
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

    args.output
        .write("soldeps-gen", "foundry.toml and package.json", &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_foundry_dep_simple() {
        let pkg = parse_foundry_dep("solady", "https://github.com/vectorized/solady");
        assert_eq!(pkg.name, "solady");
        assert_eq!(pkg.source, Source::Git);
        assert_eq!(
            pkg.repo,
            Some("https://github.com/vectorized/solady".to_string())
        );
        assert_eq!(pkg.rev, None);
    }

    #[test]
    fn test_parse_foundry_dep_with_version() {
        let pkg = parse_foundry_dep(
            "forge-std",
            "https://github.com/foundry-rs/forge-std@v1.8.0",
        );
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

    use deps_gen_kit::MemoryPrefetcher;

    /// Records every call and answers from a fixed table.
    #[derive(Default)]
    struct FakeRefs {
        commits: BTreeMap<(String, String), String>,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl RefResolver for FakeRefs {
        fn resolve_ref(&self, repo: &str, git_ref: &str) -> Result<String> {
            self.calls
                .borrow_mut()
                .push(format!("resolve {repo} {git_ref}"));
            self.commits
                .get(&(repo.to_string(), git_ref.to_string()))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown ref {git_ref}"))
        }
    }

    const COMMIT: &str = "b6a506db2262cad5ff982a87789ee6d1558ec861";

    #[test]
    fn test_prefetch_github_dep_pins_commit_and_archive_hash() {
        let mut refs = FakeRefs::default();
        refs.commits.insert(
            (
                "https://github.com/foundry-rs/forge-std".into(),
                "v1.8.0".into(),
            ),
            COMMIT.into(),
        );
        let archive = format!("https://github.com/foundry-rs/forge-std/archive/{COMMIT}.tar.gz");
        let mut hashes = MemoryPrefetcher::default().with(&archive, true, "sha256-forge");

        let mut pkg = parse_foundry_dep(
            "forge-std",
            "https://github.com/foundry-rs/forge-std@v1.8.0",
        );
        prefetch_package(&mut pkg, &refs, &mut hashes).unwrap();

        assert_eq!(pkg.version, "v1.8.0");
        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert_eq!(pkg.url.as_deref(), Some(archive.as_str()));
        assert_eq!(pkg.hash.as_deref(), Some("sha256-forge"));
    }

    #[test]
    fn test_prefetch_git_dep_without_rev_resolves_head() {
        let mut refs = FakeRefs::default();
        refs.commits.insert(
            ("https://github.com/vectorized/solady".into(), "HEAD".into()),
            COMMIT.into(),
        );
        let mut hashes = MemoryPrefetcher::default().with(
            &format!("https://github.com/vectorized/solady/archive/{COMMIT}.tar.gz"),
            true,
            "sha256-solady",
        );

        let mut pkg = parse_foundry_dep("solady", "https://github.com/vectorized/solady");
        prefetch_package(&mut pkg, &refs, &mut hashes).unwrap();

        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert_eq!(pkg.hash.as_deref(), Some("sha256-solady"));
    }

    #[test]
    fn test_prefetch_git_dep_already_at_commit_skips_resolution() {
        let refs = FakeRefs::default();
        let mut hashes = MemoryPrefetcher::default().with(
            &format!("https://github.com/foundry-rs/forge-std/archive/{COMMIT}.tar.gz"),
            true,
            "sha256-forge",
        );

        let mut pkg = parse_foundry_dep(
            "forge-std",
            &format!("https://github.com/foundry-rs/forge-std@{COMMIT}"),
        );
        prefetch_package(&mut pkg, &refs, &mut hashes).unwrap();

        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert!(refs.calls.borrow().is_empty());
    }

    #[test]
    fn test_prefetch_non_github_dep_pins_commit_without_hash() {
        let mut refs = FakeRefs::default();
        refs.commits.insert(
            ("https://gitlab.com/acme/lib".into(), "v1".into()),
            COMMIT.into(),
        );
        let mut hashes = MemoryPrefetcher::default();

        let mut pkg = parse_foundry_dep("lib", "https://gitlab.com/acme/lib@v1");
        prefetch_package(&mut pkg, &refs, &mut hashes).unwrap();

        assert_eq!(pkg.rev.as_deref(), Some(COMMIT));
        assert_eq!(pkg.url, None);
        assert_eq!(pkg.hash, None);
        assert!(hashes.calls.is_empty());
    }

    #[test]
    fn test_prefetch_git_dep_keeps_ref_when_resolution_fails() {
        let refs = FakeRefs::default();
        let mut hashes = MemoryPrefetcher::default();

        let mut pkg = parse_foundry_dep("forge-std", "https://github.com/foundry-rs/forge-std@v9");
        let err = prefetch_package(&mut pkg, &refs, &mut hashes).unwrap_err();
        assert!(format!("{err:#}").contains("v9"), "{err:#}");

        assert_eq!(pkg.rev.as_deref(), Some("v9"));
        assert_eq!(pkg.hash, None);
    }

    #[test]
    fn test_prefetch_npm_dep_without_integrity_hashes_tarball() {
        let url = npm_tarball_url("@openzeppelin/contracts", "5.4.0");
        let refs = FakeRefs::default();
        let mut hashes = MemoryPrefetcher::default().with(&url, false, "sha256-oz");

        let mut pkg = npm_package("@openzeppelin/contracts", "5.4.0", None);
        prefetch_package(&mut pkg, &refs, &mut hashes).unwrap();

        assert_eq!(pkg.integrity.as_deref(), Some("sha256-oz"));
    }

    #[test]
    fn test_prefetch_npm_dep_keeps_lockfile_integrity() {
        let refs = FakeRefs::default();
        let mut hashes = MemoryPrefetcher::default();

        let mut pkg = npm_package(
            "@openzeppelin/contracts",
            "5.4.0",
            Some("sha512-lock".into()),
        );
        prefetch_package(&mut pkg, &refs, &mut hashes).unwrap();

        assert_eq!(pkg.integrity.as_deref(), Some("sha512-lock"));
        assert!(refs.calls.borrow().is_empty());
        assert!(hashes.calls.is_empty());
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
                Some(
                    format!("https://github.com/foundry-rs/forge-std/archive/{COMMIT}.tar.gz")
                        .as_str()
                ),
                "{repo}"
            );
        }
        assert_eq!(
            github_archive_url("https://gitlab.com/acme/lib", COMMIT),
            None
        );
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

    // -- Remapping targets ---------------------------------------------------

    #[test]
    fn test_source_subpath_is_the_dependencys_src() {
        for src in ["contracts", "./contracts", "contracts/", "./contracts/"] {
            let toml = format!("[profile.default]\nsrc = \"{src}\"\n");
            assert_eq!(source_subpath(Some(&toml)).unwrap(), "contracts/", "{src}");
        }
        let nested = "[profile.default]\nsrc = \"packages/core/src\"\n";
        assert_eq!(source_subpath(Some(nested)).unwrap(), "packages/core/src/");
        let root = "[profile.default]\nsrc = \".\"\n";
        assert_eq!(source_subpath(Some(root)).unwrap(), "");
    }

    #[test]
    fn test_source_subpath_defaults_to_forges_src_when_unset() {
        // forge-std's foundry.toml sets no src
        let unset = "[profile.default]\nfs_permissions = []\n\n[rpc_endpoints]\nmainnet = \"x\"\n";
        assert_eq!(source_subpath(Some(unset)).unwrap(), "src/");
        // Another profile's src is not the default profile's
        let other = "[profile.ci]\nsrc = \"contracts\"\n";
        assert_eq!(source_subpath(Some(other)).unwrap(), "src/");
        assert_eq!(source_subpath(Some("")).unwrap(), "src/");
    }

    #[test]
    fn test_source_subpath_is_the_root_without_foundry_toml() {
        assert_eq!(source_subpath(None).unwrap(), "");
    }

    #[test]
    fn test_source_subpath_rejects_src_outside_the_repository() {
        for src in ["../elsewhere", "/abs/src", "src/../../x"] {
            let toml = format!("[profile.default]\nsrc = \"{src}\"\n");
            assert!(source_subpath(Some(&toml)).is_err(), "{src}");
        }
        assert!(source_subpath(Some("not = [toml")).is_err());
    }

    fn declared() -> Vec<OutputPackage> {
        vec![
            parse_foundry_dep("solady", "https://github.com/vectorized/solady@v0.1.0"),
            npm_package("@openzeppelin/contracts", "5.4.0", None),
        ]
    }

    fn remappings(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| e.to_string()).collect()
    }

    const VENDOR: Option<&str> = Some(".turnkey/soldeps/vendor");

    #[test]
    fn test_parse_overrides_maps_package_to_path_inside_it() {
        let overrides = parse_overrides(
            &remappings(&[
                "solady/=.turnkey/soldeps/vendor/solady/src/",
                "@openzeppelin/contracts/=.turnkey/soldeps/vendor/@openzeppelin/contracts/",
            ]),
            &declared(),
            VENDOR,
        )
        .unwrap();
        assert_eq!(
            overrides,
            BTreeMap::from([
                ("solady".to_string(), "src/".to_string()),
                ("@openzeppelin/contracts".to_string(), String::new()),
            ])
        );
    }

    #[test]
    fn test_parse_overrides_rejects_prefix_that_is_not_a_package() {
        for entry in [
            // An alias
            "sol/=.turnkey/soldeps/vendor/solady/src/",
            // A package's subdirectory
            "solady/utils/=.turnkey/soldeps/vendor/solady/src/utils/",
            // No trailing slash
            "solady=.turnkey/soldeps/vendor/solady/src/",
            // A context-scoped remapping
            "examples:solady/=.turnkey/soldeps/vendor/solady/src/",
            // Not a remapping
            "solady/",
        ] {
            let err = parse_overrides(&remappings(&[entry]), &declared(), VENDOR).unwrap_err();
            assert!(format!("{err:#}").contains(entry), "{entry}: {err:#}");
        }
    }

    #[test]
    fn test_parse_overrides_rejects_target_outside_the_vendored_package() {
        for entry in [
            "solady/=lib/solady/src/",
            "solady/=.turnkey/soldeps/vendor/solady",
            "solady/=.turnkey/soldeps/vendor/@openzeppelin/contracts/",
            "solady/=.turnkey/soldeps/vendor/solady/../other/",
        ] {
            let err = parse_overrides(&remappings(&[entry]), &declared(), VENDOR).unwrap_err();
            assert!(format!("{err:#}").contains(entry), "{entry}: {err:#}");
        }
    }

    #[test]
    fn test_parse_overrides_needs_the_vendor_dir() {
        let entry = remappings(&["solady/=.turnkey/soldeps/vendor/solady/src/"]);
        assert!(parse_overrides(&entry, &declared(), None).is_err());
        assert!(parse_overrides(&[], &declared(), None).unwrap().is_empty());
        // The vendor directory is what the flag says, not a fixed path
        let elsewhere = remappings(&["solady/=cells/sol/vendor/solady/src/"]);
        assert!(parse_overrides(&elsewhere, &declared(), VENDOR).is_err());
        assert_eq!(
            parse_overrides(&elsewhere, &declared(), Some("cells/sol/vendor/")).unwrap()["solady"],
            "src/"
        );
    }

    const SOLADY: &str = "https://github.com/vectorized/solady";

    /// solady@v0.1.0 as prefetching leaves it: pinned to COMMIT, with the
    /// archive of that commit fetched
    fn pinned_solady() -> OutputPackage {
        let mut pkg = parse_foundry_dep("solady", &format!("{SOLADY}@v0.1.0"));
        pkg.rev = Some(COMMIT.to_string());
        pkg.hash = Some("sha256-solady".to_string());
        pkg
    }

    /// solady as the previous solidity-deps.toml recorded it
    fn recorded_solady(rev: &str, remapping: &str) -> OutputPackage {
        let mut pkg = parse_foundry_dep("solady", &format!("{SOLADY}@v0.1.0"));
        pkg.rev = Some(rev.into());
        pkg.remapping = Some(remapping.into());
        pkg
    }

    fn no_overrides() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn derive(foundry_toml: Option<&str>) -> OutputPackage {
        let mut reader =
            MemorySourceReader::default().with(SOLADY, COMMIT, "foundry.toml", foundry_toml);
        let mut pkg = pinned_solady();
        assign_remapping(&mut pkg, &no_overrides(), None, Prefetch::Done(&mut reader)).unwrap();
        assert_eq!(reader.calls.len(), 1);
        pkg
    }

    #[test]
    fn test_assign_remapping_derives_the_dependencys_src() {
        let pkg = derive(Some("[profile.default]\nsrc = \"contracts\"\n"));
        assert_eq!(
            pkg.remapping.as_deref(),
            Some("solady/=lib/solady/contracts/")
        );
        assert!(!pkg.remapping_overridden);
    }

    #[test]
    fn test_assign_remapping_derives_forges_default_src_when_unset() {
        let pkg = derive(Some("[profile.default]\nout = \"out\"\n"));
        assert_eq!(pkg.remapping.as_deref(), Some("solady/=lib/solady/src/"));
    }

    #[test]
    fn test_assign_remapping_derives_the_root_without_foundry_toml() {
        let pkg = derive(None);
        assert_eq!(pkg.remapping.as_deref(), Some("solady/=lib/solady/"));
    }

    #[test]
    fn test_assign_remapping_404_without_fetched_archive_is_not_a_missing_file() {
        // GitHub also answers 404 for a private repository: without the
        // archive, a missing foundry.toml proves nothing
        let mut reader = MemorySourceReader::default().with(SOLADY, COMMIT, "foundry.toml", None);
        let mut pkg = pinned_solady();
        pkg.hash = None;

        let err = assign_remapping(&mut pkg, &no_overrides(), None, Prefetch::Done(&mut reader))
            .unwrap_err();

        assert!(format!("{err:#}").contains("archive"), "{err:#}");
        assert_eq!(pkg.remapping, None);
    }

    #[test]
    fn test_assign_remapping_non_github_dep_points_at_overrides() {
        let mut reader = MemorySourceReader::default();
        let mut pkg = parse_foundry_dep("lib", "https://gitlab.com/acme/lib@v1");
        pkg.rev = Some(COMMIT.to_string());

        let err = assign_remapping(&mut pkg, &no_overrides(), None, Prefetch::Done(&mut reader))
            .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("remappings"), "{message}");
        assert!(message.contains("`lib/=`"), "{message}");
        assert!(reader.calls.is_empty());
    }

    #[test]
    fn test_assign_remapping_override_wins() {
        let overrides = BTreeMap::from([("solady".to_string(), "src/utils/".to_string())]);
        let recorded = recorded_solady(COMMIT, "solady/=lib/solady/src/");
        let mut reader = MemorySourceReader::default();

        let mut pkg = pinned_solady();
        assign_remapping(
            &mut pkg,
            &overrides,
            Some(&recorded),
            Prefetch::Done(&mut reader),
        )
        .unwrap();

        assert_eq!(
            pkg.remapping.as_deref(),
            Some("solady/=lib/solady/src/utils/")
        );
        assert!(pkg.remapping_overridden);
        assert!(reader.calls.is_empty());
    }

    #[test]
    fn test_assign_remapping_override_of_npm_package() {
        let overrides =
            BTreeMap::from([("@openzeppelin/contracts".to_string(), "token/".to_string())]);
        let mut pkg = npm_package("@openzeppelin/contracts", "5.4.0", None);
        assign_remapping(&mut pkg, &overrides, None, Prefetch::Skipped).unwrap();
        assert_eq!(
            pkg.remapping.as_deref(),
            Some("@openzeppelin/contracts/=node_modules/@openzeppelin/contracts/token/")
        );
        assert!(pkg.remapping_overridden);
    }

    #[test]
    fn test_assign_remapping_npm_package_maps_to_its_root() {
        let mut pkg = npm_package("@openzeppelin/contracts", "5.4.0", None);
        assign_remapping(&mut pkg, &no_overrides(), None, Prefetch::Skipped).unwrap();
        assert_eq!(
            pkg.remapping.as_deref(),
            Some("@openzeppelin/contracts/=node_modules/@openzeppelin/contracts/")
        );
        assert!(!pkg.remapping_overridden);
    }

    #[test]
    fn test_assign_remapping_reuses_recorded_target_while_pin_is_unchanged() {
        let recorded = recorded_solady(COMMIT, "solady/=lib/solady/contracts/");
        let mut reader = MemorySourceReader::default();

        let mut pkg = pinned_solady();
        assign_remapping(
            &mut pkg,
            &no_overrides(),
            Some(&recorded),
            Prefetch::Done(&mut reader),
        )
        .unwrap();

        assert_eq!(
            pkg.remapping.as_deref(),
            Some("solady/=lib/solady/contracts/")
        );
        assert!(reader.calls.is_empty());
    }

    #[test]
    fn test_assign_remapping_without_prefetch_reuses_recorded_target() {
        // Without prefetching, the declared tag is not resolved to a commit
        let recorded = recorded_solady(COMMIT, "solady/=lib/solady/contracts/");
        let mut pkg = parse_foundry_dep("solady", &format!("{SOLADY}@v0.1.0"));
        assign_remapping(
            &mut pkg,
            &no_overrides(),
            Some(&recorded),
            Prefetch::Skipped,
        )
        .unwrap();
        assert_eq!(
            pkg.remapping.as_deref(),
            Some("solady/=lib/solady/contracts/")
        );
    }

    #[test]
    fn test_assign_remapping_without_prefetch_fails_when_pin_changed() {
        let mut recorded = recorded_solady(COMMIT, "solady/=lib/solady/contracts/");
        recorded.version = "v0.0.9".into();

        for recorded in [None, Some(&recorded)] {
            let mut pkg = parse_foundry_dep("solady", &format!("{SOLADY}@v0.1.0"));
            let err = assign_remapping(&mut pkg, &no_overrides(), recorded, Prefetch::Skipped)
                .unwrap_err();
            assert!(format!("{err:#}").contains("prefetching sync"), "{err:#}");
            assert_eq!(pkg.remapping, None);
        }
    }

    #[test]
    fn test_assign_remapping_reports_the_prefetch_failure_when_pin_changed() {
        // Prefetching ran, but the ref did not resolve
        let failure = anyhow::anyhow!("failed to resolve v0.1.0: ls-remote exploded");
        let mut pkg = parse_foundry_dep("solady", &format!("{SOLADY}@v0.1.0"));

        let err = assign_remapping(&mut pkg, &no_overrides(), None, Prefetch::Failed(&failure))
            .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("ls-remote exploded"), "{message}");
        assert!(!message.contains("prefetching sync"), "{message}");
    }

    #[test]
    fn test_assign_remapping_reuses_recorded_target_when_prefetch_failed() {
        let failure = anyhow::anyhow!("offline");
        let recorded = recorded_solady(COMMIT, "solady/=lib/solady/contracts/");
        let mut pkg = parse_foundry_dep("solady", &format!("{SOLADY}@v0.1.0"));
        assign_remapping(
            &mut pkg,
            &no_overrides(),
            Some(&recorded),
            Prefetch::Failed(&failure),
        )
        .unwrap();
        assert_eq!(
            pkg.remapping.as_deref(),
            Some("solady/=lib/solady/contracts/")
        );
    }

    #[test]
    fn test_assign_remapping_fails_when_read_fails_and_pin_changed() {
        // Recorded at another commit; the reader cannot read the new one
        let recorded = recorded_solady(
            "0000000000000000000000000000000000000000",
            "solady/=lib/solady/contracts/",
        );
        let mut reader = MemorySourceReader::default();

        let mut pkg = pinned_solady();
        let err = assign_remapping(
            &mut pkg,
            &no_overrides(),
            Some(&recorded),
            Prefetch::Done(&mut reader),
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("cannot read"), "{err:#}");
        assert_eq!(pkg.remapping, None);
    }

    #[test]
    fn test_assign_remapping_does_not_reuse_a_dropped_override() {
        let mut recorded = recorded_solady(COMMIT, "solady/=lib/solady/src/utils/");
        recorded.remapping_overridden = true;

        // Without prefetching there is nothing to fall back on
        let mut pkg = pinned_solady();
        assert!(
            assign_remapping(
                &mut pkg,
                &no_overrides(),
                Some(&recorded),
                Prefetch::Skipped
            )
            .is_err()
        );

        // With it, the target is derived again
        let mut reader = MemorySourceReader::default().with(SOLADY, COMMIT, "foundry.toml", None);
        let mut pkg = pinned_solady();
        assign_remapping(
            &mut pkg,
            &no_overrides(),
            Some(&recorded),
            Prefetch::Done(&mut reader),
        )
        .unwrap();
        assert_eq!(pkg.remapping.as_deref(), Some("solady/=lib/solady/"));
        assert!(!pkg.remapping_overridden);
    }

    #[test]
    fn test_previous_toml_round_trips_output_packages() {
        let mut pkg = pinned_solady();
        pkg.remapping = Some("solady/=lib/solady/src/utils/".into());
        pkg.remapping_overridden = true;
        let doc = OutputToml {
            meta: OutputMeta {
                generator: "soldeps-gen test".into(),
            },
            packages: vec![pkg, npm_package("@openzeppelin/contracts", "5.4.0", None)],
        };
        let text = deps_gen_kit::render("soldeps-gen", "foundry.toml", &doc).unwrap();
        let previous: PreviousToml = toml::from_str(&text).unwrap();

        assert_eq!(previous.package.len(), 2);
        assert_eq!(previous.package[0].source, Source::Git);
        assert!(previous.package[0].remapping_overridden);
        assert_eq!(previous.package[1].source, Source::Npm);
        assert!(!previous.package[1].remapping_overridden);
    }
}
