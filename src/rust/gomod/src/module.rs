//! Module paths and versions: a port of the parts of
//! `golang.org/x/mod/module` turnkey uses
//!
//! - [`escape_path`] and [`escape_version`], which case-encode a module
//!   version's proxy URL (the godeps cell fetches from that URL, built the
//!   same way by `nix/lib/deps-cell/fetchers.nix`);
//! - [`split_path_version`], [`check_path_major`] and
//!   [`canonical_version`], which go.mod parsing checks requirements with.

use crate::semver;
use std::fmt;

/// Why a module path or version is rejected, worded as `x/mod` words it
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `module.InvalidPathError`
    InvalidPath {
        kind: &'static str,
        path: String,
        reason: String,
    },
    /// `module.InvalidVersionError`
    InvalidVersion { version: String, reason: String },
    /// An error `x/mod` returns bare
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use gostd::strconv::quote;
        match self {
            Error::InvalidPath { kind, path, reason } => {
                write!(f, "malformed {kind} path {}: {reason}", quote(path))
            }
            Error::InvalidVersion { version, reason } => {
                write!(f, "version {} invalid: {reason}", quote(version))
            }
            Error::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

/// `module.EscapePath`: the module path as the module proxy protocol
/// spells it, each upper-case letter as '!' and its lower case; an error
/// for an invalid module path
pub fn escape_path(path: &str) -> Result<String, Error> {
    check_path(path).map_err(|reason| Error::InvalidPath {
        kind: "module",
        path: path.to_string(),
        reason,
    })?;
    Ok(escape_string(path))
}

/// `module.EscapeVersion`: the version as the module proxy protocol spells
/// it; an error for a version that is not a valid file name or holds a '!'
pub fn escape_version(v: &str) -> Result<String, Error> {
    if check_elem(v, Kind::File).is_err() || v.contains('!') {
        return Err(Error::InvalidVersion {
            version: v.to_string(),
            reason: "disallowed version string".to_string(),
        });
    }
    // check_elem lets a non-ASCII letter through, which x/mod's
    // escapeString then fails on
    if !v.is_ascii() {
        return Err(Error::Other(
            "internal error: inconsistency in EscapePath".to_string(),
        ));
    }
    Ok(escape_string(v))
}

/// Each upper-case ASCII letter as '!' and its lower case; `s` is ASCII
/// without '!'
fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('!');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `module.CanonicalVersion`: [`semver::canonical`], keeping a
/// "+incompatible" build suffix
pub fn canonical_version(v: &str) -> String {
    let mut cv = semver::canonical(v);
    if semver::build(v) == "+incompatible" {
        cv.push_str("+incompatible");
    }
    cv
}

/// `module.SplitPathVersion`: `(prefix, path_major)` with
/// `prefix + path_major == path`, where `path_major` is "" or "/vN" for
/// N >= 2 (".vN", for any N, on gopkg.in); `None` when the last element
/// looks like a major version suffix but is not a valid one
pub fn split_path_version(path: &str) -> Option<(&str, &str)> {
    if path.starts_with("gopkg.in/") {
        return split_gopkg_in(path);
    }
    let b = path.as_bytes();
    let mut i = b.len();
    let mut dot = false;
    while i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b'.') {
        if b[i - 1] == b'.' {
            dot = true;
        }
        i -= 1;
    }
    if i <= 1 || i == b.len() || b[i - 1] != b'v' || b[i - 2] != b'/' {
        return Some((path, ""));
    }
    let (prefix, path_major) = (&path[..i - 2], &path[i - 2..]);
    if dot || path_major.len() <= 2 || path_major.as_bytes()[2] == b'0' || path_major == "/v1" {
        return None;
    }
    Some((prefix, path_major))
}

