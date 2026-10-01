//! Whether a generated file is stale relative to its sources
//!
//! A target is stale when it is missing or older than any of its sources,
//! by modification time. Source paths may be globs (`*.go`, `**/*.go`); a
//! pattern that matches nothing stands for itself, a missing file.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;

use gostd::filepath;

/// A detailed staleness check
#[derive(Debug, Clone, Default)]
pub struct Check {
    /// Whether the target is missing or older than any source
    pub stale: bool,

    /// The path to the target file
    pub target_path: PathBuf,

    /// The modification time of the target; None when it is missing
    pub target_mod_time: Option<SystemTime>,

    /// Whether the target file does not exist
    pub target_missing: bool,

    /// Each source file, globs expanded
    pub sources: Vec<SourceInfo>,

    /// The source modified last, of those that exist; None when none does
    pub newest_source: Option<SourceInfo>,
}

/// A single source file
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceInfo {
    /// The file path
    pub path: PathBuf,

    /// The modification time; None when the file is missing
    pub mod_time: Option<SystemTime>,

    /// Whether the file does not exist
    pub missing: bool,
}

/// Whether target is stale relative to any of sources: missing, or older
/// than one of them
pub fn is_stale<P: AsRef<Path>>(sources: &[P], target: &Path) -> Result<bool> {
    Ok(check(sources, target)?.stale)
}

/// Checks target against sources, each a path or a glob
pub fn check<P: AsRef<Path>>(sources: &[P], target: &Path) -> Result<Check> {
    let mut result = Check {
        target_path: target.to_path_buf(),
        ..Check::default()
    };

    match std::fs::metadata(target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            result.target_missing = true;
            result.stale = true;
        }
        Err(e) => return Err(anyhow::Error::new(e).context(format!("stat {}", target.display()))),
        Ok(m) => result.target_mod_time = Some(m.modified()?),
    }

    for pattern in sources {
        for path in expand_glob(pattern.as_ref())? {
            let mut info = SourceInfo {
                path,
                ..SourceInfo::default()
            };
            match std::fs::metadata(&info.path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => info.missing = true,
                Err(e) => {
                    return Err(
                        anyhow::Error::new(e).context(format!("stat {}", info.path.display()))
                    );
                }
                Ok(m) => {
                    let mod_time = m.modified()?;
                    info.mod_time = Some(mod_time);
                    if result
                        .newest_source
                        .as_ref()
                        .is_none_or(|newest| Some(mod_time) > newest.mod_time)
                    {
                        result.newest_source = Some(info.clone());
                    }
                    if !result.target_missing && Some(mod_time) > result.target_mod_time {
                        result.stale = true;
                    }
                }
            }
            result.sources.push(info);
        }
    }

    Ok(result)
}

/// The paths a pattern stands for: itself without glob characters, else
/// its matches, or itself when there are none (a missing file)
fn expand_glob(pattern: &Path) -> Result<Vec<PathBuf>> {
    let bytes = pattern.as_os_str().as_bytes();
    if !bytes.iter().any(|c| matches!(c, b'*' | b'?' | b'[')) {
        return Ok(vec![pattern.to_path_buf()]);
    }
    if bytes.windows(2).any(|w| w == b"**") {
        return Ok(expand_double_star(pattern));
    }
    let matches = filepath::glob(pattern)?;
    if matches.is_empty() {
        return Ok(vec![pattern.to_path_buf()]);
    }
    Ok(matches)
}

/// The files under the directory before the first `**` whose name matches
/// what follows it, past its separator (only the last element counts), or
/// the pattern itself when there are none
fn expand_double_star(pattern: &Path) -> Vec<PathBuf> {
    let bytes = pattern.as_os_str().as_bytes();
    let i = bytes
        .windows(2)
        .position(|w| w == b"**")
        .expect("a ** pattern");
    let base = if i == 0 {
        PathBuf::from(".")
    } else {
        filepath::clean(Path::new(OsStr::from_bytes(&bytes[..i])))
    };
    let suffix = match &bytes[i + 2..] {
        [b'/', rest @ ..] => rest,
        rest => rest,
    };
    let suffix_path = OsStr::from_bytes(suffix);

    let mut matches = Vec::new();
    walk(&base, &mut |path, is_dir| {
        if is_dir {
            return;
        }
        if !suffix_path.is_empty() {
            let name = filepath::base(path);
            if !matches!(
                filepath::match_pattern(suffix_path, name.as_os_str()),
                Ok(true)
            ) {
                return;
            }
        }
        matches.push(path.to_path_buf());
    });

    if matches.is_empty() {
        return vec![pattern.to_path_buf()];
    }
    matches
}

/// `filepath.Walk` with a function that skips every error: visits root and
/// the entries under it in lexical order, without following symbolic links
fn walk(root: &Path, visit: &mut dyn FnMut(&Path, bool)) {
    let Ok(info) = std::fs::symlink_metadata(root) else {
        return;
    };
    walk_entry(root, info.is_dir(), visit);
}

