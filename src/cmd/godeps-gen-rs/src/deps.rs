//! A Go module dependency, and what go.mod and go.sum say about one

use anyhow::{Result, anyhow};
use gostd::unicode::is_space;
use std::collections::HashMap;

/// A Go module dependency with its metadata
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependency {
    /// The module path, as imports name it (e.g. "github.com/google/uuid")
    pub import_path: String,
    /// The module path to fetch from, when a replace directive points at
    /// another module (a fork); empty when it is `import_path`
    pub fetch_path: String,
    /// The module version (e.g. "v1.6.0")
    pub version: String,
    /// Whether it is an indirect (transitive) dependency
    pub indirect: bool,
    /// The h1: hash from go.sum, for reference only: Nix can't fetch with it
    pub go_sum_hash: String,
    /// The SRI hash of the module's proxy zip, unpacked, which Nix fetches
    /// with
    pub nix_hash: String,
}

impl Dependency {
    /// The module path to fetch: `fetch_path`, or `import_path` without one
    pub fn effective_fetch_path(&self) -> &str {
        if self.fetch_path.is_empty() {
            &self.import_path
        } else {
            &self.fetch_path
        }
    }
}

/// Which requirements to read
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Whether indirect dependencies are included
    pub include_indirect: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            include_indirect: true,
        }
    }
}

/// The requirements of a go.mod's content, sorted by module path
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_go_mod(data: &str, opts: ParseOptions) -> Result<Vec<Dependency>> {
    let f = gomod::parse_mod("go.mod", data).map_err(|e| anyhow!("parsing go.mod: {e}"))?;
    let mut deps: Vec<Dependency> = f
        .require
        .into_iter()
        .filter(|r| !r.indirect || opts.include_indirect)
        .map(|r| Dependency {
            import_path: r.module.path,
            version: r.module.version,
            indirect: r.indirect,
            ..Default::default()
        })
        .collect();
    deps.sort_by(|a, b| a.import_path.cmp(&b.import_path));
    Ok(deps)
}

/// Go's `bufio.MaxScanTokenSize`: `ParseGoSum` reads lines with a
/// `bufio.Scanner`, which fails on a longer one
const MAX_LINE: usize = 64 * 1024;

/// The h1: source hashes of a go.sum's content, keyed "path version"
/// (the `/go.mod` hashes are left out)
pub fn parse_go_sum(data: &[u8]) -> Result<HashMap<String, String>> {
    let mut hashes = HashMap::new();
    for line in data.split_inclusive(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        if line.len() >= MAX_LINE {
            return Err(anyhow!("bufio.Scanner: token too long"));
        }
        let line = String::from_utf8_lossy(line);
        let fields: Vec<&str> = line.split(is_space).filter(|f| !f.is_empty()).collect();
        let [path, version, hash] = fields[..] else {
            continue;
        };
        if version.ends_with("/go.mod") || !hash.starts_with("h1:") {
            continue;
        }
        hashes.insert(format!("{path} {version}"), hash.to_string());
    }
    Ok(hashes)
}

