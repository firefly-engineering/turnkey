//! Restarting buck2's daemon when a cell symlink changes
//!
//! When Nix rebuilds a dependency cell (a fixup change, a `nix flake
//! update`), a `.turnkey/<cell>` symlink changes to point to a new store
//! path. buck2's daemon doesn't notice: it caches the resolved cell
//! contents from the old path. [`check`] compares the current symlink
//! targets with those saved in a state file, and kills the daemon when any
//! has changed. Each isolation directory has a daemon of its own, so each
//! has its own state file.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use crate::Error;
use crate::fsutil::write_file;

const TURNKEY_DIR: &str = ".turnkey";
const STATE_FILE_NAME: &str = ".cell-targets";
const NIX_STORE_DIR: &str = "/nix/store/";

/// Compares the cell symlinks' targets with the state saved for
/// `isolation_dir`'s daemon (`None` for the one buck2 picks without
/// `--isolation-dir`). When any has changed, it calls `kill_daemon`, which
/// must kill that daemon (best-effort: it reports nothing back), and saves
/// the new state. On the first run (no state file), it saves the state
/// without killing.
///
/// `verbose` and `quiet` control what it writes to `w`. Errors are only
/// for I/O failures, not for missing state.
pub fn check(
    root: &Path,
    isolation_dir: Option<&str>,
    verbose: bool,
    quiet: bool,
    w: &mut dyn Write,
    kill_daemon: &mut dyn FnMut(),
) -> Result<(), Error> {
    let current = read_symlink_targets(root);
    if current.is_empty() {
        if verbose {
            let _ = writeln!(
                w,
                "tk: no Nix store symlinks found, skipping cell freshness check"
            );
        }
        return Ok(());
    }

    let state_file = root.join(TURNKEY_DIR).join(state_file_name(isolation_dir));
    let saved = match read_state_file(&state_file) {
        Ok(saved) => saved,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // First run: save the state, nothing to kill
            if verbose {
                let _ = writeln!(w, "tk: recording cell symlink targets (first run)");
            }
            return Ok(write_state_file(&state_file, &current)?);
        }
        Err(e) => return Err(Error::new(format!("reading cell state: {e}"))),
    };

    let changed = diff_targets(&saved, &current);
    if changed.is_empty() {
        if verbose {
            let _ = writeln!(w, "tk: cell symlinks unchanged");
        }
        return Ok(());
    }

    // Something changed: kill the daemon and update the state
    if !quiet {
        let _ = writeln!(w, "tk: cell symlink changed, restarting buck2 daemon");
        if verbose {
            for c in &changed {
                let _ = writeln!(w, "  {c}");
            }
        }
    }

    kill_daemon();

    Ok(write_state_file(&state_file, &current)?)
}

/// The state file's name for `isolation_dir`'s daemon: `.cell-targets`
/// without `--isolation-dir`, else `.cell-targets.<dir>`, the dir without
/// its leading dot (`.cell-targets.turnkey-ci` for `.turnkey-ci`)
fn state_file_name(isolation_dir: Option<&str>) -> String {
    match isolation_dir {
        None => STATE_FILE_NAME.to_string(),
        Some(dir) => format!("{STATE_FILE_NAME}.{}", dir.trim_start_matches('.')),
    }
}

/// The symlinks into the Nix store among `.buckconfig` and the entries of
/// `.turnkey/`: each one's path, relative to the root, and its target
fn read_symlink_targets(root: &Path) -> BTreeMap<String, String> {
    let mut targets = BTreeMap::new();
    let store_target = |path: &Path| {
        std::fs::read_link(path)
            .ok()
            .map(|t| t.to_string_lossy().into_owned())
            .filter(|t| t.starts_with(NIX_STORE_DIR))
    };

    if let Some(t) = store_target(&root.join(".buckconfig")) {
        targets.insert(".buckconfig".to_string(), t);
    }

    let Ok(entries) = std::fs::read_dir(root.join(TURNKEY_DIR)) else {
        return targets;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == STATE_FILE_NAME {
            continue;
        }
        if let Some(t) = store_target(&entry.path()) {
            targets.insert(format!("{TURNKEY_DIR}/{name}"), t);
        }
    }
    targets
}

