//! Go's `path` and (Unix) `path/filepath` functions on slash-separated
//! paths
//!
//! These are lexical, like Go's: nothing here reads the file system. They
//! differ from `std::path` where it matters for a port: [`join`] appends
//! an absolute element instead of restarting from it, and every result is
//! cleaned (`..` is resolved against the preceding element).

/// `path.Clean`: the shortest path equivalent to `p` by purely lexical
/// processing (`.` and empty elements dropped, `..` resolved against the
/// element before it, no trailing slash); "." for an empty result
pub fn clean(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let b = p.as_bytes();
    let rooted = b[0] == b'/';
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.push(b'/');
        r = 1;
        dotdot = 1;
    }
    while r < n {
        if b[r] == b'/' {
            // empty element
            r += 1;
        } else if b[r] == b'.' && (r + 1 == n || b[r + 1] == b'/') {
            // . element
            r += 1;
        } else if b[r] == b'.' && b[r + 1] == b'.' && (r + 2 == n || b[r + 2] == b'/') {
            // .. element: remove to the last /
            r += 2;
            if out.len() > dotdot {
                let mut w = out.len() - 1;
                while w > dotdot && out[w] != b'/' {
                    w -= 1;
                }
                out.truncate(w);
            } else if !rooted {
                // can't backtrack, but not rooted: keep the ..
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            // a real element, after a slash if needed
            if (rooted && out.len() != 1) || (!rooted && !out.is_empty()) {
                out.push(b'/');
            }
            while r < n && b[r] != b'/' {
                out.push(b[r]);
                r += 1;
            }
        }
    }
    if out.is_empty() {
        return ".".to_string();
    }
    // Only whole elements and ASCII were copied, so this is still UTF-8
    String::from_utf8(out).expect("cleaning keeps UTF-8")
}

/// `path.Join` (and `filepath.Join` on Unix): the non-empty elements
/// joined with slashes, cleaned; "" when every element is empty
pub fn join(elems: &[&str]) -> String {
    let parts: Vec<&str> = elems.iter().copied().filter(|e| !e.is_empty()).collect();
    if parts.is_empty() {
        return String::new();
    }
    clean(&parts.join("/"))
}

/// `path.Dir`: everything but the last element, cleaned
pub fn dir(p: &str) -> String {
    let i = p.rfind('/').map_or(0, |i| i + 1);
    clean(&p[..i])
}

/// `filepath.IsAbs` on Unix
pub fn is_abs(p: &str) -> bool {
    p.starts_with('/')
}

