//! Runs a native tool (go, cargo, uv) for tw, and syncs the project's deps
//! when the tool changes the files its wrapper rule watches
//!
//! A [`Wrapper`] runs the tool, and when the tool was run with one of its
//! wrapper rule's mutating subcommands and changed a watched file, runs the
//! rule's post-commands and syncs the rule's deps rule. Either way it
//! returns the tool's exit code. Running a tool and syncing are adapters
//! ([`Exec`], [`Sync`]), so the module is tested with a fake tool.
//!
//! A change is a change of content: the watched files are hashed before and
//! after the tool runs ([`crate::snapshot`]), where `tk sync` compares
//! mtimes ([`project_sync::staleness`]). The two answer different
//! questions. `tk sync` asks, with no memory of earlier runs, whether a
//! target is older than its sources, which a stat of each file answers
//! before every build. tw asks whether this one run changed the files, and
//! has their state from before it: comparing content keeps a tool that
//! rewrites a file without changing it from setting off a regeneration,
//! which can mean prefetching every dependency, and doesn't depend on the
//! filesystem's mtime resolution.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Result, anyhow};
use project_sync::config::{self, Config, WrapperRule};
use project_sync::launch::{self, Launcher};
use project_sync::syncer::{self, Syncer};

use crate::snapshot;

/// Runs tool with args in dir (the working directory when None), connected
/// to the caller's stdio, and returns its exit code
pub type Exec = Box<dyn FnMut(Option<&Path>, &OsStr, &[OsString]) -> i32>;

/// Regenerates the deps rule named rule, then every rule left stale by it,
/// saying what it does on log (more of it when verbose)
pub type Sync = Box<dyn FnMut(&str, bool, &Log) -> Result<()>>;

/// Where the wrapper's messages go, shared with the syncer it runs
#[derive(Clone)]
pub struct Log(Rc<RefCell<dyn Write>>);

impl Log {
    /// A log writing to w
    pub fn new(w: impl Write + 'static) -> Log {
        Log(Rc::new(RefCell::new(w)))
    }
}

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

/// Runs tools for one project
pub struct Wrapper {
    /// The project root, where watched files are read and post-commands
    /// run; None outside a project
    pub root: Option<PathBuf>,
    /// The wrapper rules; a tool without one is passed through
    pub config: Config,
    /// Runs the tool and the post-commands
    pub exec: Exec,
    /// Syncs a wrapper rule's deps rule
    pub sync: Sync,
    /// Pass every tool through
    pub no_sync: bool,
    /// Say what the wrapper does
    pub verbose: bool,
    /// Where the wrapper's messages go
    pub log: Log,
}

impl Wrapper {
    /// The wrapper for the project dir is in, syncing with the project's
    /// syncer, whose generators launcher starts. Outside a project, or when
    /// its sync.toml doesn't load, the wrapper passes every tool through.
    pub fn open(dir: &Path, exec: Exec, log: Log, launcher: Launcher) -> Wrapper {
        let mut w = Wrapper {
            root: None,
            config: Config::default(),
            exec,
            sync: Box::new(|_, _, _| Err(anyhow!("no project root"))),
            no_sync: false,
            verbose: false,
            log,
        };
        let Some(root) = config::find_root(dir) else {
            return w;
        };
        w.root = Some(root.clone());
        match Syncer::load(&root, launcher) {
            Err(e) => {
                w.logf(format_args!("tw: {e:#}\n"));
                let message = format!("{e:#}");
                w.sync = Box::new(move |_, _, _| Err(anyhow!("{message}")));
            }
            Ok(mut s) => {
                w.config = s.config.clone();
                w.sync = Box::new(move |rule, verbose, log| sync_rule(&mut s, rule, verbose, log));
            }
        }
        w
    }