fn walk_entry(path: &Path, is_dir: bool, visit: &mut dyn FnMut(&Path, bool)) {
    if !is_dir {
        visit(path, false);
        return;
    }
    let names = std::fs::read_dir(path).map(|entries| {
        let mut names: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        names.sort();
        names
    });
    visit(path, true);
    let Ok(names) = names else {
        return;
    };
    for name in names {
        let file = filepath::join(&[path, Path::new(&name)]);
        if let Ok(info) = std::fs::symlink_metadata(&file) {
            walk_entry(&file, info.is_dir(), visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A fixed reference time. Tests order file modification times
    /// explicitly relative to it instead of sleeping, so the outcome never
    /// depends on filesystem timestamp resolution or scheduling.
    fn base() -> SystemTime {
        // 2026-01-01T00:00:00Z
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600)
    }

    /// Writes content to path and sets its modification time to mtime
    fn write_at(path: &Path, content: &str, mtime: SystemTime) {
        std::fs::write(path, content).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    #[test]
    fn is_stale_target_missing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, "content").unwrap();
        let target = dir.path().join("target.txt"); // Does not exist

        assert!(
            is_stale(&[&source], &target).unwrap(),
            "stale when the target is missing"
        );
    }

    #[test]
    fn is_stale_target_newer() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        write_at(&source, "content", base());
        // Target is newer than the source
        let target = dir.path().join("target.txt");
        write_at(&target, "generated", base() + Duration::from_secs(1));

        assert!(
            !is_stale(&[&source], &target).unwrap(),
            "fresh when the target is newer"
        );
    }

    #[test]
    fn is_stale_source_newer() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        write_at(&target, "generated", base());
        // Source is newer than the target
        let source = dir.path().join("source.txt");
        write_at(&source, "content", base() + Duration::from_secs(1));

        assert!(
            is_stale(&[&source], &target).unwrap(),
            "stale when the source is newer"
        );
    }

    #[test]
    fn is_stale_multiple_sources() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        write_at(&target, "generated", base());
        // Both sources are newer than the target
        let source1 = dir.path().join("source1.txt");
        write_at(&source1, "content1", base() + Duration::from_secs(1));
        let source2 = dir.path().join("source2.txt");
        write_at(&source2, "content2", base() + Duration::from_secs(1));

        assert!(is_stale(&[&source1, &source2], &target).unwrap());
    }

    #[test]
    fn is_stale_glob_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        write_at(&target, "generated", base());
        // Go files newer than the target
        for name in ["a.go", "b.go", "c.go"] {
            write_at(
                &dir.path().join(name),
                "package main",
                base() + Duration::from_secs(1),
            );
        }

        let pattern = dir.path().join("*.go");
        assert!(
            is_stale(&[&pattern], &target).unwrap(),
            "stale when glob-matched sources are newer"
        );
    }

    #[test]
    fn check_detailed_result() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        write_at(&target, "generated", base());
        // Two sources newer than the target; source2 is the newest
        let source1 = dir.path().join("source1.txt");
        write_at(&source1, "content1", base() + Duration::from_secs(1));
        let source2 = dir.path().join("source2.txt");
        write_at(&source2, "content2", base() + Duration::from_secs(2));

        let result = check(&[&source1, &source2], &target).unwrap();
        assert!(result.stale);
        assert!(!result.target_missing);
        assert_eq!(result.sources.len(), 2);
        assert_eq!(result.newest_source.map(|s| s.path), Some(source2));
    }

    #[test]
    fn expand_double_star_matches_at_every_level() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("sub");
        std::fs::create_dir_all(&subdir).unwrap();
        // Files at different levels
        for f in [dir.path().join("root.go"), subdir.join("nested.go")] {
            std::fs::write(f, "package main").unwrap();
        }
        // A non-Go file that shouldn't match
        std::fs::write(dir.path().join("readme.md"), "# README").unwrap();

        let matches = expand_double_star(&dir.path().join("**/*.go"));
        assert_eq!(matches.len(), 2, "{matches:?}");
    }

    #[test]
    fn is_stale_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "generated").unwrap();
        // Non-existent source
        let source = dir.path().join("nonexistent.txt");

        let result = check(&[&source], &target).unwrap();
        assert_eq!(result.sources.len(), 1);
        assert!(result.sources[0].missing, "the source is marked missing");
    }

    #[test]
    fn a_pattern_matching_nothing_stands_for_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "generated").unwrap();

        for pattern in ["*.go", "**/*.go"] {
            let result = check(&[dir.path().join(pattern)], &target).unwrap();
            assert_eq!(result.sources.len(), 1, "{pattern}");
            assert!(result.sources[0].missing, "{pattern}");
            assert!(result.newest_source.is_none(), "{pattern}");
        }
        assert!(
            check(&[dir.path().join("[")], &target).is_err(),
            "a bad pattern"
        );
    }
}
