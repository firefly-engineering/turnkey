//! File content hashes, to tell whether a tool changed the files it was
//! watched for
//!
//! tw hashes a wrapper rule's watched files before and after the tool runs:
//! a file created, deleted or rewritten with other content is a change, a
//! file rewritten with the same content isn't.

use std::io;
use std::path::Path;

use gostd::filepath;
use sha2::{Digest, Sha256};

/// The state of a file at a point in time
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    /// The file path, relative to the capture root
    pub path: String,
    /// The SHA-256 of the file's content, in hex; empty when it doesn't
    /// exist
    pub hash: String,
    /// Whether the file existed at capture time
    pub exists: bool,
}

/// Snapshots files, relative to root. A file that doesn't exist is
/// captured as such; only an I/O error on one that does is an error.
pub fn capture(root: &Path, files: &[String]) -> io::Result<Vec<FileSnapshot>> {
    files
        .iter()
        .map(|file| {
            let path = filepath::join(&[root, Path::new(file)]);
            match hash_file(&path) {
                Ok(hash) => Ok(FileSnapshot {
                    path: file.clone(),
                    hash,
                    exists: true,
                }),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(FileSnapshot {
                    path: file.clone(),
                    hash: String::new(),
                    exists: false,
                }),
                Err(e) => Err(e),
            }
        })
        .collect()
}

/// Whether any file changed between before and after: created, deleted,
/// or with other content
pub fn changed(before: &[FileSnapshot], after: &[FileSnapshot]) -> bool {
    if before.len() != after.len() {
        return true;
    }
    let before: std::collections::HashMap<&str, &FileSnapshot> =
        before.iter().map(|s| (s.path.as_str(), s)).collect();
    after.iter().any(|a| match before.get(a.path.as_str()) {
        None => true,
        Some(b) => a.exists != b.exists || a.hash != b.hash,
    })
}

/// The SHA-256 of a file's content, in hex
fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn capture_hashes_what_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module m\n").unwrap();

        let snaps = capture(dir.path(), &files(&["go.mod", "go.sum"])).unwrap();
        assert_eq!(
            snaps,
            [
                FileSnapshot {
                    path: "go.mod".into(),
                    // sha256 of "module m\n"
                    hash: "7b79504c1c3ab1bf5e14d63a44f209cd217d9e1a47ba4a0e8dcb432c9647ea30".into(),
                    exists: true,
                },
                FileSnapshot {
                    path: "go.sum".into(),
                    hash: String::new(),
                    exists: false,
                },
            ]
        );
    }

    #[test]
    fn a_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("go.mod")).unwrap();
        assert!(capture(dir.path(), &files(&["go.mod"])).is_err());
    }

    #[test]
    fn changed_sees_content_creation_and_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let watched = files(&["a", "b"]);
        std::fs::write(dir.path().join("a"), "1").unwrap();
        let before = capture(dir.path(), &watched).unwrap();

        // Rewritten with the same content
        std::fs::write(dir.path().join("a"), "1").unwrap();
        assert!(!changed(&before, &capture(dir.path(), &watched).unwrap()));

        std::fs::write(dir.path().join("a"), "2").unwrap();
        assert!(changed(&before, &capture(dir.path(), &watched).unwrap()));

        std::fs::write(dir.path().join("a"), "1").unwrap();
        std::fs::write(dir.path().join("b"), "").unwrap();
        assert!(
            changed(&before, &capture(dir.path(), &watched).unwrap()),
            "created"
        );

        std::fs::remove_file(dir.path().join("b")).unwrap();
        std::fs::remove_file(dir.path().join("a")).unwrap();
        assert!(
            changed(&before, &capture(dir.path(), &watched).unwrap()),
            "deleted"
        );

        assert!(changed(&before, &before[..1]), "fewer files");
    }
}