/// `filepath.Rel` on Unix: a path that, joined to `base`, is lexically
/// `targ`; an error when there is none (one is absolute and the other not,
/// or `base` climbs out with `..` where `targ` doesn't)
pub fn rel(base_path: &str, targ_path: &str) -> Result<String, String> {
    let cant = || {
        Err(format!(
            "Rel: can't make {targ_path} relative to {base_path}"
        ))
    };
    let base = clean(base_path);
    let targ = clean(targ_path);
    if targ == base {
        return Ok(".".to_string());
    }
    let base = if base == "." { "" } else { base.as_str() };
    let targ = targ.as_str();
    if base.starts_with('/') != targ.starts_with('/') {
        return cant();
    }
    let (b, t) = (base.as_bytes(), targ.as_bytes());
    let (bl, tl) = (b.len(), t.len());
    let (mut b0, mut bi, mut t0, mut ti) = (0, 0, 0, 0);
    loop {
        while bi < bl && b[bi] != b'/' {
            bi += 1;
        }
        while ti < tl && t[ti] != b'/' {
            ti += 1;
        }
        if targ[t0..ti] != base[b0..bi] {
            break;
        }
        if bi < bl {
            bi += 1;
        }
        if ti < tl {
            ti += 1;
        }
        b0 = bi;
        t0 = ti;
    }
    if &base[b0..bi] == ".." {
        return cant();
    }
    if b0 != bl {
        // Base elements left: go up before going down
        let seps = base[b0..bl].matches('/').count();
        let mut buf = String::from("..");
        for _ in 0..seps {
            buf.push_str("/..");
        }
        if t0 != tl {
            buf.push('/');
            buf.push_str(&targ[t0..]);
        }
        return Ok(clean(&buf));
    }
    Ok(targ[t0..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_as_go() {
        // Go's path.Clean test table (src/path/path_test.go)
        let cases = [
            ("", "."),
            ("abc", "abc"),
            ("abc/def", "abc/def"),
            ("a/b/c", "a/b/c"),
            (".", "."),
            ("..", ".."),
            ("../..", "../.."),
            ("../../abc", "../../abc"),
            ("/abc", "/abc"),
            ("/", "/"),
            ("abc/", "abc"),
            ("abc/def/", "abc/def"),
            ("a/b/c/", "a/b/c"),
            ("./", "."),
            ("../", ".."),
            ("../../", "../.."),
            ("/abc/", "/abc"),
            ("abc//def//ghi", "abc/def/ghi"),
            ("//abc", "/abc"),
            ("///abc", "/abc"),
            ("//abc//", "/abc"),
            ("abc//", "abc"),
            ("abc/./def", "abc/def"),
            ("/./abc/def", "/abc/def"),
            ("abc/.", "abc"),
            ("abc/def/ghi/../jkl", "abc/def/jkl"),
            ("abc/def/../ghi/../jkl", "abc/jkl"),
            ("abc/def/..", "abc"),
            ("abc/def/../..", "."),
            ("/abc/def/../..", "/"),
            ("abc/def/../../..", ".."),
            ("/abc/def/../../..", "/"),
            ("abc/def/../../../ghi/jkl/../../../mno", "../../mno"),
            ("abc/./../def", "def"),
            ("abc//./../def", "def"),
            ("abc/../../././../def", "../../def"),
        ];
        for (input, want) in cases {
            assert_eq!(clean(input), want, "clean({input:?})");
        }
    }

    #[test]
    fn join_and_dir_as_go() {
        assert_eq!(join(&[]), "");
        assert_eq!(join(&["", ""]), "");
        assert_eq!(join(&["a", "b"]), "a/b");
        assert_eq!(join(&["a", ""]), "a");
        assert_eq!(join(&["", "b"]), "b");
        assert_eq!(join(&["/", "a"]), "/a");
        assert_eq!(join(&["/root", "/abs"]), "/root/abs");
        assert_eq!(join(&["/root", "a", "../../x"]), "/x");
        assert_eq!(dir(""), ".");
        assert_eq!(dir("go.mod"), ".");
        assert_eq!(dir("a/go.mod"), "a");
        assert_eq!(dir("/go.mod"), "/");
        assert_eq!(dir("a/b/"), "a/b");
    }

    #[test]
    fn rel_as_go() {
        // Go's filepath.Rel test table (src/path/filepath/path_test.go)
        let ok = [
            ("a/b", "a/b", "."),
            ("a/b/.", "a/b", "."),
            ("a/b", "a/b/.", "."),
            ("./a/b", "a/b", "."),
            ("a/b", "./a/b", "."),
            ("ab/cd", "ab/cde", "../cde"),
            ("ab/cd", "ab/c", "../c"),
            ("a/b", "a/b/c/d", "c/d"),
            ("a/b", "a/b/../c", "../c"),
            ("a/b/../c", "a/b", "../b"),
            ("a/b/c", "a/c/d", "../../c/d"),
            ("a/b", "c/d", "../../c/d"),
            ("a/b/c/d", "a/b", "../.."),
            ("a/b/c/d", "a/b/", "../.."),
            ("a/b/c/d/", "a/b", "../.."),
            ("a/b/c/d/", "a/b/", "../.."),
            ("../../a/b", "../../a/b/c/d", "c/d"),
            ("/a/b", "/a/b", "."),
            ("/a/b/.", "/a/b", "."),
            ("/a/b", "/a/b/.", "."),
            ("/ab/cd", "/ab/cde", "../cde"),
            ("/ab/cd", "/ab/c", "../c"),
            ("/a/b", "/a/b/c/d", "c/d"),
            ("/a/b", "/a/b/../c", "../c"),
            ("/a/b/../c", "/a/b", "../b"),
            ("/a/b/c", "/a/c/d", "../../c/d"),
            ("/a/b", "/c/d", "../../c/d"),
            ("/a/b/c/d", "/a/b", "../.."),
            ("/a/b/c/d", "/a/b/", "../.."),
            ("/a/b/c/d/", "/a/b", "../.."),
            ("/a/b/c/d/", "/a/b/", "../.."),
            ("/../../a/b", "/../../a/b/c/d", "c/d"),
            (".", "a/b", "a/b"),
            (".", "..", ".."),
        ];
        for (base, targ, want) in ok {
            assert_eq!(
                rel(base, targ).as_deref(),
                Ok(want),
                "rel({base:?}, {targ:?})"
            );
        }
        for (base, targ) in [
            ("..", "."),
            ("..", "a"),
            ("../..", ".."),
            ("a", "/a"),
            ("/a", "a"),
        ] {
            assert!(rel(base, targ).is_err(), "rel({base:?}, {targ:?})");
        }
    }
}