fn split_gopkg_in(path: &str) -> Option<(&str, &str)> {
    let b = path.as_bytes();
    let mut i = b.len();
    if path.ends_with("-unstable") {
        i -= "-unstable".len();
    }
    while i > 0 && b[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i <= 1 || b[i - 1] != b'v' || b[i - 2] != b'.' {
        // Every gopkg.in path ends in .vN
        return None;
    }
    let (prefix, path_major) = (&path[..i - 2], &path[i - 2..]);
    if path_major.len() <= 2 || (path_major.as_bytes()[2] == b'0' && path_major != ".v0") {
        return None;
    }
    Some((prefix, path_major))
}

/// `module.CheckPathMajor`: whether version `v` fits the major version
/// suffix `path_major` of its module path
pub fn check_path_major(v: &str, path_major: &str) -> Result<(), Error> {
    let mut path_major = path_major;
    if path_major.starts_with(".v") && path_major.ends_with("-unstable") {
        path_major = &path_major[..path_major.len() - "-unstable".len()];
    }
    if v.starts_with("v0.0.0-") && path_major == ".v1" {
        // The old pseudo-versions of gopkg.in .v1 modules
        return Ok(());
    }
    let m = semver::major(v);
    let want = if path_major.is_empty() {
        if m == "v0" || m == "v1" || semver::build(v) == "+incompatible" {
            return Ok(());
        }
        "v0 or v1"
    } else if path_major.starts_with('/') || path_major.starts_with('.') {
        if m == &path_major[1..] {
            return Ok(());
        }
        &path_major[1..]
    } else {
        path_major
    };
    Err(Error::InvalidVersion {
        version: v.to_string(),
        reason: format!("should be {want}, not {m}"),
    })
}

/// Which rules a path element is checked against
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Module,
    File,
}

/// `module.CheckPath`'s checks, the reason a module path is invalid
fn check_path(path: &str) -> Result<(), String> {
    check_path_kind(path, Kind::Module)?;
    let first = path.split('/').next().unwrap_or("");
    if path.starts_with('/') {
        return Err("leading slash".to_string());
    }
    if !first.contains('.') {
        return Err("missing dot in first path element".to_string());
    }
    if path.starts_with('-') {
        return Err("leading dash in first path element".to_string());
    }
    if let Some(c) = first
        .chars()
        .find(|&c| !(c == '-' || c == '.' || c.is_ascii_digit() || c.is_ascii_lowercase()))
    {
        return Err(format!(
            "invalid char {} in first path element",
            gostd::strconv::quote_rune(c)
        ));
    }
    if split_path_version(path).is_none() {
        return Err("invalid version".to_string());
    }
    Ok(())
}

/// `module.checkPath`
fn check_path_kind(path: &str, kind: Kind) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty string".to_string());
    }
    if path.starts_with('-') && kind != Kind::File {
        return Err("leading dash".to_string());
    }
    if path.contains("//") {
        return Err("double slash".to_string());
    }
    if path.ends_with('/') {
        return Err("trailing slash".to_string());
    }
    for elem in path.split('/') {
        check_elem(elem, kind)?;
    }
    Ok(())
}

/// The path elements Windows reserves
const BAD_WINDOWS_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `module.checkElem`
fn check_elem(elem: &str, kind: Kind) -> Result<(), String> {
    if elem.is_empty() {
        return Err("empty path element".to_string());
    }
    if elem.bytes().all(|c| c == b'.') {
        return Err(format!(
            "invalid path element {}",
            gostd::strconv::quote(elem)
        ));
    }
    if elem.starts_with('.') && kind == Kind::Module {
        return Err("leading dot in path element".to_string());
    }
    if elem.ends_with('.') {
        return Err("trailing dot in path element".to_string());
    }
    if let Some(c) = elem.chars().find(|&c| match kind {
        Kind::Module => !mod_path_ok(c),
        Kind::File => !file_name_ok(c),
    }) {
        return Err(format!("invalid char {}", gostd::strconv::quote_rune(c)));
    }
    let short = elem.split('.').next().unwrap_or("");
    // ASCII case folding is Unicode's for these names: none holds a
    // letter with a non-ASCII fold (k, s)
    if BAD_WINDOWS_NAMES
        .iter()
        .any(|bad| bad.eq_ignore_ascii_case(short))
    {
        return Err(format!(
            "{} disallowed as path element component on Windows",
            gostd::strconv::quote(short)
        ));
    }
    if kind == Kind::File {
        return Ok(());
    }
    // Windows short names: a tilde and digits
    if let Some(tilde) = short.rfind('~')
        && tilde < short.len() - 1
        && short[tilde + 1..].bytes().all(|c| c.is_ascii_digit())
    {
        return Err("trailing tilde and digits in path element".to_string());
    }
    Ok(())
}

/// `module.modPathOK`
fn mod_path_ok(c: char) -> bool {
    c == '-' || c == '.' || c == '_' || c == '~' || c.is_ascii_alphanumeric()
}

