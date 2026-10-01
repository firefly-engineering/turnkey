//! Regenerating a project's stale deps files
//!
//! A [`Syncer`] checks each `[[deps]]` rule of a project's sync config
//! with [`crate::staleness`], and regenerates a stale target by running
//! the rule's generator from the project root and writing its stdout to
//! the target. Rules run in config order, so a rule whose target is
//! another's source comes after it in `sync.toml`.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::config::{Config, DepsRule};
use crate::launch::Launcher;
use crate::staleness;
use gostd::filepath;

/// What a sync or a check did
#[derive(Debug, Default)]
pub struct SyncResult {
    /// The number of rules checked
    pub checked: usize,
    /// The number of files regenerated
    pub synced: usize,
    /// The errors met, one per rule that failed
    pub errors: Vec<anyhow::Error>,
}

/// Syncs a project's deps files from its sync config
pub struct Syncer {
    /// The sync configuration
    pub config: Config,
    /// The project root directory
    pub root: PathBuf,
    /// Say what is checked and run
    pub verbose: bool,
    /// Say only what changes and what fails
    pub quiet: bool,
    /// Say what would be regenerated, and regenerate nothing
    pub dry_run: bool,
    /// Where status messages go
    pub output: Box<dyn Write>,
    /// The rules to sync or check, by name; empty means every rule
    pub only: Vec<String>,
    /// Starts the generators
    pub launcher: Launcher,
}

/// Where a rule's target stands against its sources
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freshness {
    Fresh,
    Stale,
    /// None of the rule's sources exists (Go enabled in a project without
    /// a go.mod), so there is nothing to generate the target from
    NoSources,
}

impl Syncer {
    /// A syncer for the project at root with config, writing to stderr
    pub fn new(config: Config, root: PathBuf, launcher: Launcher) -> Syncer {
        Syncer {
            config,
            root,
            verbose: false,
            quiet: false,
            dry_run: false,
            output: Box::new(std::io::stderr()),
            only: Vec::new(),
            launcher,
        }
    }

    /// The syncer for the project at root, from its `.turnkey/sync.toml`;
    /// a project without one has no rules
    pub fn load(root: &Path, launcher: Launcher) -> Result<Syncer> {
        let config = Config::load_default_from(root).context("failed to load sync config")?;
        config.validate().context("invalid sync config")?;
        Ok(Syncer::new(config, root.to_path_buf(), launcher))
    }

    /// Regenerates the rules' stale targets
    pub fn sync_deps(&mut self) -> Result<SyncResult> {
        self.run(true).map(|(result, _)| result)
    }

    /// Regenerates rule's target unconditionally, stale or not: tw runs
    /// this when it saw a rule's dependency files change
    pub fn sync_rule(&mut self, rule: &DepsRule) -> Result<()> {
        if !self.quiet {
            self.say(format_args!("Syncing {}...\n", rule.target));
        }
        if self.dry_run {
            self.say(format_args!(
                "  Would regenerate {} (dry run)\n",
                rule.target
            ));
            return Ok(());
        }
        self.regenerate(rule)
            .with_context(|| format!("{}: regeneration failed", rule.name))?;
        if !self.quiet {
            self.say(format_args!("  Regenerated {}\n", rule.target));
        }
        Ok(())
    }

    /// Whether any rule's target is stale, without regenerating
    pub fn check(&mut self) -> Result<(SyncResult, bool)> {
        self.run(false)
    }

