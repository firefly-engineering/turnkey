//! Which files a Go build includes: their build constraints and their
//! names' `_GOOS` / `_GOARCH` suffixes, evaluated as `go/build` evaluates
//! them

use crate::GoFile;

/// What a file's build constraints are evaluated against, as `go/build`
/// evaluates them for a build
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildContext {
    /// The target's GOOS, e.g. `linux`
    pub goos: String,
    /// The target's GOARCH, e.g. `arm64`
    pub goarch: String,
    /// Whether cgo is on: the cgo tag is set, and files importing "C" are
    /// part of the build
    pub cgo_enabled: bool,
    /// The toolchain's Go version, e.g. `1.24`: the release tags go1.1 up
    /// to it are set
    pub go_version: String,
    /// The build tags set on the build (`-tags`)
    pub tags: Vec<String>,
}

/// The GOOS values the unix tag is set for (go/build's list)
const UNIX_OS: &[&str] = &[
    "aix",
    "android",
    "darwin",
    "dragonfly",
    "freebsd",
    "hurd",
    "illumos",
    "ios",
    "linux",
    "netbsd",
    "openbsd",
    "solaris",
];

/// The GOOS values a file name's suffix can name
const KNOWN_OS: &[&str] = &[
    "aix",
    "android",
    "darwin",
    "dragonfly",
    "freebsd",
    "hurd",
    "illumos",
    "ios",
    "js",
    "linux",
    "nacl",
    "netbsd",
    "openbsd",
    "plan9",
    "solaris",
    "windows",
    "zos",
];

/// The GOARCH values a file name's suffix can name
const KNOWN_ARCH: &[&str] = &[
    "386",
    "amd64",
    "amd64p32",
    "arm",
    "armbe",
    "arm64",
    "arm64be",
    "ppc64",
    "ppc64le",
    "mips",
    "mipsle",
    "mips64",
    "mips64le",
    "mips64p32",
    "mips64p32le",
    "ppc",
    "riscv",
    "riscv64",
    "s390",
    "s390x",
    "sparc",
    "sparc64",
    "wasm",
];

impl BuildContext {
    /// Whether a file is part of the build: cgo is on if it imports "C",
    /// its build constraint holds, and its name's `_GOOS` / `_GOARCH`
    /// suffixes match
    pub fn matches(&self, file: &GoFile) -> bool {
        if file.has_cgo && !self.cgo_enabled {
            return false;
        }
        if let Some(c) = &file.constraint
            && !c.eval(&mut |tag: &str| self.has_tag(tag))
        {
            return false;
        }
        let name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (os_tag, arch_tag) = parse_filename_constraint(&name);
        if !os_tag.is_empty() && !self.has_tag(os_tag) {
            return false;
        }
        if !arch_tag.is_empty() && arch_tag != self.goarch {
            return false;
        }
        true
    }

    /// Whether a build tag is set, as go/build decides: GOOS, GOARCH, unix
    /// on a Unix, cgo, gc (the compiler), the release tags up to the Go
    /// version, and the build's own tags. android implies linux, and ios
    /// darwin.
    pub fn has_tag(&self, tag: &str) -> bool {
        if tag == self.goos || tag == self.goarch || tag == "gc" {
            return true;
        }
        if tag == "unix" {
            return UNIX_OS.contains(&self.goos.as_str());
        }
        if tag == "cgo" {
            return self.cgo_enabled;
        }
        if (tag == "linux" && self.goos == "android") || (tag == "darwin" && self.goos == "ios") {
            return true;
        }
        if let Some(minor) = release_minor(tag) {
            return release_minor(&format!("go{}", self.go_version))
                .is_some_and(|current| minor <= current);
        }
        self.tags.iter().any(|t| t == tag)
    }
}

/// N for a release tag go1.N (or a version go1.N.P), as `strconv.Atoi`
/// reads N
fn release_minor(tag: &str) -> Option<i64> {
    let rest = tag.strip_prefix("go1.")?;
    let rest = rest.split_once('.').map_or(rest, |(minor, _)| minor);
    rest.parse().ok()
}