    /// Runs tool with args and returns its exit code, syncing when a
    /// mutating subcommand changed a watched file
    pub fn run(&mut self, tool: &OsStr, args: &[OsString]) -> i32 {
        let rule = tool
            .to_str()
            .and_then(|t| self.config.find_wrapper(t))
            .cloned();
        let Some(root) = self.root.clone() else {
            self.verbosef(format_args!("tw: no project root found, passing through\n"));
            return (self.exec)(None, tool, args);
        };
        let Some(rule) = rule else {
            self.verbosef(format_args!(
                "tw: no wrapper rule for {:?}, passing through\n",
                tool.to_string_lossy()
            ));
            return (self.exec)(None, tool, args);
        };
        if self.no_sync {
            self.verbosef(format_args!("tw: sync disabled, passing through\n"));
            return (self.exec)(None, tool, args);
        }
        let subcommand = args.first().map(|a| a.as_os_str()).unwrap_or_default();
        if !subcommand
            .to_str()
            .is_some_and(|s| rule.is_mutating_subcommand(s))
        {
            self.verbosef(format_args!(
                "tw: {:?} is not a mutating subcommand, passing through\n",
                subcommand.to_string_lossy()
            ));
            return (self.exec)(None, tool, args);
        }

        let watched = self.watched(&root, &rule);
        self.verbosef(format_args!(
            "tw: capturing state of {}\n",
            go_list(&watched)
        ));
        let before = match snapshot::capture(&root, &watched) {
            Ok(before) => before,
            Err(e) => {
                self.logf(format_args!("tw: failed to capture before state: {e}\n"));
                return (self.exec)(None, tool, args);
            }
        };
        let exit_code = (self.exec)(None, tool, args);
        let after = match snapshot::capture(&root, &watched) {
            Ok(after) => after,
            Err(e) => {
                self.logf(format_args!("tw: failed to capture after state: {e}\n"));
                return exit_code;
            }
        };
        if !snapshot::changed(&before, &after) {
            self.verbosef(format_args!("tw: no changes detected\n"));
            return exit_code;
        }

        self.verbosef(format_args!(
            "tw: detected changes in {}\n",
            go_list(&watched)
        ));
        // Post-commands (go mod tidy after go get) settle the files before
        // they are synced; one failing doesn't stop the sync.
        for post_cmd in &rule.post_commands {
            let parts: Vec<OsString> = post_cmd.split_whitespace().map(OsString::from).collect();
            let Some((program, post_args)) = parts.split_first() else {
                continue;
            };
            self.verbosef(format_args!("tw: running post-command: {post_cmd}\n"));
            let code = (self.exec)(Some(&root), program, post_args);
            if code != 0 {
                self.logf(format_args!(
                    "tw: post-command {post_cmd:?} failed with exit code {code}\n"
                ));
            }
        }
        self.verbosef(format_args!("tw: running sync\n"));
        let log = self.log.clone();
        if let Err(e) = (self.sync)(&rule.deps_rule, self.verbose, &log) {
            self.logf(format_args!("tw: sync failed: {e:#}\n"));
        }
        exit_code
    }

    /// The files rule watches: its watch_files, and the sources its deps
    /// rule's target lists (a Go workspace member's go.mod), which make
    /// that target stale as much as the fixed ones do
    fn watched(&mut self, root: &Path, rule: &WrapperRule) -> Vec<String> {
        let mut files = rule.watch_files.clone();
        let Some(deps) = self.config.find_deps_rule(&rule.deps_rule).cloned() else {
            return files;
        };
        match syncer::listed_sources(root, &deps) {
            Ok(listed) => files.extend(listed),
            Err(e) => self.logf(format_args!("tw: {e:#}\n")),
        }
        files
    }

    fn logf(&mut self, message: std::fmt::Arguments<'_>) {
        let _ = self.log.write_fmt(message);
    }

    fn verbosef(&mut self, message: std::fmt::Arguments<'_>) {
        if self.verbose {
            self.logf(message);
        }
    }
}

/// Regenerates the deps rule named name, then every rule left stale by it:
/// a rule whose source is that rule's target (python-deps.toml from
/// pylock.toml) comes after it in sync.toml
fn sync_rule(s: &mut Syncer, name: &str, verbose: bool, log: &Log) -> Result<()> {
    let Some(rule) = s.config.find_deps_rule(name).cloned() else {
        return Err(anyhow!("deps rule {name:?} not found"));
    };
    s.verbose = verbose;
    s.output = Box::new(log.clone());
    s.sync_rule(&rule)?;
    s.quiet = !verbose;
    let result = s.sync_deps()?;
    if result.errors.is_empty() {
        return Ok(());
    }
    let messages: Vec<String> = result.errors.iter().map(|e| format!("{e:#}")).collect();
    Err(anyhow!("{}", messages.join("\n")))
}

