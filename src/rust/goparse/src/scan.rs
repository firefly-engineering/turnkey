//! The Go files of a directory, and of a tree
//!
//! Paths are strings joined as Go's `filepath.Join` joins them (cleaned),
//! so that the checks on them (a directory under `testdata/`) see what the
//! Go version saw.

use crate::{GoFile, GoPackage, parse_file};
use gostd::path::{base, join};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// A directory entry met by [`walk_dir`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Its name (the root's is `path.Base` of its path)
    pub name: String,
    /// Whether it is a directory (not a symbolic link to one)
    pub is_dir: bool,
}

/// What [`walk_dir`] does after visiting an entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visit {
    /// Go on, into the entry if it is a directory
    Continue,
    /// Skip the directory visited (`filepath.SkipDir`)
    SkipDir,
}

/// `filepath.WalkDir`: visits `root` and, if it is a directory, the tree
/// under it, depth first, each directory's entries in name order, without
/// following symbolic links. A directory is visited before it is read, so
/// what `visit` writes there is seen. An error reading the tree ends the
/// walk with it, as it ends the Go callers' walks.
pub fn walk_dir<E: From<io::Error>>(
    root: &str,
    visit: &mut impl FnMut(&str, &DirEntry) -> Result<Visit, E>,
) -> Result<(), E> {
    let meta = fs::symlink_metadata(root)?;
    let entry = DirEntry {
        name: base(root),
        is_dir: meta.is_dir(),
    };
    walk(root, &entry, visit).map(|_| ())
}

/// Visits `path` and the tree under it; `SkipDir` when a file's visit asks
/// to skip the rest of its directory, as Go's does
fn walk<E: From<io::Error>>(
    path: &str,
    entry: &DirEntry,
    visit: &mut impl FnMut(&str, &DirEntry) -> Result<Visit, E>,
) -> Result<Visit, E> {
    let next = visit(path, entry)?;
    if !entry.is_dir {
        return Ok(next);
    }
    if next == Visit::SkipDir {
        return Ok(Visit::Continue);
    }
    for child in read_dir(path)? {
        let child_path = join(&[path, &child.name]);
        if walk(&child_path, &child, visit)? == Visit::SkipDir {
            break;
        }
    }
    Ok(Visit::Continue)
}

/// `os.ReadDir`: a directory's entries, sorted by name
fn read_dir(dir: &str) -> io::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        entries.push((
            name.as_bytes().to_vec(),
            DirEntry {
                name: name.to_string_lossy().into_owned(),
                is_dir: entry.file_type()?.is_dir(),
            },
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries.into_iter().map(|(_, e)| e).collect())
}

/// The Go files of a directory, test files included, in name order, unless
/// the directory is under a `testdata` directory. A file that can't be
/// read or parsed is left out.
pub fn scan_dir(dir: &str) -> io::Result<Vec<GoFile>> {
    let mut files = Vec::new();
    for entry in read_dir(dir)? {
        if entry.is_dir || !entry.name.ends_with(".go") {
            continue;
        }
        if dir.contains("/testdata/") || dir.ends_with("/testdata") {
            continue;
        }
        if let Ok(f) = parse_file(Path::new(&join(&[dir, &entry.name]))) {
            files.push(f);
        }
    }
    Ok(files)
}

/// The package of a directory: its non-test Go files, whatever their
/// constraints (which files a build includes is decided per build,
/// [`GoPackage::imports`]). `None` if it has none.
pub fn scan_package(dir: &str, import_path: &str) -> io::Result<Option<GoPackage>> {
    let mut pkg = GoPackage {
        dir: dir.to_string(),
        import_path: import_path.to_string(),
        ..GoPackage::default()
    };
    let mut embeds = BTreeSet::new();
    for f in scan_dir(dir)? {
        // Test files don't contribute to library deps
        if f.is_test {
            continue;
        }
        if pkg.name.is_empty() {
            pkg.name = f.package.clone();
        }
        embeds.extend(f.embed_patterns.iter().cloned());
        pkg.has_cgo |= f.has_cgo;
        pkg.files.push(f);
    }
    if pkg.files.is_empty() {
        return Ok(None);
    }
    pkg.embed_patterns = embeds.into_iter().collect();
    Ok(Some(pkg))
}