/// The GOOS and GOARCH a file name's suffixes constrain it to (each ""
/// when it names none): `_GOOS_GOARCH`, `_GOOS` or `_GOARCH` before
/// `.go`, or before `_test.go`
pub fn parse_filename_constraint(filename: &str) -> (&str, &str) {
    let name = filename.strip_suffix(".go").unwrap_or(filename);
    let name = name.strip_suffix("_test").unwrap_or(name);

    let parts: Vec<&str> = name.split('_').collect();
    if parts.len() < 2 {
        return ("", "");
    }
    let last = parts[parts.len() - 1];
    if parts.len() >= 3 {
        let prev = parts[parts.len() - 2];
        if KNOWN_OS.contains(&prev) && KNOWN_ARCH.contains(&last) {
            return (prev, last);
        }
    }
    if KNOWN_OS.contains(&last) {
        return (last, "");
    }
    if KNOWN_ARCH.contains(&last) {
        return ("", last);
    }
    ("", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gostd::constraint::parse;

    #[test]
    fn filename_constraints() {
        for (filename, os, arch) in [
            ("foo.go", "", ""),
            ("foo_linux.go", "linux", ""),
            ("foo_amd64.go", "", "amd64"),
            ("foo_linux_amd64.go", "linux", "amd64"),
            ("foo_test.go", "", ""),
            ("foo_linux_test.go", "linux", ""),
        ] {
            assert_eq!(
                parse_filename_constraint(filename),
                (os, arch),
                "{filename}"
            );
        }
    }

    fn ctx(goos: &str, goarch: &str, cgo: bool, version: &str, tags: &[&str]) -> BuildContext {
        BuildContext {
            goos: goos.into(),
            goarch: goarch.into(),
            cgo_enabled: cgo,
            go_version: version.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    fn file(path: &str, build: &str, has_cgo: bool) -> GoFile {
        GoFile {
            path: path.into(),
            constraint: (!build.is_empty())
                .then(|| parse(&format!("//go:build {build}")).expect("a constraint")),
            has_cgo,
            ..GoFile::default()
        }
    }

    #[test]
    fn build_context_matches() {
        let linux_amd64 = ctx("linux", "amd64", true, "1.24", &[]);
        let darwin_arm64 = ctx("darwin", "arm64", true, "1.24", &[]);
        let windows = ctx("windows", "amd64", false, "1.24", &[]);
        let tagged = ctx("linux", "amd64", false, "1.24", &["integration"]);
        let old = ctx("linux", "amd64", false, "1.20", &[]);

        for (name, f, ctx, want) in [
            ("plain", file("foo.go", "", false), &linux_amd64, true),
            (
                "_linux on linux",
                file("foo_linux.go", "", false),
                &linux_amd64,
                true,
            ),
            (
                "_linux on darwin",
                file("foo_linux.go", "", false),
                &darwin_arm64,
                false,
            ),
            (
                "_amd64",
                file("foo_amd64.go", "", false),
                &linux_amd64,
                true,
            ),
            (
                "_windows_amd64 on linux",
                file("foo_windows_amd64.go", "", false),
                &linux_amd64,
                false,
            ),
            (
                "unix on linux",
                file("u.go", "unix", false),
                &linux_amd64,
                true,
            ),
            (
                "unix on macos",
                file("u.go", "unix", false),
                &darwin_arm64,
                true,
            ),
            (
                "unix on windows",
                file("u.go", "unix", false),
                &windows,
                false,
            ),
            (
                "go1.21 with 1.24",
                file("v.go", "go1.21", false),
                &linux_amd64,
                true,
            ),
            (
                "go1.21 with 1.20",
                file("v.go", "go1.21", false),
                &old,
                false,
            ),
            (
                "!go1.21 with 1.24",
                file("v.go", "!go1.21", false),
                &linux_amd64,
                false,
            ),
            (
                "custom tag unset",
                file("i.go", "integration", false),
                &linux_amd64,
                false,
            ),
            (
                "custom tag set",
                file("i.go", "integration", false),
                &tagged,
                true,
            ),
            ("cgo on", file("c.go", "cgo", false), &linux_amd64, true),
            ("cgo off", file("c.go", "cgo", false), &windows, false),
            (
                "import C without cgo",
                file("c.go", "", true),
                &windows,
                false,
            ),
            (
                "gc",
                file("g.go", "gc && !gccgo", false),
                &linux_amd64,
                true,
            ),
        ] {
            assert_eq!(ctx.matches(&f), want, "{name}");
        }
    }

    /// Release tags are read as Go reads them: go1.N.P is go1.N, and an N
    /// strconv.Atoi doesn't read is no release tag
    #[test]
    fn release_tags() {
        let c = ctx("linux", "amd64", true, "1.24", &["go1.x"]);
        assert!(c.has_tag("go1.24") && c.has_tag("go1.24.9") && c.has_tag("go1.+3"));
        assert!(!c.has_tag("go1.25") && !c.has_tag("go1.25rc1"));
        assert!(c.has_tag("go1.x"));
        let no_version = ctx("linux", "amd64", true, "", &[]);
        assert!(!no_version.has_tag("go1.1"));
        assert!(ctx("android", "arm64", true, "1.24", &[]).has_tag("linux"));
        assert!(ctx("ios", "arm64", true, "1.24", &[]).has_tag("darwin"));
    }
}
