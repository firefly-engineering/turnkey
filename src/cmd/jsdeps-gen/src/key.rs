//! pnpm's package keys, and the dependency values that resolve to them
//!
//! A pnpm v9 snapshot key is `name@version`, then zero or more balanced
//! `( … )` groups: a peer resolution (`(react@18.2.0)`), which can nest
//! (`(@types/react@18.0.0(react@18.2.0))`), or a patch (`(patch_hash=…)`).
//! The name is scoped (`@scope/name`) or not. The groups are kept as
//! written: they are what tells two instances of one package apart.
//!
//! A dependency's value in a snapshot or an importer is one of:
//! - a version, maybe with groups (`18.2.0(react@18.2.0)`): the instance
//!   is `<import name>@<value>`;
//! - an alias, a whole key (`string-width@4.2.3`), when the package is
//!   imported under another name;
//! - `link:` or `file:` a local path, which jsdeps-gen doesn't support.

use anyhow::{Result, bail};

/// A pnpm package key, parsed: each part borrows from the key
#[derive(Debug, PartialEq, Eq)]
pub struct Key<'a> {
    /// The npm name, with its scope if any (`@types/node`)
    pub name: &'a str,
    /// The version, up to the first group
    pub version: &'a str,
    /// Each top-level group's contents, without its parentheses; a nested
    /// group stays inside its parent's
    pub groups: Vec<&'a str>,
}

/// Parses a key: `name@version`, then balanced `( … )` groups up to its end
pub fn parse(key: &str) -> Result<Key<'_>> {
    let Some((name, rest)) = split_name(key) else {
        bail!("`{key}` is not a pnpm package key: no package name before its version");
    };
    let Some(rest) = rest.strip_prefix('@') else {
        bail!("`{key}` is not a pnpm package key: no `@` after the name `{name}`");
    };
    let version_end = rest.find(['(', ')']).unwrap_or(rest.len());
    let version = &rest[..version_end];
    if version.is_empty() {
        bail!("`{key}` is not a pnpm package key: no version after `{name}@`");
    }
    let groups = parse_groups(key, &rest[version_end..])?;
    Ok(Key {
        name,
        version,
        groups,
    })
}

/// The instance key a dependency's value resolves to, when the package
/// imports it as `import_name`: the value itself for an alias, and
/// `<import_name>@<value>` otherwise. Fails on a `link:` or `file:` value,
/// and on a value that doesn't make a key.
pub fn resolve(import_name: &str, value: &str) -> Result<String> {
    for protocol in ["link:", "file:"] {
        if value.starts_with(protocol) {
            bail!(
                "`{import_name}` is `{value}`: {protocol} dependencies aren't supported, \
                 only packages from a registry"
            );
        }
    }
    let key = if is_alias(value) {
        value.to_string()
    } else {
        format!("{import_name}@{value}")
    };
    parse(&key)?;
    Ok(key)
}

/// Whether a dependency's value is a whole key (an alias), rather than a
/// version: it starts with a package name and an `@`. No version does: a
/// semver version holds none of `@`, a URL or git version stops being a
/// name at its `:` or `+`, and a peer's `@` is inside a group.
fn is_alias(value: &str) -> bool {
    split_name(value).is_some_and(|(_, rest)| rest.starts_with('@'))
}

/// Splits a package name off the front of `s`, scoped or not, returning it
/// and the rest; `None` if `s` doesn't start with one
fn split_name(s: &str) -> Option<(&str, &str)> {
    let len = match s.strip_prefix('@') {
        Some(scoped) => {
            let scope = name_part_len(scoped);
            let after_scope = scoped[scope..].strip_prefix('/')?;
            let name = name_part_len(after_scope);
            if scope == 0 || name == 0 {
                return None;
            }
            1 + scope + 1 + name
        }
        None => match name_part_len(s) {
            0 => return None,
            len => len,
        },
    };
    Some(s.split_at(len))
}

/// The length of the longest prefix of `s` that can be a scope or a name:
/// the characters npm allows there (lowercase in new packages, though old
/// ones have uppercase too)
fn name_part_len(s: &str) -> usize {
    s.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')))
        .unwrap_or(s.len())
}

