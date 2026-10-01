//! Starting a child process the way Go's `os/exec` does
//!
//! What a child sees of how it was started is part of what `tk` and `tw`
//! promise: its argv, its cwd, and the environment variables they set. The
//! Go versions start children with `exec.Command`, so this module does
//! what that does:
//!
//! - a name without a slash is looked up on the `PATH` [`Launcher`] was
//!   given, and a match relative to the current directory is refused
//!   (`exec.ErrDot`);
//! - `argv[0]` is the name as given, not the path it resolved to;
//! - a child run in another directory gets `PWD` set to that directory,
//!   since POSIX has `PWD` name the working directory.
//!
//! The `PATH` is an input, not read here: the binary's `main` reads its
//! environment once and hands it down.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use gostd::filepath;

/// Starts child processes, finding programs on a `PATH`
#[derive(Debug, Clone, Default)]
pub struct Launcher {
    path: Option<OsString>,
}

/// Why a program wasn't found
#[derive(Debug)]
pub enum LookPathError {
    /// No directory of the `PATH` holds an executable of that name
    NotFound(OsString),
    /// The executable found is relative to the current directory: a
    /// relative directory on the `PATH` (`exec.ErrDot`)
    Dot(OsString),
    /// The named file can't be run
    Io(OsString, std::io::Error),
}

impl fmt::Display for LookPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LookPathError::NotFound(name) => {
                write!(f, "exec: {name:?}: executable file not found in $PATH")
            }
            LookPathError::Dot(name) => write!(
                f,
                "exec: {name:?}: cannot run executable found relative to current directory"
            ),
            LookPathError::Io(name, err) => write!(f, "exec: {name:?}: {err}"),
        }
    }
}

impl std::error::Error for LookPathError {}

impl Launcher {
    /// A launcher searching path, the value of a `PATH` variable (None when
    /// there is none, which searches nothing)
    pub fn new(path: Option<OsString>) -> Launcher {
        Launcher { path }
    }

    /// `exec.LookPath`: the executable named file, itself when it holds a
    /// slash, else the first on the `PATH`
    pub fn look_path(&self, file: &OsStr) -> Result<PathBuf, LookPathError> {
        if file.as_bytes().contains(&b'/') {
            return match find_executable(Path::new(file)) {
                Ok(()) => Ok(PathBuf::from(file)),
                Err(e) => Err(LookPathError::Io(file.to_owned(), e)),
            };
        }
        let path = self.path.as_deref().unwrap_or_default();
        for dir in path.as_bytes().split(|&c| c == b':') {
            // Unix shell semantics: path element "" means "."
            let dir = if dir.is_empty() { b".".as_slice() } else { dir };
            let candidate = filepath::join(&[Path::new(OsStr::from_bytes(dir)), Path::new(file)]);
            if find_executable(&candidate).is_ok() {
                if !filepath::is_abs(&candidate) {
                    return Err(LookPathError::Dot(file.to_owned()));
                }
                return Ok(candidate);
            }
        }
        Err(LookPathError::NotFound(file.to_owned()))
    }

    /// `exec.Command(name, args...)` with `Dir` set to dir: the program
    /// looked up on the `PATH` when name is a bare name, argv[0] the name
    /// as given, and `PWD` set to dir when there is one
    ///
    /// The command inherits this process's environment and stdio, as with
    /// `exec.Command` before its streams are set.
    pub fn command<S: AsRef<OsStr>>(
        &self,
        name: &OsStr,
        args: &[S],
        dir: Option<&Path>,
    ) -> Result<Command, LookPathError> {
        let program = if filepath::base(Path::new(name)).as_os_str() == name {
            self.look_path(name)?
        } else {
            PathBuf::from(name)
        };
        let mut cmd = Command::new(&program);
        cmd.arg0(name);
        cmd.args(args);
        if let Some(dir) = dir.filter(|d| !d.as_os_str().is_empty()) {
            cmd.current_dir(dir);
            // On POSIX platforms, PWD is "an absolute pathname of the
            // current working directory": os/exec updates it when it
            // changes the directory and leaves the environment otherwise
            // as it is
            cmd.env("PWD", absolute(dir));
        }
        Ok(cmd)
    }
}

/// `filepath.Abs`: dir cleaned, against the current directory when it is
/// relative
fn absolute(dir: &Path) -> PathBuf {
    if filepath::is_abs(dir) {
        return filepath::clean(dir);
    }
    match std::env::current_dir() {
        Ok(cwd) => filepath::join(&[cwd.as_path(), dir]),
        Err(_) => filepath::clean(dir),
    }
}

