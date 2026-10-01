//! Rules sync: each `rules.star` file's deps brought in step with its
//! sources, through the mapper's language plug-ins
//!
//! Ported from Go's rulessync (#215).

use crate::mapper::{self, Language, Mapper, Package, TargetKind, Want, path_error};
use crate::report::{Result as SyncResult, TargetChange, UnreadableTarget};
use anyhow::{Context, Result, anyhow, bail};
use conditions::{Configuration, Space};
use project_sync::config::{self as sync_config, DEFAULT_CONFIG_PATH};
use project_sync::launch::Launcher;
use rules_star::Target;
use rules_star::conditional::{self, LabelsReading};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::Path;

/// The syncer's configuration
#[derive(Clone, Default)]
pub struct Config {
    /// The project's root directory
    pub project_root: String,
    /// Report what would change without writing
    pub dry_run: bool,
    /// Print what sync does on stderr
    pub verbose: bool,
    /// Sync every rules.star, not only those whose sources changed
    pub force: bool,
    /// The project's sync configuration: the languages sync resolves deps
    /// for and the build configurations it evaluates. When `None`, it is
    /// read from the project's .turnkey/sync.toml.
    pub sync: Option<sync_config::Config>,
    /// Starts the tools sync and the plug-ins run (git, go, deps-extract,
    /// python3)
    pub launcher: Launcher,
    /// The working directory relative paths are against: the process's,
    /// read once by its `main`
    pub cwd: String,
}

impl Config {
    /// The project's sync configuration: read from its .turnkey/sync.toml
    /// and validated, unless it was given.
    fn load_sync(&mut self) -> Result<&sync_config::Config> {
        if self.sync.is_none() {
            let root = Path::new(&self.project_root);
            let sync = sync_config::Config::load_default_from(root)
                .map_err(|err| anyhow!("reading sync config: {err:#}"))?;
            sync.validate()
                .map_err(|err| anyhow!("invalid sync config: {err:#}"))?;
            self.sync = Some(sync);
        }
        Ok(self.sync.as_ref().expect("the sync configuration"))
    }
}

/// Synchronizes rules.star files
pub struct Syncer {
    config: Config,
    mapper: Mapper,
    /// The configurations every target's deps are resolved for, so that
    /// what sync writes doesn't depend on the host
    space: Space,
}

impl Syncer {
    /// A syncer with the plug-ins of the configured languages.
    pub fn new(mut cfg: Config) -> Result<Syncer> {
        let sync = cfg.load_sync()?.clone();
        // Every sync.toml turnkey writes lists its languages: one without
        // them is missing or predates them, and the turnkey shell
        // regenerates it. Syncing no language would pass every check.
        if sync.languages.is_empty() {
            bail!(
                "{DEFAULT_CONFIG_PATH} lists no [[languages]]: re-enter the turnkey shell to regenerate it"
            );
        }
        let mapper = Mapper::new(&mapper::Config {
            project_root: cfg.project_root.clone(),
            cwd: cfg.cwd.clone(),
            languages: sync.languages.clone(),
            conditions: sync.conditions.clone(),
            launcher: cfg.launcher.clone(),
        })
        .context("creating mapper")?;
        Syncer::with_mapper(cfg, mapper)
    }

    /// A syncer with the given mapper.
    pub fn with_mapper(mut cfg: Config, mapper: Mapper) -> Result<Syncer> {
        let space = cfg.load_sync()?.conditions.space();
        Ok(Syncer {
            config: cfg,
            mapper,
            space,
        })
    }

    /// The mapper's plug-ins
    pub fn languages(&self) -> &[Box<dyn Language>] {
        self.mapper.languages()
    }

    /// The configurations targets' deps are resolved for
    pub fn space(&self) -> &Space {
        &self.space
    }