/// The tags the build constraints of the Go files under `dir` name
/// ([`GoFile::constraint_tags`]), sorted: those that can change which
/// files a build of a package there includes. It covers the files
/// `go list ./...` does, test files included: testdata, vendor and hidden
/// or `_`-prefixed directories are skipped. A file that doesn't parse
/// names none.
pub fn tree_constraint_tags(dir: &str) -> io::Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    walk_dir(dir, &mut |path: &str,
                        entry: &DirEntry|
     -> io::Result<Visit> {
        let name = entry.name.as_str();
        if entry.is_dir {
            if path != dir
                && (name == "testdata"
                    || name == "vendor"
                    || name.starts_with('.')
                    || name.starts_with('_'))
            {
                return Ok(Visit::SkipDir);
            }
            return Ok(Visit::Continue);
        }
        if !name.ends_with(".go") {
            return Ok(Visit::Continue);
        }
        if let Ok(f) = parse_file(Path::new(path)) {
            seen.extend(f.constraint_tags());
        }
        Ok(Visit::Continue)
    })?;
    Ok(seen.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_files(dir: &Path, files: &[(&str, &str)]) {
        for (name, content) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    /// A tree's constraint tags are its Go files' (test files and
    /// subpackages included, go:build or +build), but not those of the
    /// directories go list's ./... skips
    #[test]
    fn tree_constraint_tags_skip_what_go_list_skips() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                ("a.go", "//go:build linux && !integration\n\npackage a\n"),
                ("a_test.go", "//go:build e2e\n\npackage a\n"),
                ("old.go", "// +build legacy\n\npackage a\n"),
                ("plain.go", "package a\n\n// +build ignored_after_package\n"),
                (
                    "sub/b.go",
                    "//go:build (cgo || netgo) && !arm64\n\npackage b\n",
                ),
                (
                    "testdata/c.go",
                    "//go:build skipped_testdata\n\npackage c\n",
                ),
                ("vendor/v/v.go", "//go:build skipped_vendor\n\npackage v\n"),
                (".hidden/h.go", "//go:build skipped_hidden\n\npackage h\n"),
                (
                    "_underscore/u.go",
                    "//go:build skipped_underscore\n\npackage u\n",
                ),
                ("notgo.txt", "//go:build skipped_notgo\n"),
                ("broken.go", "//go:build skipped_broken\n\npackage\n"),
            ],
        );
        let got = tree_constraint_tags(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(
            got,
            [
                "arm64",
                "cgo",
                "e2e",
                "integration",
                "legacy",
                "linux",
                "netgo"
            ]
        );
        assert!(tree_constraint_tags(tmp.path().join("missing").to_str().unwrap()).is_err());
    }

    #[test]
    fn scan_package_keeps_non_test_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                (
                    "b.go",
                    "package p\n\nimport \"C\"\n\n//go:embed z.txt a.txt\nvar s string\n",
                ),
                ("a.go", "package p\n\n//go:embed a.txt\nvar t string\n"),
                ("a_test.go", "package p_test\n\nimport \"testing\"\n"),
                ("broken.go", "package\n"),
                ("dir.go/x.go", "package x\n"),
            ],
        );
        let dir = tmp.path().to_str().unwrap();
        let pkg = scan_package(dir, "example.com/p").unwrap().unwrap();
        let names: Vec<_> = pkg
            .files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["a.go", "b.go"]);
        assert_eq!(pkg.name, "p");
        assert!(pkg.has_cgo);
        assert_eq!(pkg.embed_patterns, ["a.txt", "z.txt"]);

        assert_eq!(
            scan_package(&format!("{dir}/dir.go"), "x")
                .unwrap()
                .unwrap()
                .files
                .len(),
            1
        );
        fs::create_dir(tmp.path().join("empty")).unwrap();
        assert_eq!(scan_package(&format!("{dir}/empty"), "x").unwrap(), None);
    }

    /// Nothing under a testdata directory is a Go file, whatever is asked
    #[test]
    fn scan_dir_under_testdata() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                ("testdata/a.go", "package a\n"),
                ("testdata/x/b.go", "package b\n"),
            ],
        );
        let dir = tmp.path().to_str().unwrap();
        assert!(scan_dir(&format!("{dir}/testdata")).unwrap().is_empty());
        assert!(scan_dir(&format!("{dir}/testdata/x")).unwrap().is_empty());
    }

    #[test]
    fn walk_dir_visits_depth_first_in_name_order() {
        let tmp = tempfile::tempdir().unwrap();
        write_files(
            tmp.path(),
            &[
                ("b/x", ""),
                ("a", ""),
                ("Z", ""),
                ("c/skip/y", ""),
                ("c/z", ""),
            ],
        );
        let root = format!("{}/", tmp.path().to_str().unwrap());
        let mut seen = Vec::new();
        walk_dir(&root, &mut |path: &str,
                              e: &DirEntry|
         -> io::Result<Visit> {
            seen.push((
                path.strip_prefix(&root).unwrap_or(path).to_string(),
                e.is_dir,
            ));
            Ok(if e.name == "skip" {
                Visit::SkipDir
            } else {
                Visit::Continue
            })
        })
        .unwrap();
        assert_eq!(seen[0], (String::new(), true));
        let rest: Vec<_> = seen[1..].iter().map(|(p, d)| (p.as_str(), *d)).collect();
        assert_eq!(
            rest,
            [
                ("Z", false),
                ("a", false),
                ("b", true),
                ("b/x", false),
                ("c", true),
                ("c/skip", true),
                ("c/z", false)
            ]
        );
    }
}