/// Whether file is a file this process may execute
fn find_executable(file: &Path) -> std::io::Result<()> {
    let meta = std::fs::metadata(file)?;
    if meta.is_dir() {
        return Err(std::io::Error::from_raw_os_error(libc::EISDIR));
    }
    let c_path = std::ffi::CString::new(file.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: c_path is a valid NUL-terminated string
    if unsafe { libc::access(c_path.as_ptr(), libc::X_OK) } == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ENOSYS) | Some(libc::EPERM) if meta.permissions().mode() & 0o111 != 0 => Ok(()),
        Some(libc::ENOSYS) | Some(libc::EPERM) => {
            Err(std::io::Error::from_raw_os_error(libc::EACCES))
        }
        _ => Err(err),
    }
}

/// An exit status as Go's `ExitCode` reads it: the code, or -1 for a child
/// killed by a signal
pub fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

/// An exit status as a shell reports it: the code, or 128 plus the signal
/// for a child killed by one
pub fn shell_exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signal)) => 128 + signal,
        (None, None) => -1,
    }
}

/// How a command that ran failed, as Go's `*exec.ExitError` prints it
pub fn exit_error(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit status {code}"),
        (None, Some(signal)) => format!("signal: {}", signal_name(signal)),
        (None, None) => "exit status -1".to_string(),
    }
}

/// A signal's name, as Go's `syscall.Signal` prints the common ones
fn signal_name(signal: i32) -> String {
    match signal {
        libc::SIGHUP => "hangup".to_string(),
        libc::SIGINT => "interrupt".to_string(),
        libc::SIGKILL => "killed".to_string(),
        libc::SIGTERM => "terminated".to_string(),
        n => format!("signal {n}"),
    }
}

/// `os.Getwd`: the working directory, named as `$PWD` (pwd) names it when
/// that is the same directory (a path through a symbolic link, as the
/// shell shows it), else as the system does
pub fn getwd(pwd: Option<&OsStr>) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let dot = std::fs::metadata(".")?;
    if let Some(dir) = pwd.filter(|d| d.as_bytes().first() == Some(&b'/'))
        && let Ok(d) = std::fs::metadata(dir)
        && d.dev() == dot.dev()
        && d.ino() == dot.ino()
    {
        return Ok(PathBuf::from(dir));
    }
    std::env::current_dir()
}

/// The child the signal handler forwards to; 0 when there is none
static CHILD: AtomicI32 = AtomicI32::new(0);

/// The signals the handler forwards, one bit per signal number; the
/// others it catches it drops
static FORWARD_MASK: AtomicU64 = AtomicU64::new(0);

const CAUGHT: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

extern "C" fn forward(signal: libc::c_int) {
    let pid = CHILD.load(Ordering::SeqCst);
    if pid > 0 && FORWARD_MASK.load(Ordering::SeqCst) & (1 << signal) != 0 {
        // SAFETY: kill is async-signal-safe
        unsafe {
            libc::kill(pid, signal);
        }
    }
}

/// Runs cmd to completion, forwarding SIGINT, SIGTERM and SIGHUP to it
///
/// While it runs, those signals don't stop this process: a terminal's ^C
/// reaches the child (once from the terminal, once forwarded, as with the
/// Go tools), and this process returns the child's status. A signal
/// arriving before the child started is dropped. Meant for one child at a
/// time: the handlers are the process's.
pub fn run_forwarding_signals(cmd: &mut Command) -> std::io::Result<ExitStatus> {
    run_catching_signals(cmd, &CAUGHT)
}

/// Runs cmd to completion as [`run_forwarding_signals`] does, except that
/// SIGINT is not forwarded: ^C reaches the child directly through the
/// terminal's process group, and forwarding it too would read as a second
/// ^C. It still doesn't stop this process.
pub fn run_leaving_interrupts(cmd: &mut Command) -> std::io::Result<ExitStatus> {
    run_catching_signals(cmd, &[libc::SIGTERM, libc::SIGHUP])
}