    /// Syncs the rules.star files under `dir`: those in directories with
    /// changed sources, as git status tells, or every one (with force, or
    /// when git status fails), walked in lexical order.
    pub fn sync_directory(&self, dir: &str) -> Result<Vec<SyncResult>> {
        let mut results = Vec::new();
        if !self.config.force {
            match self.changed_directories(dir) {
                Err(err) => {
                    if self.config.verbose {
                        eprintln!("  git status failed, falling back to full walk: {err:#}");
                    }
                }
                Ok(changed) => {
                    for pkg_dir in changed {
                        let rules_path = gostd::path::join(&[&pkg_dir, "rules.star"]);
                        if std::fs::metadata(&rules_path).is_err() {
                            continue;
                        }
                        results.push(self.sync_or_report(&rules_path));
                    }
                    return Ok(results);
                }
            }
        }

        // Force mode or git fallback: walk the entire tree
        walk(dir, &mut |path, name, is_dir| {
            if is_dir {
                // Below the directory synced
                if path != dir && (name == "vendor" || name == "testdata" || name.starts_with('.'))
                {
                    return Walk::SkipDir;
                }
                return Walk::Continue;
            }
            if name == "rules.star" {
                results.push(self.sync_or_report(path));
            }
            Walk::Continue
        })?;
        Ok(results)
    }

    /// The result of syncing a rules.star file, or of failing to
    fn sync_or_report(&self, rules_path: &str) -> SyncResult {
        self.sync_file(rules_path).unwrap_or_else(|err| SyncResult {
            path: rules_path.to_string(),
            errors: vec![format!("{err:#}")],
            ..SyncResult::default()
        })
    }

    /// Syncs a single rules.star file.
    pub fn sync_file(&self, rules_path: &str) -> Result<SyncResult> {
        let mut result = SyncResult {
            path: rules_path.to_string(),
            ..SyncResult::default()
        };
        let pkg_dir = gostd::path::dir(rules_path);

        let mut f = rules_star::parse_file(std::path::Path::new(rules_path))
            .map_err(|err| anyhow!("parsing rules.star: {err}"))?;

        // The language of the first target a plug-in owns
        let Some(lang) = f
            .targets
            .iter()
            .find_map(|t| self.mapper.rule_language(&t.rule).map(|(lang, _)| lang))
        else {
            return Ok(result);
        };
        if !lang.manages(&pkg_dir) {
            // Outside what the language builds, e.g. a Go test fixture's
            // own module: not sync's to change
            return Ok(result);
        }

        // Check staleness before running the extractor (unless forced)
        if !self.config.force {
            match is_stale(rules_path, &pkg_dir, lang.source_patterns()) {
                Err(err) => {
                    if self.config.verbose {
                        eprintln!("  staleness check failed for {rules_path}: {err:#}");
                    }
                }
                Ok(false) => {
                    result.skipped = true;
                    return Ok(result);
                }
                Ok(true) => {}
            }
        }

        let pkg = match Package::open(lang, &self.config.project_root, &pkg_dir, &self.space) {
            Ok(pkg) => pkg,
            Err(err) => {
                result.errors.push(format!("{err:#}"));
                return Ok(result);
            }
        };
        let space = pkg.space().clone();
        if let Err(err) = pkg.resolve_all() {
            result.errors.push(format!("{err:#}"));
            return Ok(result);
        }

        let mut modified = false;
        for target in &mut f.targets {
            let Some(rule) = lang.rule(&target.rule) else {
                continue;
            };
            if rule.kind == TargetKind::NotSynced {
                continue;
            }
            let attr = rule.deps_attribute;
            if target.no_sync {
                result.opted_out.push(target.name.clone());
                continue;
            }
            let Ok(old) = conditional::read_labels(target, attr, &space) else {
                result.unreadable.push(unreadable(target, attr));
                continue;
            };
            let want = match pkg.target(target, rule) {
                Ok(want) => want,
                Err(bad_attr) => {
                    result.unreadable.push(unreadable(target, &bad_attr));
                    continue;
                }
            };

            match apply_conditional(&mut result, target, attr, &space, &old, |config| {
                want.deps(config, &old.labels(config))
            }) {
                Ok(changed) => modified |= changed,
                Err(err) => {
                    result.errors.push(format!("{err:#}"));
                    return Ok(result);
                }
            }

            // The other attributes the language owns, e.g. Rust's features
            let owned = match want.owned() {
                Ok(owned) => owned,
                Err(err) => {
                    result.errors.push(format!("{err:#}"));
                    return Ok(result);
                }
            };
            for name in owned {
                let Ok(old_value) = conditional::read_labels(target, &name, &space) else {
                    result.unreadable.push(unreadable(target, &name));
                    continue;
                };
                match apply_owned(&mut result, target, &name, &space, &old_value, |config| {
                    want.attr(config, &name)
                }) {
                    Ok(changed) => modified |= changed,
                    Err(err) => {
                        result.errors.push(format!("{err:#}"));
                        return Ok(result);
                    }
                }
            }
        }
        result.errors.extend(pkg.messages());

        if modified && !self.config.dry_run {
            write_file(rules_path, &f.write()).context("writing rules.star")?;
        }
        result.updated = modified;
        Ok(result)
    }