/// A list of strings as Go's `%v` prints it
fn go_list(items: &[String]) -> String {
    format!("[{}]", items.join(" "))
}

/// Runs the real tool: the one a shell wrapper names in
/// `TURNKEY_REAL_<TOOL>`, so tw doesn't run the wrapper again, or else the
/// one on `PATH`. It forwards SIGINT, SIGTERM and SIGHUP to the tool.
pub struct RealExec {
    launcher: Launcher,
    environ: HashMap<OsString, OsString>,
}

impl RealExec {
    /// The real tools of the environment environ (the process's, read once
    /// by `main`), found on its `PATH` unless a `TURNKEY_REAL_<TOOL>` names
    /// them
    pub fn new(environ: impl IntoIterator<Item = (OsString, OsString)>) -> RealExec {
        let mut env = HashMap::new();
        for (key, value) in environ {
            // The first of two definitions wins, as with getenv
            env.entry(key).or_insert(value);
        }
        RealExec {
            launcher: Launcher::new(env.get(OsStr::new("PATH")).cloned()),
            environ: env,
        }
    }

    /// The launcher of this environment, for the syncer's generators
    pub fn launcher(&self) -> Launcher {
        self.launcher.clone()
    }

    /// Runs tool with args in dir, and returns its exit code: 1 when it
    /// can't be started, -1 when a signal killed it
    pub fn run(&self, dir: Option<&Path>, tool: &OsStr, args: &[OsString]) -> i32 {
        let Some(path) = self.find_real_tool(tool) else {
            eprintln!("tw: {} not found in PATH", tool.to_string_lossy());
            return 1;
        };
        let Ok(mut cmd) = self.launcher.command(path.as_os_str(), args, dir) else {
            return 1;
        };
        match launch::run_forwarding_signals(&mut cmd) {
            Ok(status) => launch::exit_code(status),
            Err(_) => 1,
        }
    }

