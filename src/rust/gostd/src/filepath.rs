//! Go's `path/filepath` on Unix paths
//!
//! [`crate::path`] is Go's `path`, on strings; this is `path/filepath` on
//! a `Path`, whose bytes needn't be UTF-8. The Go tools join, clean and
//! glob the paths their children run in (a child's `PWD`) and the files
//! they stat with it, and a port gets the same paths only with the same
//! functions: `Path::join` replaces on an absolute path where
//! `filepath.Join` concatenates, and cleans nothing; the `glob` crate
//! differs from `filepath.Match` at the edges.
//!
//! Unlike [`crate::path`], [`glob`] reads the file system: the directories
//! it lists. turnkey runs on Linux and macOS only, so the separator is `/`
//! and there are no volumes.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use crate::path::{base_bytes, clean_bytes};

const SEPARATOR: u8 = b'/';

fn bytes(p: &Path) -> &[u8] {
    p.as_os_str().as_bytes()
}

fn path_buf(b: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(b))
}

/// `filepath.Clean`: the shortest path naming the same file, by lexical
/// processing only
pub fn clean(path: &Path) -> PathBuf {
    path_buf(clean_bytes(bytes(path)))
}

/// `filepath.Join`: the elements joined with separators, then cleaned;
/// empty elements are ignored, and so is nothing else (an absolute element
/// after the first is appended, not substituted)
pub fn join<P: AsRef<Path>>(elems: &[P]) -> PathBuf {
    let elems: Vec<&[u8]> = elems.iter().map(|e| bytes(e.as_ref())).collect();
    match elems.iter().position(|e| !e.is_empty()) {
        Some(first) => path_buf(clean_bytes(&elems[first..].join(&SEPARATOR))),
        None => PathBuf::new(),
    }
}

/// `filepath.Split`: the path up to and including its last separator, and
/// the rest
pub fn split(path: &Path) -> (PathBuf, PathBuf) {
    let b = bytes(path);
    let i = b.iter().rposition(|&c| c == SEPARATOR).map_or(0, |i| i + 1);
    (path_buf(b[..i].to_vec()), path_buf(b[i..].to_vec()))
}

/// `filepath.Dir`: all but the last element, cleaned
pub fn dir(path: &Path) -> PathBuf {
    let b = bytes(path);
    let i = b.iter().rposition(|&c| c == SEPARATOR).map_or(0, |i| i + 1);
    path_buf(clean_bytes(&b[..i]))
}

/// `filepath.Base`: the last element, trailing separators removed
pub fn base(path: &Path) -> PathBuf {
    path_buf(base_bytes(bytes(path)).to_vec())
}

/// `filepath.IsAbs`
pub fn is_abs(path: &Path) -> bool {
    bytes(path).first() == Some(&SEPARATOR)
}

/// `filepath.ErrBadPattern`: a malformed pattern
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadPattern;

impl fmt::Display for BadPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("syntax error in pattern")
    }
}

impl std::error::Error for BadPattern {}

/// `filepath.Match`: whether `name` matches the shell pattern `pattern`
///
/// `*` matches any run of non-separator bytes, `?` one non-separator
/// character, `[...]` (or `[^...]`) a character class, and `\` escapes.
/// The only error is [`BadPattern`].
pub fn match_pattern(pattern: &OsStr, name: &OsStr) -> Result<bool, BadPattern> {
    match_bytes(pattern.as_bytes(), name.as_bytes())
}

fn match_bytes(mut pattern: &[u8], mut name: &[u8]) -> Result<bool, BadPattern> {
    'pattern: while !pattern.is_empty() {
        let (star, chunk, rest) = scan_chunk(pattern);
        pattern = rest;
        if star && chunk.is_empty() {
            // Trailing * matches rest of string unless it has a /.
            return Ok(!name.contains(&SEPARATOR));
        }
        // Look for match at current position.
        let (t, ok, err) = match_chunk(chunk, name);
        // if we're the last chunk, make sure we've exhausted the name
        // otherwise we'll give a false result even if we could still match
        // using the star
        if ok && (t.is_empty() || !pattern.is_empty()) {
            name = t;
            continue;
        }
        if let Some(err) = err {
            return Err(err);
        }
        if star {
            // Look for match skipping i+1 bytes.
            // Cannot skip /.
            let mut i = 0;
            while i < name.len() && name[i] != SEPARATOR {
                let (t, ok, err) = match_chunk(chunk, &name[i + 1..]);
                if ok {
                    // if we're the last chunk, make sure we exhausted the name
                    if pattern.is_empty() && !t.is_empty() {
                        i += 1;
                        continue;
                    }
                    name = t;
                    continue 'pattern;
                }
                if let Some(err) = err {
                    return Err(err);
                }
                i += 1;
            }
        }
        // Before returning false with no error,
        // check that the remainder of the pattern is syntactically valid.
        while !pattern.is_empty() {
            let (_, chunk, rest) = scan_chunk(pattern);
            pattern = rest;
            if let (_, _, Some(err)) = match_chunk(chunk, b"") {
                return Err(err);
            }
        }
        return Ok(false);
    }
    Ok(name.is_empty())
}