/// `module.fileNameOK`
fn file_name_ok(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || "!#$%&()+,-.=@[]^_{}~ ".contains(c)
    } else {
        gostd::unicode::is_letter(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_upper_case_in_paths_and_versions() {
        // The proxy URLs godeps-gen's prefetch tests check, and
        // x/mod's escape test table
        assert_eq!(
            escape_path("github.com/foo/bar").unwrap(),
            "github.com/foo/bar"
        );
        assert_eq!(
            escape_path("github.com/BurntSushi/toml").unwrap(),
            "github.com/!burnt!sushi/toml"
        );
        assert_eq!(
            escape_path("github.com/Azure/azure-sdk").unwrap(),
            "github.com/!azure/azure-sdk"
        );
        assert_eq!(
            escape_path("github.com/GoogleCloudPlatform/omega").unwrap(),
            "github.com/!google!cloud!platform/omega"
        );
        assert_eq!(escape_version("v1.0.0-RC1").unwrap(), "v1.0.0-!r!c1");
        assert_eq!(escape_version("v1.4.0").unwrap(), "v1.4.0");
        assert_eq!(
            escape_version("v0.0.0-20231215123456-abcdef123456").unwrap(),
            "v0.0.0-20231215123456-abcdef123456"
        );
    }

    #[test]
    fn rejects_what_x_mod_rejects() {
        for path in [
            "",
            "x",
            "/github.com/a",
            "github.com//a",
            "github.com/a/",
            "-github.com/a",
            "Github.com/a",
            "github.com/.a",
            "github.com/a.",
            "github.com/a b",
            "github.com/con",
            "github.com/a~1",
            "github.com/a/v1",
            "github.com/a/v01",
            "github.com/a/v2.0",
            "gopkg.in/yaml",
            "github.com/日本",
        ] {
            assert!(escape_path(path).is_err(), "escape_path({path:?})");
        }
        for v in ["v1.0.0!", "", ".", "a/b", "v1.0.0*", "CON", "v1.é"] {
            assert!(escape_version(v).is_err(), "escape_version({v:?})");
        }
        assert_eq!(
            escape_version("v1.0.0!").unwrap_err().to_string(),
            r#"version "v1.0.0!" invalid: disallowed version string"#
        );
        assert_eq!(
            escape_path("Github.com/a").unwrap_err().to_string(),
            r#"malformed module path "Github.com/a": invalid char 'G' in first path element"#
        );
    }

    #[test]
    fn splits_major_version_suffixes() {
        let cases = [
            ("github.com/a/b", Some(("github.com/a/b", ""))),
            ("github.com/a/b/v2", Some(("github.com/a/b", "/v2"))),
            ("github.com/a/b/v10", Some(("github.com/a/b", "/v10"))),
            ("github.com/a/b/v1", None),
            ("github.com/a/b/v0", None),
            ("github.com/a/b/v02", None),
            ("github.com/a/b/v2.1", None),
            ("github.com/a/bv2", Some(("github.com/a/bv2", ""))),
            ("gopkg.in/yaml.v3", Some(("gopkg.in/yaml", ".v3"))),
            ("gopkg.in/yaml.v0", Some(("gopkg.in/yaml", ".v0"))),
            (
                "gopkg.in/yaml.v3-unstable",
                Some(("gopkg.in/yaml", ".v3-unstable")),
            ),
            ("gopkg.in/yaml.v03", None),
            ("gopkg.in/yaml", None),
        ];
        for (path, want) in cases {
            assert_eq!(
                split_path_version(path),
                want,
                "split_path_version({path:?})"
            );
        }
    }

    #[test]
    fn checks_versions_against_the_path_major() {
        assert!(check_path_major("v1.2.3", "").is_ok());
        assert!(check_path_major("v0.1.0", "").is_ok());
        assert!(check_path_major("v4.0.0+incompatible", "").is_ok());
        assert!(check_path_major("v2.0.0", "/v2").is_ok());
        assert!(check_path_major("v3.0.1", ".v3").is_ok());
        assert!(check_path_major("v3.0.1", ".v3-unstable").is_ok());
        assert!(check_path_major("v0.0.0-20161208181325-20d25e280405", ".v1").is_ok());
        assert_eq!(
            check_path_major("v2.0.0", "").unwrap_err().to_string(),
            r#"version "v2.0.0" invalid: should be v0 or v1, not v2"#
        );
        assert_eq!(
            check_path_major("v1.0.0", "/v2").unwrap_err().to_string(),
            r#"version "v1.0.0" invalid: should be v2, not v1"#
        );
    }

    #[test]
    fn canonical_versions_keep_incompatible() {
        assert_eq!(
            canonical_version("v4.0.0+incompatible"),
            "v4.0.0+incompatible"
        );
        assert_eq!(canonical_version("v1.2.3+meta"), "v1.2.3");
        assert_eq!(canonical_version("v1.2"), "v1.2.0");
        assert_eq!(canonical_version("1.2.3"), "");
    }
}