/// Parses the tab-separated state file
fn read_state_file(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let data = std::fs::read(path)?;
    let mut targets = BTreeMap::new();
    // As bufio.Scanner splits lines: on \n, a trailing \r dropped
    for line in data.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let line = String::from_utf8_lossy(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((path, target)) = line.split_once('\t') {
            targets.insert(path.to_string(), target.to_string());
        }
    }
    Ok(targets)
}

/// Writes the tab-separated state file, sorted
fn write_state_file(path: &Path, targets: &BTreeMap<String, String>) -> io::Result<()> {
    let mut buf = String::from("# Auto-generated by tk. Do not edit.\n");
    for (k, v) in targets {
        buf.push_str(&format!("{k}\t{v}\n"));
    }
    write_file(path, buf.as_bytes())
}

/// The changes between the saved and the current targets, readable and
/// sorted; empty when they are the same
fn diff_targets(
    saved: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut changes = Vec::new();
    for (k, cur) in current {
        match saved.get(k) {
            None => changes.push(format!("+ {k} -> {cur}")),
            Some(old) if old != cur => changes.push(format!("~ {k} -> {cur} (was {old})")),
            Some(_) => {}
        }
    }
    for k in saved.keys() {
        if !current.contains_key(k) {
            changes.push(format!("- {k} (removed)"));
        }
    }
    changes.sort();
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn map(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn reads_the_symlinks_into_the_store() {
        let root = tempfile::tempdir().unwrap();
        let tk = root.path().join(".turnkey");
        std::fs::create_dir(&tk).unwrap();
        symlink("/nix/store/abc123-godeps-cell", tk.join("godeps")).unwrap();
        // Not into the store, and not a symlink: ignored
        symlink("/tmp/something", tk.join("localstuff")).unwrap();
        std::fs::write(tk.join("config.toml"), "hi").unwrap();
        symlink(
            "/nix/store/def456-turnkey.buckconfig",
            root.path().join(".buckconfig"),
        )
        .unwrap();

        assert_eq!(
            read_symlink_targets(root.path()),
            map(&[
                (".buckconfig", "/nix/store/def456-turnkey.buckconfig"),
                (".turnkey/godeps", "/nix/store/abc123-godeps-cell"),
            ])
        );
    }

    #[test]
    fn no_turnkey_dir() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_symlink_targets(root.path()).is_empty());
    }

    #[test]
    fn state_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        let original = map(&[
            (".buckconfig", "/nix/store/abc123-turnkey.buckconfig"),
            (".turnkey/godeps", "/nix/store/def456-godeps-cell"),
            (".turnkey/rustdeps", "/nix/store/ghi789-rustdeps-cell"),
        ]);
        write_state_file(&path, &original).unwrap();
        assert_eq!(read_state_file(&path).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# Auto-generated by tk. Do not edit.\n\
             .buckconfig\t/nix/store/abc123-turnkey.buckconfig\n\
             .turnkey/godeps\t/nix/store/def456-godeps-cell\n\
             .turnkey/rustdeps\t/nix/store/ghi789-rustdeps-cell\n"
        );
    }

    #[test]
    fn state_file_lines_as_a_scanner_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        std::fs::write(&path, "# c\r\na\tx\r\n\nno-tab\nb\ty\tz").unwrap();
        assert_eq!(
            read_state_file(&path).unwrap(),
            map(&[("a", "x"), ("b", "y\tz")])
        );
    }

    #[test]
    fn missing_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_state_file(&dir.path().join("nonexistent")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn diff_targets_cases() {
        let x = "/nix/store/x";
        let y = "/nix/store/y";
        let cases = [
            (
                "identical",
                map(&[("a", x), ("b", y)]),
                map(&[("a", x), ("b", y)]),
                false,
            ),
            (
                "target changed",
                map(&[("a", "/nix/store/old")]),
                map(&[("a", "/nix/store/new")]),
                true,
            ),
            (
                "new entry",
                map(&[("a", x)]),
                map(&[("a", x), ("b", y)]),
                true,
            ),
            (
                "removed entry",
                map(&[("a", x), ("b", y)]),
                map(&[("a", x)]),
                true,
            ),
            ("both empty", map(&[]), map(&[]), false),
        ];
        for (name, saved, current, changed) in cases {
            let diff = diff_targets(&saved, &current);
            assert_eq!(!diff.is_empty(), changed, "{name}: {diff:?}");
        }
        assert_eq!(
            diff_targets(&map(&[("a", x), ("c", x)]), &map(&[("a", y), ("b", y)])),
            [
                "+ b -> /nix/store/y",
                "- c (removed)",
                "~ a -> /nix/store/y (was /nix/store/x)",
            ]
        );
    }

    /// A project with a .turnkey/godeps symlink to `target`, and, if
    /// `saved` is given, a state file holding it as godeps' target
    fn project(target: &str, saved: Option<&str>) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let tk = root.path().join(".turnkey");
        std::fs::create_dir(&tk).unwrap();
        symlink(target, tk.join("godeps")).unwrap();
        if let Some(saved) = saved {
            write_state_file(
                &tk.join(STATE_FILE_NAME),
                &map(&[(".turnkey/godeps", saved)]),
            )
            .unwrap();
        }
        root
    }

    /// Runs check, verbose, and returns its output and how many kills
    fn run_check(root: &Path) -> (String, usize) {
        let mut out = Vec::new();
        let mut kills = 0;
        check(root, None, true, false, &mut out, &mut || kills += 1).unwrap();
        (String::from_utf8(out).unwrap(), kills)
    }

    #[test]
    fn first_run() {
        let root = project("/nix/store/abc123-godeps", None);
        let (out, kills) = run_check(root.path());
        assert!(out.contains("first run"), "{out}");
        assert_eq!(kills, 0);
        assert!(root.path().join(".turnkey").join(STATE_FILE_NAME).exists());
    }

    #[test]
    fn unchanged() {
        let target = "/nix/store/abc123-godeps";
        let root = project(target, Some(target));
        let (out, kills) = run_check(root.path());
        assert!(out.contains("unchanged"), "{out}");
        assert_eq!(kills, 0);
    }

    #[test]
    fn changed() {
        let new = "/nix/store/def456-godeps";
        let root = project(new, Some("/nix/store/abc123-godeps"));
        let (out, kills) = run_check(root.path());
        assert!(out.contains("restarting buck2 daemon"), "{out}");
        assert_eq!(kills, 1);
        let saved = read_state_file(&root.path().join(".turnkey").join(STATE_FILE_NAME)).unwrap();
        assert_eq!(saved, map(&[(".turnkey/godeps", new)]));
    }

    #[test]
    fn quiet_change_still_kills() {
        let root = project("/nix/store/new", Some("/nix/store/old"));
        let mut out = Vec::new();
        let mut kills = 0;
        check(root.path(), None, false, true, &mut out, &mut || kills += 1).unwrap();
        assert!(out.is_empty());
        assert_eq!(kills, 1);
    }

    /// Runs check, quiet, for `isolation_dir`'s daemon, and returns how many
    /// kills
    fn kills_for(root: &Path, isolation_dir: Option<&str>) -> usize {
        let mut kills = 0;
        check(
            root,
            isolation_dir,
            false,
            true,
            &mut Vec::new(),
            &mut || kills += 1,
        )
        .unwrap();
        kills
    }

    #[test]
    fn each_isolation_dir_has_its_own_state() {
        let root = project("/nix/store/abc123-godeps", None);
        let tk = root.path().join(".turnkey");
        // First runs: each daemon's targets recorded, nothing killed
        assert_eq!(kills_for(root.path(), None), 0);
        assert_eq!(kills_for(root.path(), Some(".turnkey-ci")), 0);
        assert!(tk.join(".cell-targets.turnkey-ci").exists());

        std::fs::remove_file(tk.join("godeps")).unwrap();
        symlink("/nix/store/def456-godeps", tk.join("godeps")).unwrap();

        // The default daemon's restart doesn't hide the change from ci's
        assert_eq!(kills_for(root.path(), None), 1);
        assert_eq!(kills_for(root.path(), Some(".turnkey-ci")), 1);
        assert_eq!(kills_for(root.path(), Some(".turnkey-ci")), 0);
        assert_eq!(kills_for(root.path(), None), 0);
    }

    #[test]
    fn no_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let (out, _) = run_check(root.path());
        assert!(out.contains("no Nix store symlinks"), "{out}");
    }
}
