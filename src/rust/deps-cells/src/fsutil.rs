//! File system writes as the Go version made them: with its permissions,
//! whatever the umask lets through, rather than std's defaults

use std::fs::{DirBuilder, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// `os.MkdirAll(path, 0o755)`
pub(crate) fn mkdir_all(path: impl AsRef<Path>) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o755).create(path)
}

/// `os.WriteFile(path, data, 0o644)`: created 0644 (less the umask),
/// truncated if it exists
pub fn write_file(path: impl AsRef<Path>, data: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(path)?
        .write_all(data)
}

/// Writes `content` atomically (a temporary file beside it, then a rename),
/// and only when the file doesn't already hold it: an unchanged file keeps
/// its mtime, so buck2 sees nothing. Whether it wrote.
pub(crate) fn write_if_changed(path: &Path, content: &[u8]) -> io::Result<bool> {
    if std::fs::read(path).is_ok_and(|current| current == content) {
        return Ok(false);
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".tmp-{base}-"))
        .rand_bytes(9)
        .tempfile_in(dir)?;
    tmp.write_all(content)?;
    tmp.as_file()
        .set_permissions(Permissions::from_mode(0o644))?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(true)
}