    /// Checks each selected rule and, when regenerating, regenerates the
    /// stale ones. Returns whether any target was stale.
    fn run(&mut self, regenerate: bool) -> Result<(SyncResult, bool)> {
        let mut result = SyncResult::default();
        let mut any_stale = false;

        for rule in self.selected_rules()? {
            result.checked += 1;

            let state = match self.freshness(&rule) {
                Ok(state) => state,
                Err(e) => {
                    result.errors.push(e.context(rule.name.clone()));
                    continue;
                }
            };

            match state {
                Freshness::NoSources => {
                    if self.verbose {
                        self.say(format_args!(
                            "{}: none of {} exists, nothing to generate {} from\n",
                            rule.name,
                            rule.sources.join(", "),
                            rule.target
                        ));
                    }
                    continue;
                }
                Freshness::Stale if !regenerate => {
                    self.say(format_args!(
                        "{}: stale ({} newer than {})\n",
                        rule.name,
                        rule.sources.join(", "),
                        rule.target
                    ));
                    any_stale = true;
                    continue;
                }
                _ if !regenerate => {
                    if self.verbose {
                        self.say(format_args!("{}: ok\n", rule.name));
                    }
                    continue;
                }
                Freshness::Fresh => {
                    // Up-to-date files are the common case: say so only
                    // when asked, so a sync before every build stays silent
                    if self.verbose {
                        self.say(format_args!("Checking {}... ok\n", rule.target));
                    }
                    continue;
                }
                Freshness::Stale => {}
            }

            any_stale = true;
            if !self.quiet {
                self.say(format_args!("Checking {}... stale\n", rule.target));
            }
            if self.dry_run {
                self.say(format_args!(
                    "  Would regenerate {} (dry run)\n",
                    rule.target
                ));
                result.synced += 1;
                continue;
            }
            if let Err(e) = self.regenerate(&rule) {
                result
                    .errors
                    .push(e.context(format!("{}: regeneration failed", rule.name)));
                continue;
            }
            self.say(format_args!("  Regenerated {}\n", rule.target));
            result.synced += 1;
        }

        Ok((result, any_stale))
    }

    /// The rules named by `only`, in config order, or every rule when
    /// `only` is empty
    fn selected_rules(&self) -> Result<Vec<DepsRule>> {
        let rules = &self.config.deps;
        if self.only.is_empty() {
            return Ok(rules.clone());
        }
        for name in &self.only {
            if !rules.iter().any(|r| &r.name == name) {
                bail!("no deps rule named {name:?}");
            }
        }
        Ok(rules
            .iter()
            .filter(|r| self.only.contains(&r.name))
            .cloned()
            .collect())
    }

    /// Compares a rule's target with the sources that exist; a missing
    /// source (a go.sum in a module without dependencies) doesn't count.
    /// The sources include those the target lists under the rule's
    /// `target_sources`, and a target that lists none is stale.
    fn freshness(&self, rule: &DepsRule) -> Result<Freshness> {
        let mut sources: Vec<PathBuf> = rule
            .sources
            .iter()
            .map(|s| filepath::join(&[self.root.as_path(), Path::new(s)]))
            .collect();
        let target = filepath::join(&[self.root.as_path(), Path::new(&rule.target)]);
        let mut listed = true;
        if !rule.target_sources.is_empty() {
            let (extra, found) = target_sources(&target, &rule.target_sources)?;
            listed = found;
            sources.extend(
                extra
                    .iter()
                    .map(|s| filepath::join(&[self.root.as_path(), Path::new(s)])),
            );
        }
        let result = staleness::check(&sources, &target)?;
        Ok(match () {
            _ if result.newest_source.is_none() => Freshness::NoSources,
            _ if result.stale || !listed => Freshness::Stale,
            _ => Freshness::Fresh,
        })
    }

    /// Runs rule's generator from the project root, and writes its stdout
    /// to the rule's target
    fn regenerate(&mut self, rule: &DepsRule) -> Result<()> {
        let Some((program, args)) = rule.generator.split_first() else {
            bail!("no generator command specified");
        };
        let target = filepath::join(&[self.root.as_path(), Path::new(&rule.target)]);

        // Create parent directories if needed
        std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o755)
            .create(filepath::dir(&target))
            .context("failed to create target directory")?;

        if self.verbose {
            self.say(format_args!("  Running: {}\n", rule.generator.join(" ")));
        }

        let output = self
            .launcher
            .command(program.as_ref(), args, Some(&self.root))
            .map_err(anyhow::Error::from)
            .and_then(|mut cmd| Ok(cmd.output()?));
        let output = match output {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                return Err(anyhow!(
                    "generator failed: {}\n{}",
                    describe_status(output.status),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            Err(e) => return Err(anyhow!("generator failed: {e:#}\n")),
        };

        // The generator's stdout is the target
        let mut file = std::os::unix::fs::OpenOptionsExt::mode(
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true),
            0o644,
        )
        .open(&target)
        .context("failed to write target")?;
        file.write_all(&output.stdout)
            .context("failed to write target")?;
        Ok(())
    }