/// Runs cmd to completion, catching SIGINT, SIGTERM and SIGHUP and
/// forwarding those in forwarded to it
fn run_catching_signals(
    cmd: &mut Command,
    forwarded: &[libc::c_int],
) -> std::io::Result<ExitStatus> {
    FORWARD_MASK.store(
        forwarded.iter().fold(0, |mask, &signal| mask | 1 << signal),
        Ordering::SeqCst,
    );
    // SAFETY: sigaction with a zeroed struct, an empty mask and a handler
    // that only calls async-signal-safe functions
    let previous: Vec<libc::sigaction> = CAUGHT
        .iter()
        .map(|&signal| unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = forward as *const () as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            let mut old: libc::sigaction = std::mem::zeroed();
            libc::sigaction(signal, &action, &mut old);
            old
        })
        .collect();

    let result = cmd.spawn().and_then(|mut child| {
        CHILD.store(child.id() as i32, Ordering::SeqCst);
        let status = child.wait();
        CHILD.store(0, Ordering::SeqCst);
        status
    });

    for (&signal, old) in CAUGHT.iter().zip(&previous) {
        // SAFETY: restores the action saved above
        unsafe {
            libc::sigaction(signal, old, std::ptr::null_mut());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty script at `path`, written by a child shell so this test
    /// binary never holds it open for writing: a test forking in another
    /// thread meanwhile would inherit that descriptor, and running the
    /// script while it is open fails with ETXTBSY (rust-lang/rust#114554).
    fn executable(path: &Path) {
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"printf '#!/bin/sh\n' > "$1" && chmod 755 "$1""#,
                "sh",
            ])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success(), "writing {}", path.display());
    }

    #[test]
    fn look_path_takes_the_first_match_on_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        executable(&b.join("tool"));
        std::fs::write(a.join("tool"), "not executable").unwrap();

        let path = OsString::from(format!("{}:{}", a.display(), b.display()));
        let launcher = Launcher::new(Some(path));
        assert_eq!(
            launcher.look_path(OsStr::new("tool")).unwrap(),
            b.join("tool")
        );
        assert!(matches!(
            launcher.look_path(OsStr::new("missing")),
            Err(LookPathError::NotFound(_))
        ));
        assert!(matches!(
            Launcher::new(None).look_path(OsStr::new("tool")),
            Err(LookPathError::NotFound(_))
        ));
    }

    #[test]
    fn a_name_with_a_slash_is_not_looked_up() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = tmp.path().join("tool");
        executable(&tool);
        let launcher = Launcher::new(None);
        assert_eq!(launcher.look_path(tool.as_os_str()).unwrap(), tool);
        assert!(matches!(
            launcher.look_path(tmp.path().as_os_str()),
            Err(LookPathError::Io(..))
        ));
    }

    #[test]
    fn a_command_run_elsewhere_gets_pwd() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = filepath::clean(tmp.path());
        let launcher = Launcher::new(Some(OsString::from("/bin:/usr/bin")));

        let cmd = launcher
            .command(OsStr::new("sh"), &["-c", "true"], Some(&dir.join("./")))
            .unwrap();
        let pwd: Vec<_> = cmd.get_envs().filter(|(k, _)| *k == "PWD").collect();
        assert_eq!(pwd, [(OsStr::new("PWD"), Some(dir.as_os_str()))]);
        assert_eq!(cmd.get_current_dir(), Some(dir.join("./").as_path()));

        let cmd = launcher
            .command(OsStr::new("sh"), &["-c", "true"], None)
            .unwrap();
        assert_eq!(cmd.get_envs().count(), 0, "no PWD without a directory");
    }

    #[test]
    fn exit_codes_read_as_go_reads_them() {
        let launcher = Launcher::new(Some(OsString::from("/bin:/usr/bin")));
        let mut cmd = launcher
            .command(OsStr::new("sh"), &["-c", "exit 3"], None)
            .unwrap();
        assert_eq!(exit_code(run_forwarding_signals(&mut cmd).unwrap()), 3);
        let mut cmd = launcher
            .command(OsStr::new("sh"), &["-c", "kill -9 $$"], None)
            .unwrap();
        assert_eq!(exit_code(run_forwarding_signals(&mut cmd).unwrap()), -1);
    }

    #[test]
    fn shell_exit_codes_count_signals_from_128() {
        let launcher = Launcher::new(Some(OsString::from("/bin:/usr/bin")));
        let mut cmd = launcher
            .command(OsStr::new("sh"), &["-c", "exit 3"], None)
            .unwrap();
        let status = run_leaving_interrupts(&mut cmd).unwrap();
        assert_eq!(shell_exit_code(status), 3);
        assert_eq!(exit_error(status), "exit status 3");
        let mut cmd = launcher
            .command(OsStr::new("sh"), &["-c", "kill -9 $$"], None)
            .unwrap();
        let status = run_leaving_interrupts(&mut cmd).unwrap();
        assert_eq!(shell_exit_code(status), 128 + 9);
        assert_eq!(exit_error(status), "signal: killed");
    }

    #[test]
    fn getwd_takes_pwd_only_when_it_names_the_working_directory() {
        let physical = std::env::current_dir().unwrap();
        assert_eq!(getwd(None).unwrap(), physical);
        assert_eq!(getwd(Some(OsStr::new("relative"))).unwrap(), physical);
        // Another directory
        assert_eq!(getwd(Some(OsStr::new("/"))).unwrap(), physical);
        // Not cleaned: the root finder cleans it
        let unclean = physical.join(".");
        assert_eq!(getwd(Some(unclean.as_os_str())).unwrap(), unclean);
    }
}
