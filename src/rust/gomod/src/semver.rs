//! Go's semantic versions: a port of `golang.org/x/mod/semver`'s parsing
//!
//! Go versions are SemVer 2.0.0 with a leading `v`, and accept the
//! shorthands `vMAJOR` and `vMAJOR.MINOR` (for `.0.0` and `.0`). The
//! `semver` crate is a different dialect (no `v`, no shorthands, and
//! build metadata takes part in its ordering), so it can't stand in.

/// The parts of a valid version, as slices of it
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Parsed<'a> {
    major: &'a str,
    /// What `canonical` appends to a shorthand: ".0.0", ".0" or ""
    short: &'static str,
    /// "+build", or ""
    build: &'a str,
}

/// `semver.IsValid`
pub fn is_valid(v: &str) -> bool {
    parse(v).is_some()
}

/// `semver.Canonical`: `v` with a shorthand completed and build metadata
/// dropped; "" when `v` is not a valid version
pub fn canonical(v: &str) -> String {
    match parse(v) {
        None => String::new(),
        Some(p) if !p.build.is_empty() => v[..v.len() - p.build.len()].to_string(),
        Some(p) => format!("{v}{}", p.short),
    }
}

/// `semver.Major`: "vN" for a valid version, "" otherwise
pub fn major(v: &str) -> &str {
    match parse(v) {
        Some(p) => &v[..1 + p.major.len()],
        None => "",
    }
}

/// `semver.Build`: the build metadata ("+..."), or ""
pub fn build(v: &str) -> &str {
    parse(v).map_or("", |p| p.build)
}

fn parse(v: &str) -> Option<Parsed<'_>> {
    let rest = v.strip_prefix('v')?;
    let mut p = Parsed::default();
    let (major, rest) = parse_int(rest)?;
    p.major = major;
    if rest.is_empty() {
        p.short = ".0.0";
        return Some(p);
    }
    let (_minor, rest) = parse_int(rest.strip_prefix('.')?)?;
    if rest.is_empty() {
        p.short = ".0";
        return Some(p);
    }
    let (_patch, mut rest) = parse_int(rest.strip_prefix('.')?)?;
    if rest.starts_with('-') {
        let (_prerelease, r) = parse_prerelease(rest)?;
        rest = r;
    }
    if rest.starts_with('+') {
        let (build, r) = parse_build(rest)?;
        p.build = build;
        rest = r;
    }
    rest.is_empty().then_some(p)
}

/// A decimal number without leading zeros, and the rest
fn parse_int(v: &str) -> Option<(&str, &str)> {
    let b = v.as_bytes();
    if b.is_empty() || !b[0].is_ascii_digit() {
        return None;
    }
    let i = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if b[0] == b'0' && i != 1 {
        return None;
    }
    Some((&v[..i], &v[i..]))
}

/// "-" and dot-separated identifiers up to a "+" or the end, numeric ones
/// without leading zeros
fn parse_prerelease(v: &str) -> Option<(&str, &str)> {
    let b = v.as_bytes();
    if b.first() != Some(&b'-') {
        return None;
    }
    let (mut i, mut start) = (1, 1);
    while i < b.len() && b[i] != b'+' {
        if !is_ident_char(b[i]) && b[i] != b'.' {
            return None;
        }
        if b[i] == b'.' {
            if start == i || is_bad_num(&v[start..i]) {
                return None;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start == i || is_bad_num(&v[start..i]) {
        return None;
    }
    Some((&v[..i], &v[i..]))
}

/// "+" and dot-separated identifiers to the end
fn parse_build(v: &str) -> Option<(&str, &str)> {
    let b = v.as_bytes();
    if b.first() != Some(&b'+') {
        return None;
    }
    let (mut i, mut start) = (1, 1);
    while i < b.len() {
        if !is_ident_char(b[i]) && b[i] != b'.' {
            return None;
        }
        if b[i] == b'.' {
            if start == i {
                return None;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start == i {
        return None;
    }
    Some((&v[..i], &v[i..]))
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}

/// A number with a leading zero
fn is_bad_num(v: &str) -> bool {
    v.len() > 1 && v.bytes().all(|c| c.is_ascii_digit()) && v.starts_with('0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_as_go() {
        // golang.org/x/mod/semver's test table: (input, canonical form, ""
        // when invalid)
        let cases = [
            ("bad", ""),
            ("v1-alpha.beta.gamma", ""),
            ("v1-pre", ""),
            ("v1+meta", ""),
            ("v1-pre+meta", ""),
            ("v1.2-pre", ""),
            ("v1.2+meta", ""),
            ("v1.2-pre+meta", ""),
            ("v1.0.0-alpha", "v1.0.0-alpha"),
            ("v1.0.0-alpha.1", "v1.0.0-alpha.1"),
            ("v1.0.0-alpha.beta", "v1.0.0-alpha.beta"),
            ("v1.0.0-beta", "v1.0.0-beta"),
            ("v1.0.0-beta.2", "v1.0.0-beta.2"),
            ("v1.0.0-beta.11", "v1.0.0-beta.11"),
            ("v1.0.0-rc.1", "v1.0.0-rc.1"),
            ("v1", "v1.0.0"),
            ("v1.0", "v1.0.0"),
            ("v1.0.0", "v1.0.0"),
            ("v1.2", "v1.2.0"),
            ("v1.2.0", "v1.2.0"),
            ("v1.2.3-456", "v1.2.3-456"),
            ("v1.2.3-456.789", "v1.2.3-456.789"),
            ("v1.2.3-456-789", "v1.2.3-456-789"),
            ("v1.2.3-456a", "v1.2.3-456a"),
            ("v1.2.3-pre", "v1.2.3-pre"),
            ("v1.2.3-pre+meta", "v1.2.3-pre"),
            ("v1.2.3-pre.1", "v1.2.3-pre.1"),
            ("v1.2.3-zzz", "v1.2.3-zzz"),
            ("v1.2.3", "v1.2.3"),
            ("v1.2.3+meta", "v1.2.3"),
            ("v1.2.3+meta-pre", "v1.2.3"),
            ("v1.2.3+meta-pre.sha.256a", "v1.2.3"),
            ("v1.2.3-01", ""),
            ("v1.2.3-pre.01", ""),
            ("v01.2.3", ""),
            ("v1.02.3", ""),
            ("v1.2.03", ""),
            ("v1.2.3+", ""),
            ("v1.2.3+a..b", ""),
            ("v1.2.3-", ""),
            ("1.2.3", ""),
            ("", ""),
        ];
        for (input, want) in cases {
            assert_eq!(canonical(input), want, "canonical({input:?})");
            assert_eq!(is_valid(input), !want.is_empty(), "is_valid({input:?})");
        }
    }

    #[test]
    fn major_and_build() {
        assert_eq!(major("v1.2.3"), "v1");
        assert_eq!(major("v12"), "v12");
        assert_eq!(major("v0.0.0-20231215123456-abcdef123456"), "v0");
        assert_eq!(major("bad"), "");
        assert_eq!(build("v4.0.0+incompatible"), "+incompatible");
        assert_eq!(build("v1.2.3"), "");
        assert_eq!(build("v1.2+x"), "");
    }
}