    /// The directories under the project root, sorted, whose rules.star
    /// covers a source file git status reports changed: for each such file,
    /// the nearest directory above it holding a rules.star, without going
    /// above `dir` or the project root.
    fn changed_directories(&self, dir: &str) -> Result<BTreeSet<String>> {
        let root = &self.config.project_root;
        let output = self
            .config
            .launcher
            .command(
                OsStr::new("git"),
                &["status", "--porcelain", "-uall"],
                Some(Path::new(root)),
            )
            .map_err(|err| anyhow!("git status failed: {err}"))?
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|err| anyhow!("git status failed: {err}"))?;
        if !output.status.success() {
            bail!("git status failed: {}", exit_error(&output.status));
        }

        let patterns: Vec<&str> = self
            .mapper
            .languages()
            .iter()
            .flat_map(|l| l.source_patterns().iter().copied())
            .collect();
        let mut changed = BTreeSet::new();
        for line in String::from_utf8_lossy(&output.stdout).split('\n') {
            if line.len() < 4 {
                continue;
            }
            // "XY filename" or "XY orig -> renamed"
            let Some(rest) = line.get(3..) else {
                continue;
            };
            let mut file = rest.trim_matches(gostd::unicode::is_space);
            if let Some(i) = file.find(" -> ") {
                // The destination of a rename
                file = &file[i + 4..];
            }
            let abs = gostd::path::join(&[root, file]);
            if !matches_any(&gostd::path::base(file), &patterns) {
                continue;
            }
            // Walk up to the nearest rules.star: a change in
            // src/cmd/tk/main.go is src/cmd/tk/rules.star's
            let mut d = gostd::path::dir(&abs);
            while d.starts_with(root.as_str()) {
                if std::fs::metadata(gostd::path::join(&[&d, "rules.star"])).is_ok() {
                    changed.insert(d);
                    break;
                }
                // Don't go above the search directory
                if d == dir || d == *root {
                    break;
                }
                d = gostd::path::dir(&d);
            }
        }
        Ok(changed)
    }
}

/// How a command that ran failed, as Go's `*exec.ExitError` prints it
fn exit_error(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit status {code}"),
        (None, Some(signal)) => format!("signal: {}", signal_name(signal)),
        (None, None) => "exit status -1".to_string(),
    }
}

/// A signal's name, as Go's `syscall.Signal` prints it
fn signal_name(signal: i32) -> String {
    match signal {
        1 => "hangup".to_string(),
        2 => "interrupt".to_string(),
        9 => "killed".to_string(),
        15 => "terminated".to_string(),
        n => format!("signal {n}"),
    }
}

/// An unreadable target's record
fn unreadable(target: &Target, attr: &str) -> UnreadableTarget {
    UnreadableTarget {
        target: target.name.clone(),
        attribute: attr.to_string(),
    }
}

/// Writes a rules.star (0644 when created), as Go's `os.WriteFile` does
fn write_file(path: &str, content: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(path)
        .map_err(|err| anyhow!(path_error("open", path, &err)))?;
    file.write_all(content.as_bytes())
        .map_err(|err| anyhow!(path_error("write", path, &err)))
}

/// What a walk does after an entry
#[derive(PartialEq, Eq)]
enum Walk {
    Continue,
    SkipDir,
    SkipAll,
}