/// The next segment of pattern: a non-star chunk, and whether stars come
/// before it
fn scan_chunk(mut pattern: &[u8]) -> (bool, &[u8], &[u8]) {
    let mut star = false;
    while !pattern.is_empty() && pattern[0] == b'*' {
        pattern = &pattern[1..];
        star = true;
    }
    let mut inrange = false;
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            b'\\' => {
                // error check handled in match_chunk: bad pattern.
                if i + 1 < pattern.len() {
                    i += 1;
                }
            }
            b'[' => inrange = true,
            b']' => inrange = false,
            b'*' if !inrange => break,
            _ => {}
        }
        i += 1;
    }
    (star, &pattern[..i], &pattern[i..])
}

/// Whether chunk matches the beginning of s: the rest of s and true when
/// it does. Past a mismatch, the chunk is still checked for a bad pattern.
fn match_chunk<'s>(mut chunk: &[u8], mut s: &'s [u8]) -> (&'s [u8], bool, Option<BadPattern>) {
    // failed records whether the match has failed.
    // After the match fails, the loop continues on processing chunk,
    // checking that the pattern is well-formed but no longer reading s.
    let mut failed = false;
    while !chunk.is_empty() {
        if !failed && s.is_empty() {
            failed = true;
        }
        match chunk[0] {
            b'[' => {
                // character class
                let mut r: u32 = 0;
                if !failed {
                    let (c, n) = decode_rune(s);
                    r = c;
                    s = &s[n..];
                }
                chunk = &chunk[1..];
                // possibly negated
                let mut negated = false;
                if !chunk.is_empty() && chunk[0] == b'^' {
                    negated = true;
                    chunk = &chunk[1..];
                }
                // parse all ranges
                let mut matched = false;
                let mut nrange = 0;
                loop {
                    if !chunk.is_empty() && chunk[0] == b']' && nrange > 0 {
                        chunk = &chunk[1..];
                        break;
                    }
                    let lo;
                    match get_esc(chunk) {
                        Ok((c, rest)) => {
                            lo = c;
                            chunk = rest;
                        }
                        Err(e) => return (b"", false, Some(e)),
                    }
                    let mut hi = lo;
                    if chunk[0] == b'-' {
                        match get_esc(&chunk[1..]) {
                            Ok((c, rest)) => {
                                hi = c;
                                chunk = rest;
                            }
                            Err(e) => return (b"", false, Some(e)),
                        }
                    }
                    if lo <= r && r <= hi {
                        matched = true;
                    }
                    nrange += 1;
                }
                if matched == negated {
                    failed = true;
                }
            }
            b'?' => {
                if !failed {
                    if s[0] == SEPARATOR {
                        failed = true;
                    }
                    let (_, n) = decode_rune(s);
                    s = &s[n..];
                }
                chunk = &chunk[1..];
            }
            c => {
                let mut c = c;
                if c == b'\\' {
                    chunk = &chunk[1..];
                    if chunk.is_empty() {
                        return (b"", false, Some(BadPattern));
                    }
                    c = chunk[0];
                }
                if !failed {
                    if c != s[0] {
                        failed = true;
                    }
                    s = &s[1..];
                }
                chunk = &chunk[1..];
            }
        }
    }
    if failed {
        return (b"", false, None);
    }
    (s, true, None)
}

/// A possibly escaped character of a character class
fn get_esc(mut chunk: &[u8]) -> Result<(u32, &[u8]), BadPattern> {
    if chunk.is_empty() || chunk[0] == b'-' || chunk[0] == b']' {
        return Err(BadPattern);
    }
    if chunk[0] == b'\\' {
        chunk = &chunk[1..];
        if chunk.is_empty() {
            return Err(BadPattern);
        }
    }
    let (r, n) = decode_rune(chunk);
    if r == REPLACEMENT && n == 1 {
        return Err(BadPattern);
    }
    let rest = &chunk[n..];
    if rest.is_empty() {
        return Err(BadPattern);
    }
    Ok((r, rest))
}