    /// The path of the real tool, if there is one
    fn find_real_tool(&self, name: &OsStr) -> Option<PathBuf> {
        let key = format!("TURNKEY_REAL_{}", name.to_string_lossy().to_uppercase());
        if let Some(path) = self.environ.get(OsStr::new(&key))
            && !path.is_empty()
            && std::fs::metadata(path).is_ok()
        {
            return Some(PathBuf::from(path));
        }
        self.launcher.look_path(name).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a fake tool ran, one line per run
    type Ran = Rc<RefCell<Vec<String>>>;

    /// A fake go tool for a project at root: `go get` adds a require line
    /// to go.mod, anything else leaves it be, and every run exits with the
    /// exit code it is given
    fn fake_go(root: PathBuf, exit_code: Rc<RefCell<i32>>, ran: Ran) -> Exec {
        Box::new(move |dir, tool, args| {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let dir = dir.map(|d| d.display().to_string()).unwrap_or_default();
            ran.borrow_mut().push(
                format!("{dir} {} {}", tool.to_string_lossy(), args.join(" "))
                    .trim()
                    .to_owned(),
            );
            if tool == "go" && args.first().map(String::as_str) == Some("get") {
                let path = root.join("go.mod");
                let mut data = std::fs::read_to_string(&path).unwrap_or_default();
                data.push_str(&format!("require {} v1.0.0\n", args[1]));
                std::fs::write(&path, data).unwrap();
            }
            *exit_code.borrow()
        })
    }

    struct Fixture {
        w: Wrapper,
        ran: Ran,
        exit_code: Rc<RefCell<i32>>,
        synced: Rc<RefCell<Vec<String>>>,
        _dir: tempfile::TempDir,
    }

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    /// A wrapper for a project holding a go.mod, whose go wrapper rule runs
    /// `go mod tidy` and syncs the "go" rule, with the fake go tool and a
    /// sync recording the rules it was asked for
    fn new_wrapper() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("go.mod"), "module example.com/m\n").unwrap();
        let ran: Ran = Rc::default();
        let exit_code = Rc::new(RefCell::new(0));
        let synced: Rc<RefCell<Vec<String>>> = Rc::default();
        let recorded = synced.clone();
        let w = Wrapper {
            root: Some(root.clone()),
            config: Config {
                wrappers: vec![WrapperRule {
                    name: "go".into(),
                    command: "go".into(),
                    mutating_subcommands: vec!["get".into(), "mod".into()],
                    watch_files: vec!["go.mod".into(), "go.sum".into()],
                    deps_rule: "go".into(),
                    post_commands: vec!["go mod tidy".into()],
                }],
                ..Config::default()
            },
            exec: fake_go(root, exit_code.clone(), ran.clone()),
            sync: Box::new(move |rule, _, _| {
                recorded.borrow_mut().push(rule.to_owned());
                Ok(())
            }),
            no_sync: false,
            verbose: false,
            log: Log::new(std::io::sink()),
        };
        Fixture {
            w,
            ran,
            exit_code,
            synced,
            _dir: dir,
        }
    }

    #[test]
    fn run_without_change_does_not_sync() {
        let mut f = new_wrapper();
        assert_eq!(f.w.run(OsStr::new("go"), &args(&["mod", "verify"])), 0);
        assert!(f.synced.borrow().is_empty(), "go.mod didn't change");
        assert_eq!(*f.ran.borrow(), ["go mod verify"], "no post-commands");
    }

    #[test]
    fn run_with_change_runs_post_commands_then_syncs_the_rule() {
        let mut f = new_wrapper();
        assert_eq!(
            f.w.run(OsStr::new("go"), &args(&["get", "example.com/dep"])),
            0
        );
        let root = f.w.root.clone().unwrap();
        assert_eq!(
            *f.ran.borrow(),
            [
                "go get example.com/dep".to_owned(),
                format!("{} go mod tidy", root.display())
            ]
        );
        assert_eq!(*f.synced.borrow(), ["go"]);
    }

    #[test]
    fn run_passes_the_tools_exit_code_through() {
        let mut f = new_wrapper();
        *f.exit_code.borrow_mut() = 3;
        let recorded = f.synced.clone();
        f.w.sync = Box::new(move |rule, _, _| {
            recorded.borrow_mut().push(rule.to_owned());
            Err(anyhow!("generator failed"))
        });

        // The tool changed go.mod before failing: its files are synced all
        // the same, and neither the post-command's nor the sync's failure
        // hides the tool's exit code.
        assert_eq!(
            f.w.run(OsStr::new("go"), &args(&["get", "example.com/dep"])),
            3
        );
        assert_eq!(f.synced.borrow().len(), 1, "the go rule synced once");
        assert_eq!(f.w.run(OsStr::new("go"), &args(&["build", "./..."])), 3);
    }

    #[test]
    fn run_passes_through_without_syncing() {
        for (name, tool, no_sync) in [
            ("no-sync", "go", true),
            ("tool without rules", "gofmt", false),
        ] {
            let mut f = new_wrapper();
            f.w.no_sync = no_sync;

            f.w.run(OsStr::new(tool), &args(&["get", "example.com/dep"]));
            assert!(f.synced.borrow().is_empty(), "{name}");
            assert_eq!(
                *f.ran.borrow(),
                [format!("{tool} get example.com/dep")],
                "{name}"
            );
        }
    }

    #[test]
    fn run_does_not_watch_a_non_mutating_subcommand() {
        let mut f = new_wrapper();
        f.w.config.wrappers[0].mutating_subcommands = vec!["mod".into()];

        // get changes go.mod, but the rule doesn't say it may.
        f.w.run(OsStr::new("go"), &args(&["get", "example.com/dep"]));
        assert!(
            f.synced.borrow().is_empty(),
            "get isn't a mutating subcommand"
        );
    }

    fn launcher() -> Launcher {
        Launcher::new(std::env::var_os("PATH"))
    }

    #[test]
    fn open_outside_a_project_passes_through() {
        let dir = tempfile::tempdir().unwrap();
        // The temporary directory may itself be inside a project
        if config::find_root(dir.path()).is_some() {
            return;
        }
        let ran: Ran = Rc::default();
        let exec = fake_go(
            dir.path().to_path_buf(),
            Rc::new(RefCell::new(2)),
            ran.clone(),
        );

        let mut w = Wrapper::open(dir.path(), exec, Log::new(std::io::sink()), launcher());
        assert_eq!(
            w.run(OsStr::new("go"), &args(&["get", "example.com/dep"])),
            2
        );
        assert_eq!(*ran.borrow(), ["go get example.com/dep"]);
    }

    #[test]
    fn open_syncs_with_the_projects_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".turnkey")).unwrap();
        std::fs::write(
            root.join(config::DEFAULT_CONFIG_PATH),
            r#"
[[deps]]
name = "go"
sources = ["go.mod"]
target = "go-deps.toml"
generator = ["sh", "-c", "wc -l < go.mod"]

[[wrappers]]
name = "go"
command = "go"
mutating_subcommands = ["get"]
watch_files = ["go.mod"]
deps_rule = "go"
"#,
        )
        .unwrap();
        std::fs::write(root.join("go.mod"), "module example.com/m\n").unwrap();
        let sub = root.join("src");
        std::fs::create_dir(&sub).unwrap();
        let exec = fake_go(root.to_path_buf(), Rc::new(RefCell::new(0)), Rc::default());

        let mut w = Wrapper::open(&sub, exec, Log::new(std::io::sink()), launcher());
        assert_eq!(
            w.run(OsStr::new("go"), &args(&["get", "example.com/dep"])),
            0
        );
        let got =
            std::fs::read_to_string(root.join("go-deps.toml")).expect("go-deps.toml generated");
        assert_eq!(got.trim(), "2", "the line count of the changed go.mod");
    }

    #[test]
    fn open_with_an_invalid_sync_toml_passes_through() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".turnkey")).unwrap();
        // A wrapper rule whose deps rule doesn't exist
        std::fs::write(
            root.join(config::DEFAULT_CONFIG_PATH),
            "[[wrappers]]\nname = \"go\"\ncommand = \"go\"\nmutating_subcommands = [\"get\"]\nwatch_files = [\"go.mod\"]\ndeps_rule = \"go\"\n",
        )
        .unwrap();
        let ran: Ran = Rc::default();
        let exec = fake_go(root.to_path_buf(), Rc::new(RefCell::new(0)), ran.clone());
        let buffer = Rc::new(RefCell::new(Vec::<u8>::new()));

        let log = Log(buffer.clone());
        let mut w = Wrapper::open(root, exec, log, launcher());
        assert_eq!(w.root.as_deref(), Some(root));
        w.run(OsStr::new("go"), &args(&["get", "example.com/dep"]));
        assert_eq!(*ran.borrow(), ["go get example.com/dep"], "passed through");
        let logged = String::from_utf8(buffer.borrow().clone()).unwrap();
        assert!(logged.starts_with("tw: invalid sync config: "), "{logged}");
    }

    #[test]
    fn run_watches_the_sources_the_deps_file_lists() {
        let mut f = new_wrapper();
        // A go.work workspace: go-deps.toml lists a member's go.mod, which
        // `go get` in that member changes
        f.w.config.deps = vec![config::DepsRule {
            name: "go".into(),
            sources: vec!["go.work".into()],
            target: "go-deps.toml".into(),
            target_sources: "sources".into(),
            ..config::DepsRule::default()
        }];
        f.w.config.wrappers[0].watch_files = vec!["go.work".into()];
        let root = f.w.root.clone().unwrap();
        std::fs::write(root.join("go-deps.toml"), "sources = [\"go.mod\"]\n").unwrap();

        f.w.run(OsStr::new("go"), &args(&["get", "example.com/dep"]));
        assert_eq!(
            *f.synced.borrow(),
            ["go"],
            "go.mod is listed in go-deps.toml (ran {:?})",
            f.ran.borrow()
        );
    }

    #[test]
    fn real_exec_prefers_turnkey_real_tool() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-go");
        std::fs::write(&real, "").unwrap();
        let exec = RealExec::new([
            (OsString::from("PATH"), OsString::from("")),
            (
                OsString::from("TURNKEY_REAL_GO"),
                real.clone().into_os_string(),
            ),
            (
                OsString::from("TURNKEY_REAL_GO"),
                OsString::from("/second/wins/not"),
            ),
            (
                OsString::from("TURNKEY_REAL_CARGO"),
                OsString::from("/no/such/cargo"),
            ),
        ]);
        assert_eq!(exec.find_real_tool(OsStr::new("go")), Some(real));
        assert_eq!(
            exec.find_real_tool(OsStr::new("cargo")),
            None,
            "not on PATH either"
        );
        assert_eq!(exec.run(None, OsStr::new("cargo"), &[]), 1);
    }
}