/// `filepath.Walk`: visits `root` and everything below it in lexical
/// order, without following symlinks, calling `visit` with each path, its
/// name and whether it is a directory. An entry that can't be read stops
/// the walk with its error.
fn walk(root: &str, visit: &mut dyn FnMut(&str, &str, bool) -> Walk) -> Result<()> {
    let meta =
        std::fs::symlink_metadata(root).map_err(|err| anyhow!(path_error("lstat", root, &err)))?;
    walk_entry(root, &gostd::path::base(root), meta.is_dir(), visit)?;
    Ok(())
}

/// Visits one entry, and below it if it is a directory; false when the
/// walk is to stop. As in Go's walk, a directory's entries are read before
/// it is visited, so one that can't be read stops the walk even if it would
/// be skipped.
fn walk_entry(
    path: &str,
    name: &str,
    is_dir: bool,
    visit: &mut dyn FnMut(&str, &str, bool) -> Walk,
) -> Result<bool> {
    if !is_dir {
        return Ok(visit(path, name, false) != Walk::SkipAll);
    }
    let mut names: Vec<String> = std::fs::read_dir(path)
        .map_err(|err| anyhow!(path_error("open", path, &err)))?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()
        .map_err(|err| anyhow!(path_error("readdirent", path, &err)))?;
    names.sort();
    match visit(path, name, true) {
        Walk::SkipAll => return Ok(false),
        Walk::SkipDir => return Ok(true),
        Walk::Continue => {}
    }
    for name in names {
        let child = gostd::path::join(&[path, &name]);
        let meta = std::fs::symlink_metadata(&child)
            .map_err(|err| anyhow!(path_error("lstat", &child, &err)))?;
        if !walk_entry(&child, &name, meta.is_dir(), visit)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether rules.star needs updating: whether a file under `pkg_dir`
/// matching `patterns` is newer than it. Without patterns, or when
/// rules.star can't be read, it does.
fn is_stale(rules_path: &str, pkg_dir: &str, patterns: &[&str]) -> Result<bool> {
    let rules_mtime = std::fs::metadata(rules_path)
        .and_then(|m| m.modified())
        .map_err(|err| anyhow!(path_error("stat", rules_path, &err)))?;
    if patterns.is_empty() {
        return Ok(true);
    }
    let mut newer = false;
    let mut failed = None;
    walk(pkg_dir, &mut |path, name, is_dir| {
        if is_dir {
            // Hidden directories and common non-source ones, below the
            // package's
            let skip = path != pkg_dir
                && (matches!(
                    name,
                    "vendor" | "node_modules" | "testdata" | "__pycache__" | ".venv" | "target"
                ) || name.starts_with('.'));
            return if skip { Walk::SkipDir } else { Walk::Continue };
        }
        if !matches_any(name, patterns) {
            return Walk::Continue;
        }
        match std::fs::symlink_metadata(path).and_then(|m| m.modified()) {
            Ok(mtime) if mtime > rules_mtime => {
                newer = true;
                Walk::SkipAll
            }
            Ok(_) => Walk::Continue,
            Err(err) => {
                failed = Some(path_error("lstat", path, &err));
                Walk::SkipAll
            }
        }
    })?;
    if let Some(err) = failed {
        bail!(err);
    }
    Ok(newer)
}

/// Whether a file name matches any of `patterns`
fn matches_any(name: &str, patterns: &[&str]) -> bool {
    patterns
        .iter()
        .any(|p| gostd::filepath::match_pattern(OsStr::new(p), OsStr::new(name)).unwrap_or(false))
}

/// `old_deps` with the manual deps of `old_deps` merged into `new_deps`:
/// the local target deps (":mylib", manual same-package deps) and the deps
/// outside the auto-managed section of a list with markers (`preserved`)
/// come first, then `new_deps`.
fn merge_with_preserved(
    old_deps: &[String],
    new_deps: &[String],
    preserved: &[String],
) -> Vec<String> {
    let is_preserved: HashSet<&str> = preserved.iter().map(String::as_str).collect();
    let mut seen: HashSet<&str> = new_deps.iter().map(String::as_str).collect();
    let mut merged = Vec::new();
    for d in old_deps {
        if (d.starts_with(':') || is_preserved.contains(d.as_str())) && seen.insert(d) {
            merged.push(d.clone());
        }
    }
    merged.extend(new_deps.iter().cloned());
    merged
}

/// `deps` without those in `drop`
fn without_deps(deps: &[String], drop: &[String]) -> Vec<String> {
    deps.iter().filter(|d| !drop.contains(d)).cloned().collect()
}

/// Sets a target's deps, held in its `attr` attribute (deps, npm_deps,
/// ...), to what `want` returns in each configuration of `space`,
/// preserving manual deps, and records the change. `old` gives the deps
/// the target has in each configuration. Deps every configuration has are
/// written as a plain list; the others as a `select()` (see
/// [`conditional::set_labels`]). It reports whether the target's deps
/// changed.
pub(crate) fn apply_conditional(
    result: &mut SyncResult,
    target: &mut Target,
    attr: &str,
    space: &Space,
    old: &LabelsReading,
    mut want: impl FnMut(&Configuration) -> Result<Want>,
) -> Result<bool> {
    let preserved = target.get_preserved_labels(attr);
    let mut new_deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut changed = false;
    let (mut added, mut removed, mut kept, mut unmapped) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for config in &space.configurations {
        let old_deps = old.labels(config);
        let w = want(config)?;
        let (deps, k) = merge_deps(&old_deps, &w, &preserved);
        if old_deps != deps {
            changed = true;
        }
        let (a, rm) = diff_deps(&old_deps, &deps);
        union(&mut added, &a);
        union(&mut removed, &rm);
        if !k.is_empty() {
            union(&mut kept, &k);
            union(&mut unmapped, &w.unmapped);
        }
        new_deps.insert(config.to_string(), deps);
    }

    if changed {
        // With markers, set_labels writes only the auto-managed section.
        conditional::set_labels(target, attr, space, |config| {
            without_deps(&new_deps[&config.to_string()], &preserved)
        });
    }
    if changed || !kept.is_empty() {
        result.changes.push(TargetChange {
            target: target.name.clone(),
            added,
            removed,
            kept: kept.clone(),
            unmapped: if kept.is_empty() {
                Vec::new()
            } else {
                unmapped
            },
            ..TargetChange::default()
        });
    }
    Ok(changed)
}

/// Sets a target's `attr`, an attribute other than the deps that sync
/// owns, to exactly what `want` returns in each configuration of `space`,
/// written as a plain list or a `select()` like the deps. `old` gives the
/// values the target has in each configuration. Values are compared as
/// sets, and an absent attribute with no values stays absent. It reports
/// whether the attribute changed.
pub(crate) fn apply_owned(
    result: &mut SyncResult,
    target: &mut Target,
    attr: &str,
    space: &Space,
    old: &LabelsReading,
    mut want: impl FnMut(&Configuration) -> Result<Vec<String>>,
) -> Result<bool> {
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    let (mut changed, mut any_value) = (false, false);
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    for config in &space.configurations {
        let old_values = old.labels(config);
        let mut new_values = want(config)?;
        if same_dep_set(&old_values, &new_values) {
            new_values = old_values.clone();
        }
        any_value = any_value || !new_values.is_empty();
        if old_values != new_values {
            changed = true;
        }
        let (a, rm) = diff_deps(&old_values, &new_values);
        union(&mut added, &a);
        union(&mut removed, &rm);
        values.insert(config.to_string(), new_values);
    }
    if !changed || (target.get_attribute(attr).is_none() && !any_value) {
        return Ok(false);
    }
    conditional::set_labels(target, attr, space, |config| {
        values[&config.to_string()].clone()
    });
    result.changes.push(TargetChange {
        target: target.name.clone(),
        attribute: attr.to_string(),
        added,
        removed,
        ..TargetChange::default()
    });
    Ok(true)
}

/// The deps a target with `old_deps` gets in one configuration: w's
/// labels, preserving manual ones, and the deps kept. If w has unmapped
/// imports its labels are incomplete, so no existing dep is removed: those
/// that would have been are returned as kept. An existing dep in the Buck2
/// package of an unsynced dep is never removed either (nor reported as
/// kept). Deps that are `old_deps` in another order are `old_deps`.
fn merge_deps(old_deps: &[String], w: &Want, preserved: &[String]) -> (Vec<String>, Vec<String>) {
    let new_deps = merge_with_preserved(old_deps, &prefer_existing(old_deps, w), preserved);

    let unsynced_pkgs: HashSet<String> = w
        .unsynced
        .iter()
        .map(|d| label_package(&w.stands_for(d)).to_string())
        .collect();
    let (mut new_deps, mut kept) = keep_existing(old_deps, new_deps, |d| {
        !w.unmapped.is_empty() || unsynced_pkgs.contains(label_package(&w.stands_for(d)))
    });
    if w.unmapped.is_empty() {
        // Only unsynced deps were kept; they are reported on their own.
        kept = Vec::new();
    }
    if same_dep_set(old_deps, &new_deps) {
        // Sync manages which deps a target has, not their order.
        new_deps = old_deps.to_vec();
    }
    (new_deps, kept)
}

/// Appends the strings of `more` that `list` lacks, in order.
fn union(list: &mut Vec<String>, more: &[String]) {
    for s in more {
        if !list.contains(s) {
            list.push(s.clone());
        }
    }
}

/// The deps of `old_deps` that `new_deps` lacks and `keep` accepts, as
/// kept. If there are any, merged is `old_deps` without the others, in
/// their order, followed by the deps of `new_deps` that are not in
/// `old_deps`; otherwise merged is `new_deps`.
fn keep_existing(
    old_deps: &[String],
    new_deps: Vec<String>,
    keep: impl Fn(&str) -> bool,
) -> (Vec<String>, Vec<String>) {
    let in_new: HashSet<&str> = new_deps.iter().map(String::as_str).collect();
    let in_old: HashSet<&str> = old_deps.iter().map(String::as_str).collect();
    let kept: Vec<String> = old_deps
        .iter()
        .filter(|d| !in_new.contains(d.as_str()) && keep(d))
        .cloned()
        .collect();
    if kept.is_empty() {
        return (new_deps, Vec::new());
    }
    let mut merged: Vec<String> = old_deps
        .iter()
        .filter(|d| in_new.contains(d.as_str()) || keep(d))
        .cloned()
        .collect();
    merged.extend(
        new_deps
            .iter()
            .filter(|d| !in_old.contains(d.as_str()))
            .cloned(),
    );
    (merged, kept)
}

/// Whether `a` and `b` hold the same deps, in any order
fn same_dep_set(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for d in a {
        *count.entry(d).or_default() += 1;
    }
    for d in b {
        match count.get_mut(d.as_str()) {
            Some(n) if *n > 0 => *n -= 1,
            _ => return false,
        }
    }
    true
}

/// w's labels, each replaced by the existing dep that stands for it, if
/// there is one (see [`Want::stands_for`]).
fn prefer_existing(old_deps: &[String], w: &Want) -> Vec<String> {
    let mut existing: HashMap<String, &String> = HashMap::new();
    for d in old_deps {
        let u = w.stands_for(d);
        if u != *d {
            existing.insert(u, d);
        }
    }
    w.labels
        .iter()
        .map(|d| existing.get(d).map_or_else(|| d.clone(), |e| (*e).clone()))
        .collect()
}

/// A label's Buck2 package: "//src/rust/composition:composition-full" is
/// in "//src/rust/composition".
fn label_package(label: &str) -> &str {
    label.split_once(':').map_or(label, |p| p.0)
}

/// The deps added and the deps removed
fn diff_deps(old_deps: &[String], new_deps: &[String]) -> (Vec<String>, Vec<String>) {
    let old_set: HashSet<&str> = old_deps.iter().map(String::as_str).collect();
    let new_set: HashSet<&str> = new_deps.iter().map(String::as_str).collect();
    let added = new_deps
        .iter()
        .filter(|d| !old_set.contains(d.as_str()))
        .cloned()
        .collect();
    let removed = old_deps
        .iter()
        .filter(|d| !new_set.contains(d.as_str()))
        .cloned()
        .collect();
    (added, removed)
}

#[cfg(test)]
mod tests;