/// Parses the groups after a key's version: each `(` opens one, closed by
/// its matching `)`, and nothing may sit between or after them
fn parse_groups<'a>(key: &str, mut rest: &'a str) -> Result<Vec<&'a str>> {
    let mut groups = Vec::new();
    while !rest.is_empty() {
        if !rest.starts_with('(') {
            bail!("`{key}` is not a pnpm package key: `{rest}` isn't a `( … )` group");
        }
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            bail!("`{key}` is not a pnpm package key: `{rest}` has an unclosed `(`");
        };
        groups.push(&rest[1..close]);
        rest = &rest[close + 1..];
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unscoped() {
        let key = parse("lodash@4.17.21").unwrap();
        assert_eq!(
            key,
            Key {
                name: "lodash",
                version: "4.17.21",
                groups: vec![],
            }
        );
    }

    #[test]
    fn test_parse_scoped_with_peer() {
        let key = parse("@babel/core@7.26.0(@swc/core@1.10.14)").unwrap();
        assert_eq!(key.name, "@babel/core");
        assert_eq!(key.version, "7.26.0");
        assert_eq!(key.groups, vec!["@swc/core@1.10.14"]);
    }

    #[test]
    fn test_parse_nested_groups() {
        let key =
            parse("@testing-library/react@14.0.0(@types/react@18.2.0(react@18.2.0))(react@18.2.0)")
                .unwrap();
        assert_eq!(key.name, "@testing-library/react");
        assert_eq!(key.version, "14.0.0");
        assert_eq!(
            key.groups,
            vec!["@types/react@18.2.0(react@18.2.0)", "react@18.2.0"]
        );
        // A nested group is a key of its own
        let inner = parse(key.groups[0]).unwrap();
        assert_eq!(inner.name, "@types/react");
        assert_eq!(inner.groups, vec!["react@18.2.0"]);
    }

    #[test]
    fn test_parse_patch_hash_group() {
        let key = parse("left-pad@1.3.0(patch_hash=abc123)(react@18.2.0)").unwrap();
        assert_eq!(key.groups, vec!["patch_hash=abc123", "react@18.2.0"]);
    }

    #[test]
    fn test_parse_prerelease_is_not_the_release() {
        let release = parse("foo@1.0.0").unwrap();
        let beta = parse("foo@1.0.0-beta.1(bar@2.0.0)").unwrap();
        assert_eq!(release.version, "1.0.0");
        assert_eq!(beta.version, "1.0.0-beta.1");
        assert_eq!(beta.groups, vec!["bar@2.0.0"]);
    }

    #[test]
    fn test_parse_git_version() {
        let key = parse("foo@git+ssh://git@github.com/o/foo.git#abc(bar@1.0.0)").unwrap();
        assert_eq!(key.name, "foo");
        assert_eq!(key.version, "git+ssh://git@github.com/o/foo.git#abc");
        assert_eq!(key.groups, vec!["bar@1.0.0"]);
    }

    #[test]
    fn test_parse_rejects_malformed() {
        for bad in [
            "lodash",
            "@types/node",
            "@types@1.0.0",
            "lodash@",
            "lodash@(react@18.2.0)",
            "a@1.0.0(b@2.0.0",
            "a@1.0.0(b@2.0.0))",
            "a@1.0.0(b@2.0.0)x",
            "a@1.0)",
        ] {
            assert!(parse(bad).is_err(), "`{bad}` parsed");
        }
    }

    #[test]
    fn test_resolve_version() {
        assert_eq!(resolve("braces", "3.0.3").unwrap(), "braces@3.0.3");
        assert_eq!(
            resolve("react-dom", "18.2.0(react@18.2.0)").unwrap(),
            "react-dom@18.2.0(react@18.2.0)"
        );
    }

    #[test]
    fn test_resolve_alias() {
        assert_eq!(
            resolve("string-width-cjs", "string-width@4.2.3").unwrap(),
            "string-width@4.2.3"
        );
        assert_eq!(
            resolve("types-react", "@types/react@18.2.0(react@18.2.0)").unwrap(),
            "@types/react@18.2.0(react@18.2.0)"
        );
    }

    #[test]
    fn test_resolve_rejects_local_paths() {
        let err = resolve("mylib", "link:../mylib").unwrap_err().to_string();
        assert!(
            err.contains("mylib") && err.contains("link:../mylib"),
            "{err}"
        );
        let err = resolve("vendored", "file:vendor/x.tgz")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("vendored") && err.contains("file:vendor/x.tgz"),
            "{err}"
        );
    }
}
