//! The buck2 daemons running on a project inside a mount
//!
//! A buck2 daemon keeps file descriptors and its view of the project from
//! when it started. When the composition view is remounted, a daemon on a
//! project inside it still holds the old mount, so turnkey-composed kills
//! it; the next buck2 command starts a fresh one.
//!
//! buck2 records each daemon in `<buck home>/buckd/<project root>/<isolation
//! dir>/buckd.info`, the project root's path without its leading `/`
//! (`InvocationPaths::daemon_dir` in buck2's
//! `app/buck2_common/src/invocation_paths.rs`). The file is JSON holding the
//! daemon's pid.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The file buck2 writes in a daemon's directory
const INFO_FILE: &str = "buckd.info";

/// A buck2 daemon, as its buckd.info records it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Daemon {
    /// The project the daemon runs on
    pub project_root: PathBuf,
    /// The daemon's isolation dir (`--isolation-dir`)
    pub isolation: String,
    /// The daemon's process id
    pub pid: u32,
}

#[derive(Deserialize)]
struct Info {
    pid: u32,
}

/// The daemons buck2 has recorded under buck_home (`~/.buck`) for projects
/// at or below root. A daemon may have exited since it was recorded; its
/// pid is then gone, or another process's.
///
/// root is matched as given and canonicalized, as buck2 records the project
/// root it resolved (`/tmp/x` is recorded as `/private/tmp/x` on macOS).
pub fn daemons_under(buck_home: &Path, root: &Path) -> Vec<Daemon> {
    let buckd = buck_home.join("buckd");
    let mut roots = vec![root.to_path_buf()];
    if let Ok(canonical) = root.canonicalize()
        && canonical != root
    {
        roots.push(canonical);
    }
    let mut daemons = Vec::new();
    for root in roots {
        let Ok(relative) = root.strip_prefix("/") else {
            continue;
        };
        collect(&buckd, &buckd.join(relative), &mut daemons);
    }
    daemons
}

/// Adds the daemons recorded in dir and below it to daemons
fn collect(buckd: &Path, dir: &Path, daemons: &mut Vec<Daemon>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    if let Some(daemon) = read_daemon(buckd, dir) {
        daemons.push(daemon);
    }
    for entry in entries.flatten() {
        // Not following symlinks: buck2 only creates directories here
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect(buckd, &entry.path(), daemons);
        }
    }
}

/// The daemon recorded in dir, an isolation dir under buckd, if it holds a
/// readable buckd.info
fn read_daemon(buckd: &Path, dir: &Path) -> Option<Daemon> {
    let info: Info = serde_json::from_slice(&std::fs::read(dir.join(INFO_FILE)).ok()?).ok()?;
    let isolation = dir.file_name()?.to_str()?.to_string();
    let project_root = Path::new("/").join(dir.parent()?.strip_prefix(buckd).ok()?);
    Some(Daemon {
        project_root,
        isolation,
        pid: info.pid,
    })
}

/// Whether args, a process's command line, is a buck2 daemon running with
/// isolation. Guards against killing whatever reused a dead daemon's pid.
pub fn is_daemon_command(args: &str, isolation: &str) -> bool {
    let mut words = args.split_whitespace();
    let Some(name) = words.next() else {
        return false;
    };
    // buck2, or on macOS buck2d[<project dir>], the name a daemon gives
    // its process
    if !name.contains("buck2") {
        return false;
    }
    let words: Vec<&str> = words.collect();
    words
        .windows(3)
        .any(|w| w == ["--isolation-dir", isolation, "daemon"])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(buck_home: &Path, daemon_dir: &str, pid: u32) {
        let dir = buck_home.join("buckd").join(daemon_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(INFO_FILE),
            format!(r#"{{"pid":{pid},"endpoint":"tcp:1","version":"v","auth_token":"t"}}"#),
        )
        .unwrap();
    }

    fn sorted(mut daemons: Vec<Daemon>) -> Vec<Daemon> {
        daemons.sort_by_key(|d| d.pid);
        daemons
    }

    /// Daemons on the root and on projects below it, in any isolation dir;
    /// none outside it
    #[test]
    fn finds_daemons_at_and_below_root() {
        let home = tempfile::tempdir().unwrap();
        record(home.path(), "mnt/proj/v2", 1);
        record(home.path(), "mnt/proj/.turnkey", 2);
        record(home.path(), "mnt/proj/sub/v2", 3);
        record(home.path(), "mnt/other/v2", 4);
        record(home.path(), "mnt/projx/v2", 5);

        let daemons = sorted(daemons_under(home.path(), Path::new("/mnt/proj")));
        assert_eq!(
            daemons,
            [
                Daemon {
                    project_root: "/mnt/proj".into(),
                    isolation: "v2".into(),
                    pid: 1,
                },
                Daemon {
                    project_root: "/mnt/proj".into(),
                    isolation: ".turnkey".into(),
                    pid: 2,
                },
                Daemon {
                    project_root: "/mnt/proj/sub".into(),
                    isolation: "v2".into(),
                    pid: 3,
                },
            ]
        );
    }

    /// buck2 records the canonical project root, so a root reached through
    /// a symlink still finds its daemons
    #[test]
    fn finds_daemons_recorded_under_the_canonical_root() {
        let home = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        let canonical = real.path().canonicalize().unwrap();
        let link = home.path().join("link");
        std::os::unix::fs::symlink(&canonical, &link).unwrap();
        record(
            home.path(),
            canonical
                .join("v2")
                .strip_prefix("/")
                .unwrap()
                .to_str()
                .unwrap(),
            7,
        );

        let daemons = daemons_under(home.path(), &link);
        assert_eq!(
            daemons,
            [Daemon {
                project_root: canonical,
                isolation: "v2".into(),
                pid: 7,
            }]
        );
    }

    /// No buckd directory, or a buckd.info buck2 left half-written, is no
    /// daemon rather than an error
    #[test]
    fn skips_what_it_cannot_read() {
        let home = tempfile::tempdir().unwrap();
        assert!(daemons_under(home.path(), Path::new("/mnt/proj")).is_empty());

        let dir = home.path().join("buckd/mnt/proj/v2");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(INFO_FILE), "{\"pid\":").unwrap();
        assert!(daemons_under(home.path(), Path::new("/mnt/proj")).is_empty());
    }

    #[test]
    fn recognises_a_daemon_by_its_command_line() {
        let args = r#"buck2d[proj] --isolation-dir .turnkey daemon --daemon-id 209d {"paranoid":false} --skip-macos-qos"#;
        assert!(is_daemon_command(args, ".turnkey"));
        assert!(is_daemon_command(
            "/nix/store/abc-buck2/bin/buck2 --isolation-dir v2 daemon --daemon-id 1",
            "v2"
        ));
        // Another isolation dir's daemon on the same project
        assert!(!is_daemon_command(args, "v2"));
        // Its forkserver, and processes that reused a daemon's pid
        assert!(!is_daemon_command(
            "(buck2-forkserver) forkserver --isolation-dir .turnkey --fd 22",
            ".turnkey"
        ));
        assert!(!is_daemon_command(
            "vim --isolation-dir .turnkey daemon",
            ".turnkey"
        ));
        assert!(!is_daemon_command("", ".turnkey"));
    }
}