    fn say(&mut self, message: std::fmt::Arguments<'_>) {
        let _ = self.output.write_fmt(message);
    }
}

/// An exit status as Go's `ExitError` describes it
fn describe_status(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit status {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        _ => status.to_string(),
    }
}

/// The sources rule's target lists under the rule's `target_sources`,
/// relative to the project root at root: none when the rule has no
/// `target_sources` or the target doesn't list them yet
pub fn listed_sources(root: &Path, rule: &DepsRule) -> Result<Vec<String>> {
    if rule.target_sources.is_empty() {
        return Ok(Vec::new());
    }
    let target = filepath::join(&[root, Path::new(&rule.target)]);
    Ok(target_sources(&target, &rule.target_sources)?.0)
}

/// The sources a target lists under key: a top-level array of paths.
/// found is false when the target doesn't exist or doesn't have the key,
/// as a target written before its rule named the key.
fn target_sources(target: &Path, key: &str) -> Result<(Vec<String>, bool)> {
    let content = match std::fs::read(target) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(e) => return Err(anyhow::Error::new(e).context(format!("read {}", target.display()))),
    };
    let doc: toml::Table = std::str::from_utf8(&content)
        .map_err(anyhow::Error::from)
        .and_then(|text| Ok(toml::from_str(text)?))
        .with_context(|| format!("reading {}'s {key}", target.display()))?;
    let Some(value) = doc.get(key) else {
        return Ok((Vec::new(), false));
    };
    let Some(list) = value.as_array() else {
        bail!("{}: {key} is not an array", target.display());
    };
    let mut sources = Vec::with_capacity(list.len());
    for item in list {
        let Some(source) = item.as_str() else {
            bail!("{}: {key} holds {item}, not a path", target.display());
        };
        sources.push(source.to_owned());
    }
    Ok((sources, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// The PATH the tests run with, for the generators (`echo`)
    fn launcher() -> Launcher {
        Launcher::new(std::env::var_os("PATH"))
    }

    /// A Syncer over a fresh root holding one source file per rule, with
    /// each rule's generator printing the rule's name
    fn new_syncer(names: &[&str]) -> (Syncer, tempfile::TempDir) {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        for name in names {
            std::fs::write(root.path().join(format!("{name}.src")), name).unwrap();
            cfg.deps.push(DepsRule {
                name: name.to_string(),
                sources: vec![format!("{name}.src")],
                target: format!("{name}.out"),
                generator: vec!["echo".into(), name.to_string()],
                ..DepsRule::default()
            });
        }
        let mut s = Syncer::new(cfg, root.path().to_path_buf(), launcher());
        s.output = Box::new(std::io::sink());
        (s, root)
    }

    fn set_mtime(path: &Path, mtime: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    #[test]
    fn sync_deps_only_syncs_named_rules() {
        let (mut s, root) = new_syncer(&["go", "python"]);
        s.only = vec!["python".into()];

        let result = s.sync_deps().unwrap();
        assert_eq!(result.synced, 1);
        assert!(
            !root.path().join("go.out").exists(),
            "only python was named"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("python.out")).unwrap(),
            "python\n"
        );
    }

    #[test]
    fn sync_deps_rejects_unknown_rule() {
        let (mut s, _root) = new_syncer(&["go"]);
        s.only = vec!["haskell".into()];

        assert!(s.sync_deps().is_err());
        assert!(s.check().is_err());
    }

    #[test]
    fn sync_deps_runs_rules_in_config_order() {
        let (mut s, root) = new_syncer(&["pylock", "python"]);
        // python reads what pylock writes: pylock must run first even when
        // named second.
        s.config.deps[1].sources = vec!["pylock.out".into()];
        s.only = vec!["python".into(), "pylock".into()];

        s.sync_deps().unwrap();
        for out in ["pylock.out", "python.out"] {
            assert!(root.path().join(out).exists(), "{out} not generated");
        }
    }

    #[test]
    fn a_rule_without_sources_is_skipped() {
        // Go enabled in a project with no go.mod: nothing to generate from.
        let (mut s, root) = new_syncer(&["go"]);
        std::fs::remove_file(root.path().join("go.src")).unwrap();

        let result = s.sync_deps().unwrap();
        assert!(result.errors.is_empty() && result.synced == 0, "{result:?}");
        let (_, stale) = s.check().unwrap();
        assert!(!stale);
    }

    #[test]
    fn a_missing_source_does_not_keep_a_rule_stale() {
        // A module without dependencies has a go.mod and no go.sum.
        let (mut s, _root) = new_syncer(&["go"]);
        s.config.deps[0].sources = vec!["go.src".into(), "go.sum".into()];
        s.sync_deps().unwrap();

        let (_, stale) = s.check().unwrap();
        assert!(!stale, "fresh after a sync");
    }

    // A rule whose target lists more sources: the target is stale when one
    // of them is newer, and when it lists none (written before the rule
    // named the key).
    #[test]
    fn target_listed_sources_make_a_rule_stale() {
        let (mut s, root) = new_syncer(&["rust"]);
        s.config.deps[0].target_sources = "manifests".into();
        s.config.deps[0].generator =
            vec!["echo".into(), r#"manifests = ["member/Cargo.toml"]"#.into()];
        let member = root.path().join("member/Cargo.toml");
        std::fs::create_dir_all(member.parent().unwrap()).unwrap();
        std::fs::write(&member, "[package]").unwrap();
        let past = SystemTime::now() - Duration::from_secs(3600);
        for file in [member.clone(), root.path().join("rust.src")] {
            set_mtime(&file, past);
        }
        let target = root.path().join("rust.out");

        // Written before the rule named the key: stale
        std::fs::write(&target, "schema_version = 1\n").unwrap();
        assert!(s.check().unwrap().1, "stale with no manifests listed");

        s.sync_deps().unwrap();
        assert!(!s.check().unwrap().1, "fresh after sync");
        assert_eq!(
            listed_sources(root.path(), &s.config.deps[0]).unwrap(),
            ["member/Cargo.toml"]
        );

        // A features-only edit to the member
        set_mtime(&member, SystemTime::now() + Duration::from_secs(3600));
        assert!(s.check().unwrap().1, "stale after the member changed");
    }

    #[test]
    fn a_failing_generator_leaves_the_target_alone() {
        let (mut s, root) = new_syncer(&["go"]);
        s.config.deps[0].generator = vec![
            "sh".into(),
            "-c".into(),
            "echo out; echo oops >&2; exit 2".into(),
        ];
        s.config.deps[0].target = "sub/dir/go.out".into();

        let result = s.sync_deps().unwrap();
        assert_eq!(result.synced, 0);
        let message = format!("{:#}", result.errors[0]);
        assert!(
            message.starts_with("go: regeneration failed: generator failed: exit status 2\noops"),
            "{message}"
        );
        assert!(
            root.path().join("sub/dir").is_dir(),
            "the target's directory is created first"
        );
        assert!(!root.path().join("sub/dir/go.out").exists());

        let rule = s.config.deps[0].clone();
        assert!(s.sync_rule(&rule).is_err());

        s.config.deps[0].generator = vec!["no-such-generator-anywhere".into()];
        let result = s.sync_deps().unwrap();
        let message = format!("{:#}", result.errors[0]);
        assert!(
            message.contains("executable file not found in $PATH"),
            "{message}"
        );
    }

    #[test]
    fn target_sources_must_be_an_array_of_paths() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deps.toml");
        for (content, ok) in [
            ("sources = [\"a\", \"b\"]\n", true),
            ("other = 1\n", true),
            ("sources = \"a\"\n", false),
            ("sources = [1]\n", false),
            ("sources = [\n", false),
        ] {
            std::fs::write(&target, content).unwrap();
            assert_eq!(target_sources(&target, "sources").is_ok(), ok, "{content}");
        }
        std::fs::write(&target, "other = 1\n").unwrap();
        assert_eq!(target_sources(&target, "sources").unwrap(), (vec![], false));
        std::fs::remove_file(&target).unwrap();
        assert_eq!(target_sources(&target, "sources").unwrap(), (vec![], false));
    }
}