/// Each dependency's go.sum hash, from hashes keyed "path version"
#[cfg_attr(not(test), allow(dead_code))]
pub fn merge_hashes(deps: &mut [Dependency], hashes: &HashMap<String, String>) {
    for dep in deps {
        if let Some(hash) = hashes.get(&format!("{} {}", dep.import_path, dep.version)) {
            dep.go_sum_hash = hash.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(deps: &[Dependency]) -> Vec<&str> {
        deps.iter().map(|d| d.import_path.as_str()).collect()
    }

    #[test]
    fn single_require() {
        let deps = parse_go_mod(
            "module example.com/mymod\n\ngo 1.21\n\nrequire github.com/foo/bar v1.0.0\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(
            deps,
            vec![Dependency {
                import_path: "github.com/foo/bar".into(),
                version: "v1.0.0".into(),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn require_block_is_sorted() {
        let deps = parse_go_mod(
            "module example.com/mymod\n\ngo 1.21\n\nrequire (\n\tgithub.com/foo/bar v1.0.0\n\tgithub.com/baz/qux v1.2.0\n)\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(
            paths(&deps),
            vec!["github.com/baz/qux", "github.com/foo/bar"]
        );
    }

    #[test]
    fn indirect_dependencies() {
        let input = "module example.com/mymod\n\ngo 1.21\n\nrequire (\n\tgithub.com/direct/dep v1.0.0\n\tgithub.com/indirect/dep v1.2.0 // indirect\n)\n";
        let deps = parse_go_mod(input, ParseOptions::default()).unwrap();
        assert_eq!(deps.len(), 2);
        let indirect = deps
            .iter()
            .find(|d| d.import_path == "github.com/indirect/dep")
            .unwrap();
        assert!(indirect.indirect);

        let deps = parse_go_mod(
            input,
            ParseOptions {
                include_indirect: false,
            },
        )
        .unwrap();
        assert_eq!(paths(&deps), vec!["github.com/direct/dep"]);
    }

    #[test]
    fn empty_module() {
        let deps = parse_go_mod(
            "module example.com/mymod\n\ngo 1.21\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn mixed_requires() {
        let deps = parse_go_mod(
            "module example.com/mymod\n\ngo 1.21\n\nrequire github.com/single/dep v1.0.0\n\nrequire (\n\tgithub.com/block/dep1 v1.1.0\n\tgithub.com/block/dep2 v1.2.0\n)\n\nrequire github.com/another/single v1.3.0\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(deps.len(), 4);
    }

    #[test]
    fn comments() {
        let deps = parse_go_mod(
            "module example.com/mymod\n\ngo 1.21\n\n// This is a comment before require\nrequire (\n\t// Comment inside block\n\tgithub.com/foo/bar v1.0.0 // trailing comment\n\tgithub.com/baz/qux v1.1.0\n)\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().all(|d| !d.indirect));
    }

    #[test]
    fn pseudo_incompatible_and_long_paths() {
        let cases = [
            ("github.com/foo/bar", "v0.0.0-20231215123456-abcdef123456"),
            ("github.com/foo/bar", "v4.0.0+incompatible"),
            ("github.com/very/long/nested/module/path/here", "v1.0.0"),
        ];
        for (path, version) in cases {
            let deps = parse_go_mod(
                &format!("module example.com/mymod\n\ngo 1.21\n\nrequire {path} {version}\n"),
                ParseOptions::default(),
            )
            .unwrap();
            assert_eq!(deps.len(), 1);
            assert_eq!(
                (deps[0].import_path.as_str(), deps[0].version.as_str()),
                (path, version)
            );
        }
    }

    #[test]
    fn invalid_syntax() {
        assert!(
            parse_go_mod(
                "this is not valid go.mod syntax at all",
                ParseOptions::default()
            )
            .is_err()
        );
    }

    #[test]
    fn sorted() {
        let deps = parse_go_mod(
            "module example.com/mymod\n\ngo 1.21\n\nrequire (\n\tgithub.com/zzz/last v1.0.0\n\tgithub.com/aaa/first v1.0.0\n\tgithub.com/mmm/middle v1.0.0\n)\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(
            paths(&deps),
            vec![
                "github.com/aaa/first",
                "github.com/mmm/middle",
                "github.com/zzz/last"
            ]
        );
    }

    #[test]
    fn go_sum_entries() {
        let hashes = parse_go_sum(b"github.com/foo/bar v1.0.0 h1:abcdef123456=\n").unwrap();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes["github.com/foo/bar v1.0.0"], "h1:abcdef123456=");

        // Only source hashes, not /go.mod ones
        let hashes = parse_go_sum(b"github.com/foo/bar v1.0.0 h1:hash1=\ngithub.com/foo/bar v1.0.0/go.mod h1:modhash=\ngithub.com/baz/qux v2.0.0 h1:hash2=\ngithub.com/baz/qux v2.0.0/go.mod h1:modhash2=\n",
        )
        .unwrap();
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes["github.com/foo/bar v1.0.0"], "h1:hash1=");
        assert_eq!(hashes["github.com/baz/qux v2.0.0"], "h1:hash2=");

        assert!(parse_go_sum(b"").unwrap().is_empty());

        let hashes = parse_go_sum(
            b"\ngithub.com/foo/bar v1.0.0 h1:hash=\n\ngithub.com/baz/qux v2.0.0 h1:hash2=\n\n",
        )
        .unwrap();
        assert_eq!(hashes.len(), 2);

        let hashes = parse_go_sum(b"github.com/foo/bar v1.0.0 h1:hash1=\ngithub.com/foo/bar v1.1.0 h1:hash2=\ngithub.com/foo/bar v2.0.0 h1:hash3=\n",
        )
        .unwrap();
        assert_eq!(hashes.len(), 3);
        assert_eq!(hashes["github.com/foo/bar v1.1.0"], "h1:hash2=");

        // Only h1: hashes
        let hashes = parse_go_sum(b"github.com/foo/bar v1.0.0 h1:validhash=\ngithub.com/baz/qux v1.0.0 sha256:notavalidformat\n",
        )
        .unwrap();
        assert_eq!(hashes.len(), 1);
    }

    #[test]
    fn go_sum_lines_as_bufio_reads_them() {
        // CRLF line ends, and a line too long for bufio.Scanner
        let hashes = parse_go_sum(b"a.com/b v1.0.0 h1:x=\r\n").unwrap();
        assert_eq!(hashes["a.com/b v1.0.0"], "h1:x=");
        assert!(parse_go_sum("x".repeat(MAX_LINE - 1).as_bytes()).is_ok());
        assert!(parse_go_sum("x".repeat(MAX_LINE).as_bytes()).is_err());
    }

    #[test]
    fn merging_hashes() {
        let mut deps = vec![
            Dependency {
                import_path: "github.com/foo/bar".into(),
                version: "v1.0.0".into(),
                ..Default::default()
            },
            Dependency {
                import_path: "github.com/missing/hash".into(),
                version: "v2.0.0".into(),
                ..Default::default()
            },
        ];
        let hashes = HashMap::from([(
            "github.com/foo/bar v1.0.0".to_string(),
            "h1:foohash=".to_string(),
        )]);
        merge_hashes(&mut deps, &hashes);
        assert_eq!(deps[0].go_sum_hash, "h1:foohash=");
        assert_eq!(deps[1].go_sum_hash, "");

        merge_hashes(&mut deps[1..], &HashMap::new());
        assert_eq!(deps[1].go_sum_hash, "");
    }

    #[test]
    fn effective_fetch_path() {
        let dep = |import_path: &str, fetch_path: &str| Dependency {
            import_path: import_path.into(),
            fetch_path: fetch_path.into(),
            ..Default::default()
        };
        assert_eq!(
            dep("github.com/foo/bar", "").effective_fetch_path(),
            "github.com/foo/bar"
        );
        assert_eq!(
            dep("github.com/original/pkg", "github.com/fork/pkg").effective_fetch_path(),
            "github.com/fork/pkg"
        );
    }
}