const REPLACEMENT: u32 = 0xFFFD;

/// `utf8.DecodeRune`: the first character of s and its length in bytes;
/// U+FFFD and 1 for an invalid encoding, U+FFFD and 0 for an empty s
fn decode_rune(s: &[u8]) -> (u32, usize) {
    let Some(&first) = s.first() else {
        return (REPLACEMENT, 0);
    };
    let len = match first {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return (REPLACEMENT, 1),
    };
    match s.get(..len).and_then(|b| std::str::from_utf8(b).ok()) {
        Some(text) => (text.chars().next().map_or(REPLACEMENT, u32::from), len),
        None => (REPLACEMENT, 1),
    }
}

/// Whether path holds any of the characters `filepath.Match` treats
/// specially
fn has_meta(path: &[u8]) -> bool {
    path.iter().any(|c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
}

/// `filepath.Glob`: the names of all files matching pattern, or none;
/// I/O errors are ignored, and the only error is [`BadPattern`]
pub fn glob(pattern: &Path) -> Result<Vec<PathBuf>, BadPattern> {
    glob_with_limit(pattern, 0)
}

fn glob_with_limit(pattern: &Path, depth: usize) -> Result<Vec<PathBuf>, BadPattern> {
    // This limit is used to prevent stack exhaustion issues.
    const PATH_SEPARATORS_LIMIT: usize = 10000;
    if depth == PATH_SEPARATORS_LIMIT {
        return Err(BadPattern);
    }

    // Check pattern is well-formed.
    match_bytes(bytes(pattern), b"")?;
    if !has_meta(bytes(pattern)) {
        if std::fs::symlink_metadata(pattern).is_err() {
            return Ok(Vec::new());
        }
        return Ok(vec![pattern.to_path_buf()]);
    }

    let (dir, file) = split(pattern);
    let dir = clean_glob_path(&dir);

    if !has_meta(bytes(&dir)) {
        return Ok(glob_dir(&dir, &file, Vec::new()));
    }

    // Prevent infinite recursion.
    if dir == pattern {
        return Err(BadPattern);
    }

    let mut matches = Vec::new();
    for d in glob_with_limit(&dir, depth + 1)? {
        matches = glob_dir(&d, &file, matches);
    }
    Ok(matches)
}

/// The directory part of a glob, prepared for globbing
fn clean_glob_path(path: &Path) -> PathBuf {
    let b = bytes(path);
    match b {
        b"" => PathBuf::from("."),
        b"/" => path.to_path_buf(),
        // chop off trailing separator
        _ => path_buf(b[..b.len() - 1].to_vec()),
    }
}

/// Appends to matches the entries of dir matching pattern, in name order;
/// a dir that can't be read adds none. pattern is already checked.
fn glob_dir(dir: &Path, pattern: &Path, mut matches: Vec<PathBuf>) -> Vec<PathBuf> {
    match std::fs::metadata(dir) {
        Ok(m) if m.is_dir() => {}
        _ => return matches,
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return matches;
    };
    let mut names: Vec<OsString> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    names.sort();
    for name in names {
        if let Ok(true) = match_pattern(pattern.as_os_str(), &name) {
            matches.push(join(&[dir, Path::new(&name)]));
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(p: PathBuf) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn clean_keeps_non_utf8_bytes() {
        let path = Path::new(OsStr::from_bytes(b"/a/./b\xff/../c\xfe//"));
        assert_eq!(clean(path).as_os_str().as_bytes(), b"/a/c\xfe");
    }

    #[test]
    fn join_concatenates_absolute_elements() {
        assert_eq!(s(join(&["/root", "/abs"])), "/root/abs");
        assert_eq!(s(join(&["/root", "./go.mod"])), "/root/go.mod");
        assert_eq!(s(join(&["", "b"])), "b");
        assert_eq!(s(join(&["a", ""])), "a");
        assert_eq!(s(join::<&str>(&["", ""])), "");
    }

    #[test]
    fn dir_and_base_match_go() {
        for (path, want) in [
            ("", "."),
            (".", "."),
            ("/.", "/"),
            ("/", "/"),
            ("////", "/"),
            ("/foo", "/"),
            ("x/", "x"),
            ("abc", "."),
            ("abc/def", "abc"),
            ("a/b/.x", "a/b"),
            ("a/b/c.", "a/b"),
            ("a/b/c.x", "a/b"),
        ] {
            assert_eq!(s(dir(Path::new(path))), want, "Dir({path:?})");
        }
        for (path, want) in [
            ("", "."),
            (".", "."),
            ("/.", "."),
            ("/", "/"),
            ("////", "/"),
            ("x/", "x"),
            ("abc", "abc"),
            ("abc/def", "def"),
            ("a/b/.x", ".x"),
        ] {
            assert_eq!(s(base(Path::new(path))), want, "Base({path:?})");
        }
    }

    #[test]
    fn match_matches_go() {
        // Go's path/filepath matchTests
        let bad = Err(BadPattern);
        let cases: &[(&str, &str, Result<bool, BadPattern>)] = &[
            ("abc", "abc", Ok(true)),
            ("*", "abc", Ok(true)),
            ("*c", "abc", Ok(true)),
            ("a*", "a", Ok(true)),
            ("a*", "abc", Ok(true)),
            ("a*", "ab/c", Ok(false)),
            ("a*/b", "abc/b", Ok(true)),
            ("a*/b", "a/c/b", Ok(false)),
            ("a*b*c*d*e*/f", "axbxcxdxe/f", Ok(true)),
            ("a*b*c*d*e*/f", "axbxcxdxexxx/f", Ok(true)),
            ("a*b*c*d*e*/f", "axbxcxdxe/xxx/f", Ok(false)),
            ("a*b*c*d*e*/f", "axbxcxdxexxx/fff", Ok(false)),
            ("a*b?c*x", "abxbbxdbxebxczzx", Ok(true)),
            ("a*b?c*x", "abxbbxdbxebxczzy", Ok(false)),
            ("ab[c]", "abc", Ok(true)),
            ("ab[b-d]", "abc", Ok(true)),
            ("ab[e-g]", "abc", Ok(false)),
            ("ab[^c]", "abc", Ok(false)),
            ("ab[^b-d]", "abc", Ok(false)),
            ("ab[^e-g]", "abc", Ok(true)),
            ("a\\*b", "a*b", Ok(true)),
            ("a\\*b", "ab", Ok(false)),
            ("a?b", "a☺b", Ok(true)),
            ("a[^a]b", "a☺b", Ok(true)),
            ("a???b", "a☺b", Ok(false)),
            ("a[^a][^a][^a]b", "a☺b", Ok(false)),
            ("[a-ζ]*", "α", Ok(true)),
            ("*[a-ζ]", "A", Ok(false)),
            ("a?b", "a/b", Ok(false)),
            ("a*b", "a/b", Ok(false)),
            ("[\\]a]", "]", Ok(true)),
            ("[\\-]", "-", Ok(true)),
            ("[x\\-]", "x", Ok(true)),
            ("[x\\-]", "-", Ok(true)),
            ("[x\\-]", "z", Ok(false)),
            ("[\\-x]", "x", Ok(true)),
            ("[\\-x]", "-", Ok(true)),
            ("[\\-x]", "a", Ok(false)),
            ("[]a]", "]", bad.clone()),
            ("[-]", "-", bad.clone()),
            ("[x-]", "x", bad.clone()),
            ("[x-]", "-", bad.clone()),
            ("[x-]", "z", bad.clone()),
            ("[-x]", "x", bad.clone()),
            ("[-x]", "-", bad.clone()),
            ("[-x]", "a", bad.clone()),
            ("\\", "a", bad.clone()),
            ("[a-b-c]", "a", bad.clone()),
            ("[", "a", bad.clone()),
            ("[^", "a", bad.clone()),
            ("[^bc", "a", bad.clone()),
            ("a[", "a", bad.clone()),
            ("a[", "ab", bad.clone()),
            ("a[", "x", bad.clone()),
            ("a/b[", "x", bad.clone()),
            ("*x", "xxx", Ok(true)),
        ];
        for (pattern, name, want) in cases {
            assert_eq!(
                &match_pattern(OsStr::new(pattern), OsStr::new(name)),
                want,
                "Match({pattern:?}, {name:?})"
            );
        }
    }

    #[test]
    fn glob_lists_matches_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.go", "a.go", "c.txt"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/d.go"), "").unwrap();

        let root = dir.path().to_string_lossy().into_owned();
        let got: Vec<String> = glob(&dir.path().join("*.go"))
            .unwrap()
            .into_iter()
            .map(s)
            .collect();
        assert_eq!(got, [format!("{root}/a.go"), format!("{root}/b.go")]);

        let got: Vec<String> = glob(&dir.path().join("s*/*.go"))
            .unwrap()
            .into_iter()
            .map(s)
            .collect();
        assert_eq!(got, [format!("{root}/sub/d.go")]);

        assert!(glob(&dir.path().join("*.rs")).unwrap().is_empty());
        assert_eq!(glob(&dir.path().join("[")), Err(BadPattern));
    }
}
